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
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use async_trait::async_trait;
use http::{Response, StatusCode};
use rcgen::date_time_ymd;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_rustls::rustls;
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
    println!("provision: received provision request");

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
pub async fn provision<R: Runner>(
    args: ProvisionCommand,
    runner: R,
) -> Result<()> {
    // Q: we could wait to init cert + TLS until we've gotten a TCP connection?

    // TODO(phlip9): zeroize secrets

    // Generate a fresh key pair, which we'll use for the provisioning cert.
    let cert_key_pair = attest::gen_ed25519_key_pair()
        .context("Failed to generate ed25519 cert key pair")?;

    // Get our enclave measurement and cert pubkey quoted by the enclave
    // platform. This process binds the cert pubkey to the quote evidence. When
    // a client verifies the Quote, they can also trust that the cert was
    // generated on a valid, genuine enclave. Once this trust is settled,
    // they can safely provision secrets onto the enclave via the newly
    // established secure TLS channel.
    //
    // Returns the quote as an x509 cert extension that we'll embed in our
    // self-signed provisioning cert.
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

    let tls_config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::Certificate(cert_der)],
            rustls::PrivateKey(cert_key_der),
        )
        .context("Failed to build TLS config")?;

    // we'll trigger this when we've completed the provisioning process
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

    // Bind TCP listener on port (queues up any inbound connections).
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let routes = provision_routes(shutdown_tx);
    let (listen_addr, service) = warp::serve(routes)
        .tls()
        .preconfigured_tls(tls_config)
        .bind_ephemeral(addr);
    let port = listen_addr.port();

    println!("provision: listening on port {port}");

    // don't wait forever for the client
    let timeout = tokio::time::sleep(PROVISION_TIMEOUT);

    // notify the runner that we're ready for a client connection
    runner
        .ready(args.user_id, port)
        .await
        .context("Failed to notify runner of our readiness")?;

    println!("provision: notified runner; awaiting client connection");

    tokio::select! {
        // done provisioning
        _ = shutdown_rx.recv() => {
            Ok(())
        }
        _ = timeout => {
            Err(format_err!("Timeout: Client failed to provision in time"))
        }
        _ = service => {
            Err(format_err!("warp provisioning service future should never resolve"))
        }
    }
}

#[cfg(test)]
mod test {
    use secrecy::Secret;

    use super::*;
    use crate::cli;

    #[tokio::test(start_paused = true)]
    #[ignore]
    async fn test_provision() {
        struct MockRunner(mpsc::Sender<UserPort>);

        #[async_trait]
        impl Runner for MockRunner {
            async fn ready(
                &self,
                user_id: UserId,
                port: Port,
            ) -> Result<(), api::ApiError> {
                let req = UserPort { user_id, port };
                self.0.send(req).await.unwrap();
                Ok(())
            }
        }

        let user_id: UserId = 123;
        let node_dns_name = "node.example.com";

        let args = cli::ProvisionCommand {
            user_id,
            node_dns_name: node_dns_name.to_owned(),
            port: 0,
        };

        let (runner_req_tx, mut runner_req_rx) = mpsc::channel(1);
        let runner = MockRunner(runner_req_tx);
        let provision_task = provision(args, runner);

        let test_task = async {
            let runner_req = runner_req_rx.recv().await.unwrap();
            assert_eq!(runner_req.user_id, user_id);
            let port = runner_req.port;

            let provision_req = ProvisionRequest {
                root_seed: RootSeed::new(Secret::new([0x42; 32])),
            };

            // TODO(phlip9): client TLS config
            let client = reqwest::Client::builder().build().unwrap();
            client
                .post(format!("https://127.0.0.1:{port}/provision"))
                .json(&provision_req)
                .send()
                .await
                .unwrap();
        };

        let (_, _) = tokio::join!(provision_task, test_task);
    }
}
