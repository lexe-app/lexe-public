//! # Provisioning a new lexe node
//!
//! This module is responsible for running the node provisioning process for new
//! users and for existing users upgrading to new enclave versions.
//!
//! The intention of the provisioning process is for users to transfer their
//! secure secrets into a trusted enclave version with the operator (lexe)
//! learning their secrets. These secrets include sensitive data like wallet
//! private keys or mTLS certificates.
//!
//! A node enclave must also convince the user that the software is a version
//! that they trust and the software is running inside an up-to-date secure
//! enclave. We do this using a variant of RA-TLS (Remote Attestation TLS),
//! where the enclave platform endorsements and enclave measurements are bundled
//! in&&to a self-signed TLS certificate, which users must verify when
//! connecting to the provisioning endpoint.

use std::{
    convert::Infallible,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use common::{
    api::{
        auth::BearerAuthenticator,
        def::NodeRunnerApi,
        error::{NodeApiError, NodeErrorKind},
        ports::UserPorts,
        provision::{NodeProvisionRequest, SealedSeed},
        rest, Empty, UserPk,
    },
    cli::node::ProvisionArgs,
    client::tls,
    enclave,
    enclave::Measurement,
    rng::{Crng, SysRng},
    shutdown::ShutdownChannel,
};
use tracing::{debug, info, instrument, warn, Span};
use warp::{filters::BoxedFilter, http::Response, hyper::Body, Filter};

use crate::{api::BackendApiClient, persister};

const PROVISION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
struct RequestContext {
    current_user_pk: UserPk,
    measurement: Measurement,
    shutdown: ShutdownChannel,
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    // TODO(phlip9): make generic, use test rng in test
    rng: SysRng,
}

/// Makes a [`RequestContext`] available to subsequent [`Filter`]s.
fn with_request_context(
    ctx: RequestContext,
) -> impl Filter<Extract = (RequestContext,), Error = Infallible> + Clone {
    warp::any().map(move || ctx.clone())
}

/// Provision a user node.
///
/// The `UserPk` is given by the runner so we know which user we should
/// provision to and have a simple method to authenticate their connection.
#[instrument(skip_all, parent = None, name = "(node-provision)")]
pub async fn provision_node<R: Crng>(
    rng: &mut R,
    args: ProvisionArgs,
    runner_api: Arc<dyn NodeRunnerApi + Send + Sync>,
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
) -> anyhow::Result<()> {
    debug!(%args.user_pk, args.port, %args.node_dns_name, "provisioning");

    // Set up the request context and warp routes.
    let measurement = enclave::measurement();
    let mut shutdown = ShutdownChannel::new();
    let ctx = RequestContext {
        current_user_pk: args.user_pk,
        measurement,
        shutdown: shutdown.clone(),
        backend_api,
        // TODO(phlip9): use passed in rng
        rng: SysRng::new(),
    };
    let routes = app_routes(ctx);
    // TODO(phlip9): remove when rest::serve_* supports TLS
    let routes = routes.with(rest::trace_requests(Span::current().id()));

    // Set up the TLS config.
    let tls_config = tls::node_provision_tls_config(rng, args.node_dns_name)
        .context("Failed to build TLS config for provisioning")?;

    // Set up a shutdown future that completes when either:
    //
    // a) Provisioning succeeds and `provision_handler` sends a shutdown signal
    // b) Provisioning times out.
    //
    // This future is passed into and driven by the warp service, allowing it to
    // gracefully shut down once either of these conditions has been reached.
    let warp_shutdown_fut = async move {
        tokio::select! {
            () = shutdown.recv() => info!("Provision succeeded"),
            _ = tokio::time::sleep(PROVISION_TIMEOUT) => {
                warn!("Timed out waiting for successful provision request");
                // Send a shutdown signal just in case we later add another task
                // that depends on the timeout
                shutdown.send();
            }
        }
    };

    // Finally, set up the warp service, passing in the above components.
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port.unwrap_or(0)));
    let (listen_addr, service) = warp::serve(routes)
        .tls()
        .preconfigured_tls(tls_config)
        .bind_with_graceful_shutdown(addr, warp_shutdown_fut);
    let app_port = listen_addr.port();
    info!(%listen_addr, "listening for connections");

    // Notify the runner that we're ready for a client connection
    let user_ports = UserPorts::new_provision(args.user_pk, app_port);
    runner_api
        .ready(user_ports)
        .await
        .context("Failed to notify runner of our readiness")?;
    debug!("Notified runner; awaiting client request");

    // Drive the warp service, wait for finish
    service.await;

    Ok(())
}

/// Implements [`AppNodeProvisionApi`] - only callable by the node owner.
///
/// [`AppNodeProvisionApi`]: common::api::def::AppNodeProvisionApi
fn app_routes(ctx: RequestContext) -> BoxedFilter<(Response<Body>,)> {
    let provision = warp::path::path("provision")
        .and(warp::post())
        .and(with_request_context(ctx))
        .and(warp::body::json::<NodeProvisionRequest>())
        .then(provision_handler)
        .map(rest::into_response);

    let routes = warp::path("app").and(provision);

    routes.boxed()
}

/// Handles a provision request.
///
/// POST /provision [`NodeProvisionRequest`] -> ()
async fn provision_handler(
    mut ctx: RequestContext,
    req: NodeProvisionRequest,
) -> Result<Empty, NodeApiError> {
    debug!("Received provision request");

    // Validation: the user pk derived from the given root seed should match the
    // user pk given in our CLI args. This is a sanity check in case e.g. the
    // provision request was routed to the wrong user node.
    let user_key_pair = req.root_seed.derive_user_key_pair();
    let derived_user_pk =
        UserPk::from_ref(user_key_pair.public_key().as_inner());
    if derived_user_pk != &ctx.current_user_pk {
        return Err(NodeApiError::wrong_user_pk(
            ctx.current_user_pk,
            *derived_user_pk,
        ));
    }

    let sealed_seed_res = SealedSeed::seal_from_root_seed(
        &mut ctx.rng,
        &req.root_seed,
        req.deploy_env,
        req.network,
        ctx.measurement,
        enclave::machine_id(),
    );

    let sealed_seed = sealed_seed_res.map_err(|err| NodeApiError {
        kind: NodeErrorKind::Provision,
        msg: format!("{err:#}"),
    })?;

    // TODO(phlip9): [perf] could get the user to pass us their auth token in
    // the provision request instead of reauthing here.

    // authenticate as the user to the backend
    let authenticator =
        BearerAuthenticator::new(user_key_pair, None /* maybe_token */);
    let token = authenticator
        .get_token(ctx.backend_api.as_ref(), SystemTime::now())
        .await
        .map_err(|err| NodeApiError {
            kind: NodeErrorKind::BadAuth,
            msg: format!("{err:#}"),
        })?;

    // store the sealed seed and new node metadata in the backend
    ctx.backend_api
        .create_sealed_seed(sealed_seed, token.clone())
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Provision,
            msg: format!("Could not persist sealed seed: {e:#}"),
        })?;

    // Encrypt the GDriveCredentials and upsert into Lexe's untrusted DB.
    let vfs_master_key = req.root_seed.derive_vfs_master_key();
    persister::persist_gdrive_credentials(
        &mut ctx.rng,
        ctx.backend_api.as_ref(),
        &vfs_master_key,
        &req.gdrive_credentials,
        token,
    )
    .await?;

    // Provisioning done. Stop the node.
    ctx.shutdown.send();

    Ok(Empty {})
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use common::{
        api::provision::GDriveCredentials,
        attest,
        attest::verify::EnclavePolicy,
        cli::{node::ProvisionArgs, Network},
        env::DeployEnv,
        rng::WeakRng,
        root_seed::RootSeed,
    };
    use tokio_rustls::rustls;

    use super::*;
    use crate::api::mock::{MockBackendClient, MockRunnerClient};

    #[cfg(target_env = "sgx")]
    #[test]
    #[ignore] // << uncomment to dump fresh attestation cert
    fn dump_attest_cert() {
        use common::ed25519;

        let mut rng = WeakRng::new();
        let cert_key_pair = ed25519::KeyPair::from_seed(&[0x42; 32]);
        let cert_pk = cert_key_pair.public_key();
        let attestation = attest::quote_enclave(&mut rng, cert_pk).unwrap();
        let dns_names = vec!["localhost".to_string()];

        let attest_cert = attest::AttestationCert::new(
            cert_key_pair.to_rcgen(),
            dns_names,
            attestation,
        )
        .unwrap();

        println!("measurement: '{}'", enclave::measurement());
        println!("cert_pk: '{cert_pk}'");

        let cert_der = attest_cert.serialize_der_signed().unwrap();

        println!("attestation certificate:");
        println!("-----BEGIN CERTIFICATE-----");
        println!("{}", base64::encode(cert_der));
        println!("-----END CERTIFICATE-----");
    }

    #[tokio::test]
    async fn test_provision() {
        let root_seed = RootSeed::from_u64(0x42);
        let user_pk = root_seed.derive_user_pk();

        let args = ProvisionArgs {
            user_pk,
            // we're not going through a proxy and can't change DNS resolution
            // here (yet), so just bind cert to "localhost".
            node_dns_name: "localhost".to_owned(),
            ..ProvisionArgs::default()
        };

        let runner_api = Arc::new(MockRunnerClient::new());
        let backend_api = Arc::new(MockBackendClient::new());
        let mut notifs_rx = runner_api.notifs_rx();

        let provision_task = async {
            let mut rng = WeakRng::new();
            provision_node(&mut rng, args, runner_api, backend_api)
                .await
                .unwrap();
        };

        let test_task = async {
            // runner recv ready notification w/ listening port
            let req = notifs_rx.recv().await.unwrap();
            assert_eq!(req.user_pk, user_pk);
            let provision_ports = req.unwrap_provision();
            let port = provision_ports.app_port;

            let expect_dummy_quote = cfg!(not(target_env = "sgx"));

            let mut tls_config = rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(Arc::new(
                    attest::ServerCertVerifier {
                        expect_dummy_quote,
                        enclave_policy: EnclavePolicy::trust_self(),
                    },
                ))
                .with_no_client_auth();
            tls_config.alpn_protocols = vec!["h2".into(), "http/1.1".into()];

            // client sends provision request to node
            let gdrive_credentials = GDriveCredentials::dummy();
            let network = Network::REGTEST;
            let deploy_env = DeployEnv::Dev;
            let provision_req = NodeProvisionRequest {
                root_seed,
                deploy_env,
                network,
                gdrive_credentials,
            };
            let client = reqwest::Client::builder()
                .use_preconfigured_tls(tls_config)
                .build()
                .unwrap();
            let resp = client
                .post(format!("https://localhost:{port}/app/provision"))
                .json(&provision_req)
                .send()
                .await
                .unwrap();
            if !resp.status().is_success() {
                let err: common::api::error::ErrorResponse =
                    resp.json().await.unwrap();
                panic!("Failed to provision: {err:#?}");
            }
        };

        let (_, _) = tokio::join!(provision_task, test_task);

        // test that we can unseal the provisioned data

        // TODO(phlip9): add mock db
        // let node = api.get_node(user_pk).await.unwrap().unwrap();
        // assert_eq!(node.user_pk, user_pk);
    }
}
