//! HTTP delivery for the WALLET and a mailbox client for the REQUESTER.

use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, ensure};
use lexe_common::env::DeployEnv;
use lexe_uri::Uri;
use reqwest::StatusCode;

use crate::{mailbox::Address, seal::Blob, wallet::HttpDelivery};

/// A client for REQUESTER servers and mailboxes.
///
/// - Trusts Mozilla's webpki roots.
/// - Follows no redirects, so a redirecting server can't re-route a plaintext
///   response.
/// - Requires `https://` in staging and prod.
//
// Spec: `post_url` delivery.
#[derive(Clone)]
pub struct LexeConnectClient(reqwest::Client);

impl LexeConnectClient {
    const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

    pub fn new(deploy_env: DeployEnv) -> anyhow::Result<Self> {
        // Use the default ring CryptoProvider with webpki roots for broad
        // compatibility with REQUESTER servers.
        #[allow(clippy::disallowed_methods)]
        let tls_config = rustls::ClientConfig::builder_with_protocol_versions(
            lexe_tls_core::LEXE_TLS_PROTOCOL_VERSIONS,
        )
        .with_root_certificates(lexe_tls_core::WEBPKI_ROOT_CERTS.clone())
        .with_no_client_auth();

        let client = reqwest::Client::builder()
            .https_only(deploy_env.is_staging_or_prod())
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Self::HTTP_TIMEOUT)
            .use_preconfigured_tls(tls_config)
            .build()
            .context("Failed to build LexeConnect reqwest client")?;

        Ok(Self(client))
    }

    /// Performs an [`HttpDelivery`]. Any non-2xx status is an error,
    /// including a mailbox's `409 Conflict` for an address holding a
    /// different blob.
    pub async fn deliver(&self, delivery: &HttpDelivery) -> anyhow::Result<()> {
        let resp = self
            .0
            .post(&delivery.url)
            .header(reqwest::header::CONTENT_TYPE, delivery.content_type)
            .body(delivery.body.clone())
            .send()
            .await
            .context("Failed to deliver response")?;

        let status = resp.status();
        ensure!(status.is_success(), "Delivery failed: HTTP {status}");

        Ok(())
    }

    /// Fetches the blob at `address`, or `None` if it hasn't arrived.
    pub async fn mailbox_get(
        &self,
        mailbox_url: &str,
        address: &Address,
    ) -> anyhow::Result<Option<Blob>> {
        let mut url = Uri::parse(mailbox_url).context("Invalid mailbox url")?;
        url.push_param("address", address.to_string());

        let resp = self
            .0
            .get(url.to_string())
            .send()
            .await
            .context("Failed to query mailbox")?;

        match resp.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                let bytes =
                    resp.bytes().await.context("Failed to read blob")?;
                Ok(Some(Blob(bytes.to_vec())))
            }
            status => Err(anyhow!("Mailbox query failed: HTTP {status}")),
        }
    }

    /// Polls `address` every `interval` until a blob arrives or `timeout`
    /// elapses.
    pub async fn mailbox_poll(
        &self,
        mailbox_url: &str,
        address: &Address,
        interval: Duration,
        timeout: Duration,
    ) -> anyhow::Result<Blob> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(blob) = self.mailbox_get(mailbox_url, address).await? {
                return Ok(blob);
            }

            ensure!(
                Instant::now() + interval <= deadline,
                "Timed out waiting for the mailbox"
            );
            tokio::time::sleep(interval).await;
        }
    }
}
