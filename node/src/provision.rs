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
//! into a self-signed TLS certificate, which users must verify when connecting
//! to the provisioning endpoint.

#![allow(dead_code)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use common::attest::cert::AttestationCert;
use common::rng::Crng;
use common::root_seed::RootSeed;
use common::{ed25519, hex};
use http::{Response, StatusCode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_rustls::rustls;
use tracing::{debug, info, instrument, warn};
use warp::hyper::Body;
use warp::reject::Reject;
use warp::{Filter, Rejection, Reply};

use crate::api::{self, LexeApiClient, UserPort};
use crate::attest;
use crate::cli::ProvisionCommand;
use crate::types::{ApiClientType, Port, UserId};

const PROVISION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Error, Debug)]
#[error("todo")]
struct ApiError;

impl Reply for ApiError {
    fn into_response(self) -> Response<Body> {
        // TODO(phlip9): fill out
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(self.to_string().into())
            .expect("Could not construct Response")
    }
}

impl Reject for ApiError {}

fn with_shutdown_tx(
    shutdown_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = (mpsc::Sender<()>,), Error = Infallible> + Clone {
    warp::any().map(move || shutdown_tx.clone())
}

#[derive(Serialize, Deserialize)]
struct ProvisionRequest {
    root_seed: RootSeed,
}

// # provision service
//
// POST /provision
//
// {
//   root_seed: "87089d313793a902a25b0126439ab1ac"
// }
async fn provision_request(
    shutdown_tx: mpsc::Sender<()>,
    _req: ProvisionRequest,
) -> Result<impl Reply, ApiError> {
    debug!("received provision request");

    // 3. read root secret
    // 4. seal root secret w/ platform key
    // 6. push sealed root secret and extras to persistent storage
    // 7. return success & exit node

    // Provisioning done. Stop node.
    let _ = shutdown_tx.try_send(());

    Ok("hello, world")
}

fn provision_routes(
    shutdown_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    // TODO(phlip9): need to decide how client connects to node after
    // provisioning...

    // POST /provision
    warp::path::path("provision")
        .and(warp::post())
        .and(with_shutdown_tx(shutdown_tx))
        .and(warp::body::json())
        .then(provision_request)
}

#[async_trait]
pub trait Runner {
    async fn ready(
        &self,
        user_id: UserId,
        port: Port,
    ) -> Result<(), api::ApiError>;
}

pub struct LexeRunner {
    api: ApiClientType,
}

impl LexeRunner {
    pub fn new(backend_url: String, runner_url: String) -> Self {
        let api = Arc::new(LexeApiClient::new(backend_url, runner_url));
        Self { api }
    }
}

#[async_trait]
impl Runner for LexeRunner {
    async fn ready(
        &self,
        user_id: UserId,
        port: Port,
    ) -> Result<(), api::ApiError> {
        let req = UserPort { user_id, port };
        self.api.notify_runner(req).await.map(|_| ())
    }
}

/// Provision a new lexe node
///
/// Both `userid` and `auth_token` are given by the orchestrator so we know
/// which user we should provision to and have a simple method to authenticate
/// their connection.
#[instrument(skip_all)]
pub async fn provision<R: Runner>(
    args: ProvisionCommand,
    rng: &mut dyn Crng,
    runner: R,
) -> Result<()> {
    debug!(args.user_id, args.port, %args.node_dns_name, "provisioning");

    // TODO(phlip9): zeroize secrets

    // Generate a fresh key pair, which we'll use for the provisioning cert.
    let cert_key_pair = ed25519::gen_key_pair(rng);
    let cert_pubkey = cert_key_pair.public_key_raw();
    debug!(cert_pubkey = %hex::display(cert_pubkey), "attesting to pubkey");

    // Get our enclave measurement and cert pubkey quoted by the enclave
    // platform. This process binds the cert pubkey to the quote evidence. When
    // a client verifies the Quote, they can also trust that the cert was
    // generated on a valid, genuine enclave. Once this trust is settled,
    // they can safely provision secrets onto the enclave via the newly
    // established secure TLS channel.
    //
    // Returns the quote as an x509 cert extension that we'll embed in our
    // self-signed provisioning cert.
    let attest_start = Instant::now();
    let cert_pubkey = ed25519::PublicKey::try_from(&cert_key_pair).unwrap();
    let attestation = attest::quote_enclave(rng, &cert_pubkey)
        .context("Failed to get node enclave quoted")?;

    // Generate a self-signed x509 cert with the remote attestation embedded.
    let dns_names = vec![args.node_dns_name];
    let cert = AttestationCert::new(cert_key_pair, dns_names, attestation)
        .context("Failed to generate remote attestation cert")?;
    let cert_der = cert
        .serialize_der_signed()
        .expect("Failed to sign and serialize attestation cert");
    let cert_key_der = cert.serialize_key_der();

    debug!(
        "acquired attestation: cert size: {} B, time elapsed: {:?}",
        cert_der.len(),
        attest_start.elapsed()
    );

    let tls_config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::Certificate(cert_der)],
            rustls::PrivateKey(cert_key_der),
        )
        .context("Failed to build TLS config")?;

    // we'll trigger `shutdown_tx` when we've completed the provisioning process
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    let shutdown = async move {
        // don't wait forever for the client
        match tokio::time::timeout(PROVISION_TIMEOUT, shutdown_rx.recv()).await
        {
            Ok(_) => {
                debug!("received shutdown; done provisioning");
            }
            Err(_) => {
                warn!(
                    "timeout waiting for successful client provision request"
                );
            }
        }
    };

    // bind TCP listener on port (queues up any inbound connections).
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let routes = provision_routes(shutdown_tx);
    let (listen_addr, service) = warp::serve(routes)
        .tls()
        .preconfigured_tls(tls_config)
        .bind_with_graceful_shutdown(addr, shutdown);
    let port = listen_addr.port();

    info!(%listen_addr, "listening for connections");

    // notify the runner that we're ready for a client connection
    runner
        .ready(args.user_id, port)
        .await
        .context("Failed to notify runner of our readiness")?;

    debug!("notified runner; awaiting client request");
    service.await;

    Ok(())
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use common::attest;
    use common::attest::verify::EnclavePolicy;
    use common::rng::SysRng;
    use secrecy::Secret;
    use tokio::sync::mpsc;
    use tracing::trace;

    use super::*;
    use crate::cli::{self, DEFAULT_BACKEND_URL, DEFAULT_RUNNER_URL};
    use crate::lexe::logger;

    #[cfg(target_env = "sgx")]
    #[test]
    #[ignore] // << uncomment to dump fresh attestation cert
    fn dump_attest_cert() {
        let mut rng = SysRng::new();
        let cert_key_pair = ed25519::from_seed(&[0x42; 32]);
        let cert_pubkey = ed25519::PublicKey::try_from(&cert_key_pair).unwrap();
        let attestation =
            crate::attest::quote_enclave(&mut rng, &cert_pubkey).unwrap();
        let dns_names = vec!["localhost".to_string()];

        let attest_cert =
            AttestationCert::new(cert_key_pair, dns_names, attestation)
                .unwrap();

        let self_report = sgx_isa::Report::for_self();
        println!("MRENCLAVE: '{}'", hex::display(&self_report.mrenclave));
        println!("cert_pubkey: '{cert_pubkey}'");

        let cert_der = attest_cert.serialize_der_signed().unwrap();

        println!("attestation certificate:");
        println!("-----BEGIN CERTIFICATE-----");
        println!("{}", base64::encode(&cert_der));
        println!("-----END CERTIFICATE-----");
    }

    #[tokio::test]
    async fn test_provision() {
        logger::init_for_testing();

        struct MockRunner(mpsc::Sender<UserPort>);

        #[async_trait]
        impl Runner for MockRunner {
            async fn ready(
                &self,
                user_id: UserId,
                port: Port,
            ) -> Result<(), api::ApiError> {
                let req = UserPort { user_id, port };
                trace!("mock runner: received ready notification");
                self.0.send(req).await.unwrap();
                Ok(())
            }
        }

        let user_id: UserId = 123;
        let node_dns_name = "localhost";

        let args = cli::ProvisionCommand {
            user_id,
            node_dns_name: node_dns_name.to_owned(),
            port: 0,
            backend_url: DEFAULT_BACKEND_URL.into(),
            runner_url: DEFAULT_RUNNER_URL.into(),
        };

        let (runner_req_tx, mut runner_req_rx) = mpsc::channel(1);
        let mut rng = SysRng::new();
        let runner = MockRunner(runner_req_tx);
        let provision_task = provision(args, &mut rng, runner);

        let test_task = async {
            // runner recv ready notification w/ listening port
            let runner_req = runner_req_rx.recv().await.unwrap();
            assert_eq!(runner_req.user_id, user_id);
            let port = runner_req.port;

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
            let provision_req = ProvisionRequest {
                root_seed: RootSeed::new(Secret::new([0x42; 32])),
            };
            let client = reqwest::Client::builder()
                .use_preconfigured_tls(tls_config)
                .build()
                .unwrap();
            let resp = client
                .post(format!("https://localhost:{port}/provision"))
                .json(&provision_req)
                .send()
                .await
                .unwrap();
            assert!(resp.status().is_success());
        };

        let (_, _) = tokio::join!(provision_task, test_task);
    }
}
