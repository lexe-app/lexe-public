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

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use rcgen::date_time_ymd;
use tokio::sync::mpsc;
use warp::{Filter, Rejection, Reply};

use crate::api::{self, UserPort};
use crate::attest;
use crate::cli::ProvisionCommand;
use crate::types::{Port, UserId};

const RUNNER_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const PROVISION_TIMEOUT: Duration = Duration::from_secs(10);

async fn notify_runner(user_id: UserId, port: Port) -> Result<()> {
    let client = reqwest::ClientBuilder::new()
        .timeout(RUNNER_REQUEST_TIMEOUT)
        .build()
        .expect("Failed to build reqwest Client");

    let req = UserPort { user_id, port };

    let _ = api::notify_runner(&client, req).await?;
    Ok(())
}

// # provision service (idk)
//
// POST /provision
// BEARER <AUTH-TOKEN>
//
// {
//   shared_secret: "87089d313793a902a25b0126439ab1ac"
// }

fn provision_routes(
    shutdown_tx: mpsc::Sender<()>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::path::end().map(move || {
        let _ = shutdown_tx.try_send(());
        "Hello, world"
    })
}

/// Provision a new lexe node
///
/// Both `userid` and `auth_token` are given by the orchestrator so we know
/// which user we should provision to and have a simple method to authenticate
/// their connection.
pub async fn provision(args: ProvisionCommand) -> Result<()> {
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
        // TODO(phlip9): choose a narrow validity range
        not_before: date_time_ymd(2022, 5, 22),
        not_after: date_time_ymd(2032, 5, 22),
        attestation,
    };
    let cert = cert_params
        .gen_cert()
        .context("Failed to generate remote attestation cert")?;
    let cert_der = cert.serialize_der().expect("Failed to DER serialize cert");
    let cert_key_der = cert.serialize_private_key_der();

    // we'll trigger this when we've completed the provisioning process
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

    // Bind TCP listener on port (queues up any inbound connections).
    // TODO(phlip9): how to best avoid handling more than one connection/request
    // at once?
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let (_listen_addr, service) = warp::serve(provision_routes(shutdown_tx))
        .tls()
        .cert(&cert_der)
        .key(&cert_key_der)
        .bind_ephemeral(addr);

    // notify the runner that we're ready for a client connection
    notify_runner(args.user_id, args.port)
        .await
        .context("Failed to notify runner of our readiness")?;

    // don't wait forever for the client
    let timeout = tokio::time::sleep(PROVISION_TIMEOUT);

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
