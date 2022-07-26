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
use std::time::{Duration, Instant};

use anyhow::{ensure, Context, Result};
use bitcoin::secp256k1::PublicKey;
use common::attest::cert::AttestationCert;
use common::enclave::{self, Sealed};
use common::rng::{Crng, SysRng};
use common::root_seed::RootSeed;
use common::{ed25519, hex};
use http::{Response, StatusCode};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_rustls::rustls;
use tracing::{debug, info, instrument, warn};
use warp::hyper::Body;
use warp::reject::Reject;
use warp::{Filter, Rejection, Reply};

use crate::api::{Enclave, Instance, Node, NodeInstanceEnclave, UserPort};
use crate::attest;
use crate::cli::ProvisionCommand;
use crate::convert::{get_enclave_id, get_instance_id};
use crate::lexe::keys_manager::LexeKeysManager;
use crate::types::{ApiClientType, UserId};

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

fn with_api_client(
    api: ApiClientType,
) -> impl Filter<Extract = (ApiClientType,), Error = Infallible> + Clone {
    warp::any().map(move || api.clone())
}

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
struct ProvisionRequest {
    /// The client's user id.
    user_id: UserId,
    /// The client's node public key, derived from the root seed. The node
    /// should sanity check by re-deriving the node pubkey and checking that it
    /// equals the client's expected value.
    node_pubkey: PublicKey,
    /// The secret root seed the client wants to provision into the node.
    root_seed: RootSeed,
}

impl ProvisionRequest {
    fn verify<R: Crng>(
        self,
        rng: &mut R,
        expected_user_id: &UserId,
    ) -> Result<(UserId, PublicKey, ProvisionedSecrets)> {
        ensure!(&self.user_id == expected_user_id);

        // TODO(phlip9): derive just the node pubkey without all the extra junk
        // that gets derived constructing a whole KeysManager
        let _keys_manager =
            LexeKeysManager::init(rng, &self.node_pubkey, &self.root_seed)?;
        Ok((
            self.user_id,
            self.node_pubkey,
            ProvisionedSecrets {
                root_seed: self.root_seed,
            },
        ))
    }
}

/// The enclave's provisioned secrets that it will seal and persist using its
/// platform enclave keys that are software and version specific.
///
/// See: [`common::enclave::seal`]
struct ProvisionedSecrets {
    pub root_seed: RootSeed,
}

impl ProvisionedSecrets {
    const LABEL: &'static [u8] = b"provisioned secrets";

    fn seal(&self, rng: &mut dyn Crng) -> Result<Sealed<'_>> {
        let root_seed_ref = self.root_seed.expose_secret().as_slice();
        enclave::seal(rng, Self::LABEL, root_seed_ref.into())
            .context("Failed to seal provisioned secrets")
    }

    fn unseal(sealed: Sealed<'_>) -> Result<Self> {
        let bytes = enclave::unseal(Self::LABEL, sealed)
            .context("Failed to unseal provisioned secrets")?;
        let root_seed = RootSeed::try_from(bytes.as_slice())
            .context("Failed to deserialize root seed")?;
        Ok(Self { root_seed })
    }
}

// # provision service
//
// POST /provision
//
// ```json
// {
//   "user_id": 123,
//   "node_pubkey": "031355a4419a2b31c9b1ba2de0bcbefdd4a2ef6360f2b018736162a9b3be329fd4".parse().unwrap(),
//   "root_seed": "86e4478f9f7e810d883f22ea2f0173e193904b488a62bb63764c82ba22b60ca7".parse().unwrap(),
// }
// ```
async fn provision_request(
    api: ApiClientType,
    shutdown_tx: mpsc::Sender<()>,
    req: ProvisionRequest,
) -> Result<impl Reply, ApiError> {
    debug!("received provision request");

    let user_id: UserId = 123;

    // TODO(phlip9): inject rng?
    let mut rng = SysRng::new();

    let (user_id, node_public_key, provisioned_secrets) =
        req.verify(&mut rng, &user_id).map_err(|_| ApiError)?;

    let sealed_secrets =
        provisioned_secrets.seal(&mut rng).map_err(|_| ApiError)?;

    // TODO(phlip9): add some constructors / ID newtypes
    let measurement = enclave::measurement();
    let node = Node {
        public_key: node_public_key,
        user_id,
    };
    let instance_id = get_instance_id(&node_public_key, &measurement);
    let instance = Instance {
        id: instance_id.clone(),
        node_public_key,
        measurement,
    };
    let machine_id = enclave::machine_id();
    let machine_id_str = format!("{}", machine_id);
    let enclave = Enclave {
        id: get_enclave_id(&instance_id, &machine_id_str),
        seed: sealed_secrets.serialize(),
        instance_id,
    };

    let batch = NodeInstanceEnclave {
        node,
        instance,
        enclave,
    };

    // TODO(phlip9): auth using user id derived from root seed

    api.create_node_instance_enclave(batch)
        .await
        .map_err(|_| ApiError)?;

    // Provisioning done. Stop node.
    let _ = shutdown_tx.try_send(());

    Ok("OK")
}

fn provision_routes(
    api: ApiClientType,
    shutdown_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    // POST /provision
    warp::path::path("provision")
        .and(warp::post())
        .and(with_api_client(api))
        .and(with_shutdown_tx(shutdown_tx))
        .and(warp::body::json())
        .then(provision_request)
}

/// Provision a new lexe node
///
/// Both `userid` and `auth_token` are given by the orchestrator so we know
/// which user we should provision to and have a simple method to authenticate
/// their connection.
#[instrument(skip_all)]
pub async fn provision(
    args: ProvisionCommand,
    api: ApiClientType,
    rng: &mut dyn Crng,
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
    let routes = provision_routes(api.clone(), shutdown_tx);
    let (listen_addr, service) = warp::serve(routes)
        .tls()
        .preconfigured_tls(tls_config)
        .bind_with_graceful_shutdown(addr, shutdown);
    let port = listen_addr.port();

    info!(%listen_addr, "listening for connections");

    // notify the runner that we're ready for a client connection
    let user_id = args.user_id;
    api.notify_runner(UserPort { user_id, port })
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

    use super::*;
    use crate::api::mock::MockApiClient;
    use crate::cli::{self, DEFAULT_BACKEND_URL, DEFAULT_RUNNER_URL};
    use crate::lexe::logger;
    use crate::types::UserId;

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

        println!("measurement: '{}'", enclave::measurement());
        println!("cert_pubkey: '{cert_pubkey}'");

        let cert_der = attest_cert.serialize_der_signed().unwrap();

        println!("attestation certificate:");
        println!("-----BEGIN CERTIFICATE-----");
        println!("{}", base64::encode(&cert_der));
        println!("-----END CERTIFICATE-----");
    }

    #[test]
    fn test_provision_request_serde() {
        let req = ProvisionRequest {
            user_id: 123,
            node_pubkey: "031355a4419a2b31c9b1ba2de0bcbefdd4a2ef6360f2b018736162a9b3be329fd4".parse().unwrap(),         root_seed:
        "86e4478f9f7e810d883f22ea2f0173e193904b488a62bb63764c82ba22b60ca7".parse().unwrap(),
        };
        let actual = serde_json::to_value(&req).unwrap();
        let expected = serde_json::json!({
            "user_id": 123,
            "node_pubkey": "031355a4419a2b31c9b1ba2de0bcbefdd4a2ef6360f2b018736162a9b3be329fd4",
            "root_seed": "86e4478f9f7e810d883f22ea2f0173e193904b488a62bb63764c82ba22b60ca7",
        });
        assert_eq!(&actual, &expected);
    }

    #[tokio::test]
    async fn test_provision() {
        logger::init_for_testing();

        let user_id: UserId = 123;
        let node_dns_name = "localhost";

        let args = cli::ProvisionCommand {
            machine_id: enclave::machine_id(),
            user_id,
            node_dns_name: node_dns_name.to_owned(),
            port: 0,
            backend_url: DEFAULT_BACKEND_URL.into(),
            runner_url: DEFAULT_RUNNER_URL.into(),
        };

        let mut rng = SysRng::new();
        let api = Arc::new(MockApiClient::new());
        let mut notifs_rx = api.notifs_rx();

        let provision_task = async {
            provision(args, api, &mut rng).await.unwrap();
        };

        let test_task = async {
            // runner recv ready notification w/ listening port
            let req = notifs_rx.recv().await.unwrap();
            assert_eq!(req.user_id, user_id);
            let port = req.port;

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
                user_id,
                node_pubkey: "031f7d233e27e9eaa68b770717c22fddd3bdd58656995d9edc32e84e6611182241".parse().unwrap(),
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

        // test that we can unseal the provisioned data

        // TODO(phlip9): add mock db
        // let node = api.get_node(user_id).await.unwrap().unwrap();
        // assert_eq!(node.user_id, user_id);
    }
}
