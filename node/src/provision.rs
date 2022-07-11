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

use anyhow::{Context, Result};
use async_trait::async_trait;
use common::hex;
use http::{Response, StatusCode};
use rcgen::date_time_ymd;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_rustls::rustls;
use tracing::{debug, info, instrument, warn};
use warp::hyper::Body;
use warp::reject::Reject;
use warp::{Filter, Rejection, Reply};

use crate::api::{self, UserPort};
use crate::attest;
use crate::cli::ProvisionCommand;
use crate::types::{Port, RootSeed, UserId};

const RUNNER_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
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
    // 5. derive node cert? node sends CSR to client?
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

#[derive(Clone)]
pub struct LexeRunner {
    client: reqwest::Client,
}

impl LexeRunner {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(RUNNER_REQUEST_TIMEOUT)
            .build()
            .expect("Failed to build reqwest Client");
        Self { client }
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
        api::notify_runner(&self.client, req).await.map(|_| ())
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
    runner: R,
) -> Result<()> {
    debug!(args.user_id, args.port, %args.node_dns_name, "provisioning");

    // TODO(phlip9): zeroize secrets

    // Generate a fresh key pair, which we'll use for the provisioning cert.
    let cert_key_pair = attest::gen_ed25519_key_pair()
        .context("Failed to generate ed25519 cert key pair")?;

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
    let attestation = attest::quote_enclave(&cert_key_pair)
        .context("Failed to get node enclave quoted")?;

    // Generate a self-signed x509 cert with the remote attestation embedded.
    let cert_params = attest::CertificateParams {
        key_pair: cert_key_pair,
        dns_names: vec![args.node_dns_name],
        // TODO(phlip9): choose a very narrow validity range, like ~1 hour
        not_before: date_time_ymd(1975, 1, 1),
        not_after: date_time_ymd(4096, 1, 1),
        attestation,
    };
    let cert = cert_params
        .gen_cert()
        .context("Failed to generate remote attestation cert")?;
    let cert_der = cert.serialize_der().expect("Failed to DER serialize cert");
    let cert_key_der = cert.serialize_private_key_der();

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
    use std::time::SystemTime;

    use asn1_rs::FromDer;
    use secrecy::Secret;
    use tokio::sync::mpsc;
    use tokio_rustls::rustls::client::{
        ServerCertVerified, ServerCertVerifier,
    };
    use tokio_rustls::rustls::{Certificate, ServerName};
    use tracing::trace;
    use x509_parser::certificate::X509Certificate;

    use super::*;
    use crate::attest::SgxAttestationExtension;
    use crate::{cli, ed25519, logger};

    struct AttestCertVerifier {
        is_debug: bool,
    }

    impl AttestCertVerifier {
        fn real() -> Self {
            Self { is_debug: false }
        }

        fn debug() -> Self {
            Self { is_debug: true }
        }
    }

    impl ServerCertVerifier for AttestCertVerifier {
        fn verify_server_cert(
            &self,
            end_entity: &Certificate,
            intermediates: &[Certificate],
            server_name: &ServerName,
            scts: &mut dyn Iterator<Item = &[u8]>,
            ocsp_response: &[u8],
            now: SystemTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            trace!("client: verify_server_cert");

            // there should be no intermediate certs
            if !intermediates.is_empty() {
                return Err(rustls::Error::General(
                    "received unexpected intermediate certs".to_owned(),
                ));
            }

            // verify the self-signed cert "normally"; ensure everything is
            // well-formed, signatures verify, validity ranges OK, SNI matches,
            // etc...
            let mut trust_roots = rustls::RootCertStore::empty();
            trust_roots.add(end_entity).map_err(|err| {
                rustls::Error::InvalidCertificateData(err.to_string())
            })?;

            let ct_policy = None;
            let webpki_verifier =
                rustls::client::WebPkiVerifier::new(trust_roots, ct_policy);

            let verified_token = webpki_verifier.verify_server_cert(
                end_entity,
                &[],
                server_name,
                scts,
                ocsp_response,
                now,
            )?;

            // in addition to the typical cert checks, we also need to extract
            // the enclave attestation quote from the cert and verify that.

            // TODO(phlip9): parse quote

            let (_, cert) =
                X509Certificate::from_der(&end_entity.0).map_err(|err| {
                    rustls::Error::InvalidCertificateData(err.to_string())
                })?;

            // TODO(phlip9): check binding b/w cert pubkey and Quote report data
            let _cert_pubkey = ed25519::PublicKey::try_from(cert.public_key())?;

            for ext in cert.extensions() {
                debug!(ext_oid = %ext.oid, ext_value = %hex::display(ext.value));
            }

            let sgx_ext_oid = SgxAttestationExtension::oid_asn1_rs();
            let cert_ext = cert
                .get_extension_unique(&sgx_ext_oid)
                .map_err(|err| {
                    rustls::Error::InvalidCertificateData(err.to_string())
                })?
                .ok_or_else(|| {
                    rustls::Error::InvalidCertificateData(
                        "no SGX attestation extension".to_string(),
                    )
                })?;

            let attest =
                SgxAttestationExtension::from_der_bytes(cert_ext.value)
                    .map_err(|err| {
                        rustls::Error::InvalidCertificateData(format!(
                            "invalid SGX attestation: {err}"
                        ))
                    })?;

            if !self.is_debug {
                // 4. (if not dev mode) parse out quote and quote report
                // 5. (if not dev mode) ensure report contains the pubkey hash
                // 6. (if not dev mode) verify quote and quote report
                todo!()
            } else if attest != SgxAttestationExtension::dummy() {
                return Err(rustls::Error::InvalidCertificateData(
                    "invalid SGX attestation".to_string(),
                ));
            }

            Ok(verified_token)
        }
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
        };

        let (runner_req_tx, mut runner_req_rx) = mpsc::channel(1);
        let runner = MockRunner(runner_req_tx);
        let provision_task = provision(args, runner);

        let test_task = async {
            // runner recv ready notification w/ listening port
            let runner_req = runner_req_rx.recv().await.unwrap();
            assert_eq!(runner_req.user_id, user_id);
            let port = runner_req.port;

            let mut tls_config = rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(Arc::new(
                    AttestCertVerifier::debug(),
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
