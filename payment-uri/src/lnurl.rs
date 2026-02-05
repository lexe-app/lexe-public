//! Resolver for LNURL-Pay and Lightning Address (LUD-06, LUD-16).
//!
//! This module implements the LNURL-pay flow for HTTP URLs derived from
//! Lightning Addresses, URI-encoded LNURLs, or bech32-encoded LNURLs.
//!
//! ## LNURL-pay Protocol Flow (LUD-06)
//!
//! 1. **Decode LNURL to HTTP URL**:
//!    - Bech32: `lnurl1dp68gurn8ghj7...` → `https://service.com/api/lnurl/abc123`
//!    - LUD-17: `lnurlp://service.com/path` → `https://service.com/path`
//!
//! 2. **GET initial endpoint** → Receive:
//! ```json
//! {
//!   "callback": "https://service.com/api/lnurl/abc123/callback",
//!   "minSendable": 1000,        // millisatoshis
//!   "maxSendable": 1000000000,   // millisatoshis
//!   "metadata": "[[\"text/plain\",\"Payment for coffee\"]]",
//!   "tag": "payRequest"
//! }
//! ```
//!
//! 3. **User selects amount** (in millisatoshis), then **GET callback** with
//!    amount: `GET https://service.com/api/lnurl/abc123/callback?amount=50000`
//!
//! 4. **Receive Lightning invoice**:
//!
//! ```json
//! {
//!   "pr": "lnbc500n1...",  // BOLT11 invoice
//!   "routes": []           // Always empty (deprecated field)
//! }
//! ```
//!
//! 5. **Verify invoice**:
//!    - Amount matches requested amount
//!    - `description_hash` = SHA256(metadata)
//!
//! 6. **Pay the invoice**
//!
//! The `metadata` field contains info which may be displayed to the user.
//! Its hash becomes the invoice's `description_hash` for cryptographic proof
//! of what you're paying for.
//!
//! ## Lightning Address Flow (LUD-16)
//!
//! Lightning Address provides human-readable addresses like `alice@example.com`
//! that resolve to LNURL-pay endpoints.
//!
//! 1. **Parse address**: `alice@example.com` → Extract username and domain
//!
//! 2. **Build URL**: `https://example.com/.well-known/lnurlp/alice`
//!
//! 3. **GET this URL** → Receive standard LNURL-pay response:
//! ```json
//! {
//!   "callback": "https://example.com/api/lnurl/alice/callback",
//!   "minSendable": 1000,
//!   "maxSendable": 1000000000,
//!   "metadata": "[[\"text/plain\",\"Pay to alice@example.com\"],\
//!                  [\"text/identifier\",\"alice@example.com\"]]",
//!   "tag": "payRequest"
//! }
//! ```
//!
//! 4. **Continue with LNURL-pay flow** (steps 3-6 from above)
//!
//! There are some differences from regular LNURL-Pay:
//! - No bech32 encoding - address stays human-readable
//! - URL constructed from `username@domain` pattern
//! - Metadata MUST include `text/identifier` or `text/email` entry
//! - Supports optional `+tag` suffix for multiple payment endpoints
//!
//! Example with tag: `alice+tips@example.com`
//!   => `https://example.com/.well-known/lnurlp/alice+tips`

use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow, ensure};
use common::{constants, env::DeployEnv, ln::amount::Amount};
use lexe_api_core::types::{
    invoice::LxInvoice,
    lnurl::{
        LnurlCallbackResponse, LnurlErrorWire, LnurlPayRequest,
        LnurlPayRequestMetadata, LnurlPayRequestWire,
    },
};
use lexe_tls_core::rustls::{self, RootCertStore, pki_types::CertificateDer};
use serde::Deserialize;
use tracing::debug;

/// Timeout for LNURL HTTP requests.
// TODO(max): const_assert! that this timeout is shorter than the timeout on the
// API handler for a sidecar `pay_lnurl_pay_request` endpoint?
pub(crate) const LNURL_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// A client for LNURL-pay and Lightning Address requests.
/// Trusts Mozilla's webpki roots.
#[derive(Clone)]
pub struct LnurlClient(reqwest::Client);

impl LnurlClient {
    pub fn new(deploy_env: DeployEnv) -> anyhow::Result<Self> {
        let ca_certs = if deploy_env.is_staging_or_prod() {
            lexe_tls_core::WEBPKI_ROOT_CERTS.clone()
        } else {
            let mut root_store = RootCertStore::empty();
            root_store
                .add(CertificateDer::from_slice(
                    constants::LEXE_DUMMY_CA_CERT_DER,
                ))
                .context("Failed to add dummy Lexe CA cert")?;
            Arc::new(root_store)
        };

        // Use the default ring CryptoProvider with webpki roots for broad
        // compatibility with Lightning Address servers
        #[allow(clippy::disallowed_methods)]
        let tls_config = rustls::ClientConfig::builder_with_protocol_versions(
            lexe_tls_core::LEXE_TLS_PROTOCOL_VERSIONS,
        )
        .with_root_certificates(ca_certs)
        .with_no_client_auth();

        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(LNURL_HTTP_TIMEOUT)
            .use_preconfigured_tls(tls_config)
            .build()
            .context("Failed to build LNURL reqwest client")?;

        Ok(Self(client))
    }

    /// Fetches a [`LnurlPayRequest`] from an LNURL-pay HTTP URL.
    pub async fn get_pay_request(
        &self,
        http_url: &str,
    ) -> anyhow::Result<LnurlPayRequest> {
        debug!("Fetching LNURL-pay response from: {http_url}");

        /// The raw LNURL-pay response prior to parsing and validation.
        ///
        /// LNURL doesn't use a consistent tagging scheme, so we need to use
        /// a serde untagged enum, which will just try each variant in order.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawResponse {
            PayRequest(LnurlPayRequestWire),
            Error(LnurlErrorWire),
        }

        let raw_response = self
            .0
            .get(http_url)
            .send()
            .await
            .context("Failed to fetch LNURL-pay endpoint")?
            .json::<RawResponse>()
            .await
            .context("Failed to parse LNURL-pay response")?;

        let pay_req_wire = match raw_response {
            RawResponse::PayRequest(x) => x,
            RawResponse::Error(LnurlErrorWire { reason, .. }) =>
                return Err(anyhow!("LNURL-pay endpoint: {reason}")),
        };
        let LnurlPayRequestWire {
            callback,
            min_sendable_msat,
            max_sendable_msat,
            metadata,
            comment_allowed,
            tag,
        } = pay_req_wire;

        ensure!(
            tag == "payRequest",
            "Expected LNURL-pay endpoint, got '{tag}'"
        );

        let min_sendable = Amount::from_msat(min_sendable_msat);
        let max_sendable = Amount::from_msat(max_sendable_msat);

        ensure!(
            min_sendable > Amount::ZERO,
            "LNURL-pay minSendable must be positive, got {min_sendable}"
        );
        ensure!(
            min_sendable <= max_sendable,
            "LNURL-pay has invalid amount range: \
             min {min_sendable} > max {max_sendable}"
        );

        let metadata = LnurlPayRequestMetadata::from_raw_string(metadata)?;
        debug!(
            %callback, ?comment_allowed, %min_sendable, %max_sendable,
            description = %metadata.description,
            "Fetched LNURL-pay payRequest",
        );

        Ok(LnurlPayRequest {
            callback,
            min_sendable,
            max_sendable,
            metadata,
            comment_allowed,
        })
    }

    /// Resolves a given [`LnurlPayRequest`] and amount into a BOLT11 invoice.
    ///
    /// The amount must be within the min/max range from the pay request.
    /// If `comment` is provided (LUD-12), it is validated against
    /// `comment_allowed` and appended to the callback URL.
    pub async fn resolve_pay_request(
        &self,
        pay_req: &LnurlPayRequest,
        amount: Amount,
        comment: Option<&str>,
    ) -> anyhow::Result<LxInvoice> {
        let callback = &pay_req.callback;
        debug!(%amount, %callback, "Resolving LNURL-pay request");

        let min_sendable = pay_req.min_sendable;
        let max_sendable = pay_req.max_sendable;
        ensure!(
            amount >= min_sendable,
            "Amount {amount} sats below minimum {min_sendable} sats \
             required by LNURL-pay request",
        );
        ensure!(
            amount <= max_sendable,
            "Amount {amount} sats exceeds maximum {max_sendable} sats \
             allowed by LNURL-pay request",
        );

        // LUD-12: validate comment against comment_allowed.
        if let Some(comment) = comment {
            let max_len = pay_req.comment_allowed.ok_or_else(|| {
                anyhow!("Recipient doesn't support comments (LUD-12)")
            })?;
            let char_count = comment.chars().count();
            ensure!(
                char_count <= usize::from(max_len),
                "Comment is {char_count} chars, \
                 exceeds maximum of {max_len} chars",
            );
        }

        // Build callback URL with amount and optional comment.
        let callback_url = {
            let amount_msat = amount.msat();
            let sep = if callback.contains('?') { '&' } else { '?' };
            let mut url = format!("{callback}{sep}amount={amount_msat}");

            if let Some(comment) = comment
                && !comment.is_empty()
            {
                use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
                let encoded = utf8_percent_encode(comment, NON_ALPHANUMERIC);
                url.push_str(&format!("&comment={encoded}"));
            }

            url
        };

        /// The raw LNURL-pay callback response prior to parsing and validation.
        ///
        /// LNURL doesn't use a consistent tagging scheme, so we need to use
        /// a serde untagged enum, which will just try each variant in order.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawResponse {
            Invoice(LnurlCallbackResponse),
            Error(LnurlErrorWire),
        }

        let raw_response = self
            .0
            .get(&callback_url)
            .send()
            .await
            .context("Failed to request invoice from LNURL-pay callback")?
            .json::<RawResponse>()
            .await
            .context("Failed to parse LNURL-pay callback response")?;

        let raw_invoice_resp = match raw_response {
            RawResponse::Invoice(x) => x,
            RawResponse::Error(LnurlErrorWire { reason, .. }) =>
                return Err(anyhow!("LNURL-pay callback: {reason}")),
        };
        let LnurlCallbackResponse {
            pr: invoice,
            routes: _,
        } = raw_invoice_resp;

        // Validate amount
        let invoice_amount = invoice
            .amount()
            .context("LNURL-pay invoice must have an amount")?;
        ensure!(
            invoice_amount == amount,
            "Invoice amount {invoice_amount} doesn't match requested {amount}"
        );

        // Validate description hash
        let description_hash = invoice
            .description_hash()
            .context("LNURL-pay: returned invoice must use description hash")?;
        ensure!(
            description_hash == &pay_req.metadata.description_hash,
            "Invoice description hash mismatch"
        );

        debug!("Resolved LNURL-pay invoice: {invoice}");

        Ok(invoice)
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use common::{
        env::DeployEnv,
        ln::amount::Amount,
        rng::{Rng, ThreadFastRng},
    };
    use tracing::info;

    use super::*;

    /// Live test that resolves D++'s Lightning Address me@dplus.plus.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_lightning_address_dplus -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_lightning_address_dplus() {
        logger::init_for_testing();

        let ln_address = "me@dplus.plus";
        info!("Lightning Address: {ln_address}");

        let payment_uri =
            payment_uri_core::PaymentUri::parse(ln_address).unwrap();

        let email_like = match payment_uri {
            payment_uri_core::PaymentUri::EmailLikeAddress(email_like) =>
                email_like,
            other => panic!("Expected EmailLikeAddress, got: {other:?}"),
        };

        let ln_address_url = email_like.lightning_address_url;
        info!("Lightning Address URL: {ln_address_url}");

        let lnurl_client = LnurlClient::new(DeployEnv::Prod).unwrap();

        let pay_request = tokio::time::timeout(
            Duration::from_secs(10),
            lnurl_client.get_pay_request(&ln_address_url),
        )
        .await
        .unwrap()
        .unwrap();

        info!("Lightning Address successfully resolved into payRequest");
        let callback = &pay_request.callback;
        let min_sendable = pay_request.min_sendable;
        let max_sendable = pay_request.max_sendable;
        let comment_allowed = pay_request.comment_allowed;
        info!("Callback URL: {callback}");
        info!("Min amount: {min_sendable} sats");
        info!("Max amount: {max_sendable} sats");
        info!("Description: {}", pay_request.metadata.description);
        info!("Comment allowed: {comment_allowed:?}");

        // Request invoice with random amount within allowed range
        let amount = {
            let mut rng = ThreadFastRng::new();
            let amount_msat =
                rng.gen_range(min_sendable.msat()..=max_sendable.msat());
            Amount::from_msat(amount_msat)
        };
        // Send a comment if the recipient supports it.
        let comment = comment_allowed.map(|_| "Hello from Lexe! 🚀");
        info!("Requesting invoice for {amount} sats");
        let invoice = tokio::time::timeout(
            Duration::from_secs(10),
            lnurl_client.resolve_pay_request(&pay_request, amount, comment),
        )
        .await
        .unwrap()
        .unwrap();

        info!("Successfully received invoice: {invoice}");
        info!("Invoice network: {:?}", invoice.network());
    }
}
