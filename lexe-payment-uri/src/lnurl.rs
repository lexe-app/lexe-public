//! This module implements the HTTP-related LNURL protocol steps for [`Lnurl`]s
//! derived from Lightning Addresses (LUD-16), URI-encoded LNURLs (LUD-17),
//! or bech32-encoded LNURLs (LUD-01).
//!
//! Supported LNURL flows include:
//!
//! Payments
//! - LUD-06: LNURL-pay
//! - LUD-12: LNURL-pay comments
//!   - [`get_pay_request`](crate::lnurl::LnurlClient::get_pay_request)
//!   - [`resolve_pay_request`](crate::lnurl::LnurlClient::resolve_pay_request)
//!
//! Withdraws
//! - LUD-03: LNURL-withdraw
//! - LUD-08: Fast LNURL-withdraw
//!   - [`get_withdraw_request`](crate::lnurl::LnurlClient::get_withdraw_request)
//!   - [`fetch_withdraw_request`](crate::lnurl::LnurlClient::fetch_withdraw_request)
//!   - [`resolve_withdraw_request`](crate::lnurl::LnurlClient::resolve_withdraw_request)
//!
//! General resolution of LNURLs
//! - [`get_lnurl_intermediate`](crate::lnurl::LnurlClient::get_lnurl_intermediate)
//! - [`resolve_lnurl`](crate::lnurl::LnurlClient::resolve_lnurl)
//!
//! Some steps in the LNURL flows require only parsing, and are implemented as
//! methods on [`Lnurl`] rather than in this module. These include:
//!   - [`to_fast_withdraw_request`](Lnurl::to_fast_withdraw_request)
//!
//! Terminology:
//! - LNURL ([`Lnurl`]): The decoded URI used as the starting point for LNURL
//!   flows
//! - LNURL flow: A specific LNURL protocol flow (LNURL-pay, LNURL-withdraw,
//!   etc.) which is always associated with an LNURL `tag` (LUD-01):
//! - LNURL intermediate
//!   ([`LnurlIntermediate`](crate::lnurl::LnurlIntermediate)): The data derived
//!   directly from an LNURL, either by parsing the query parameters or by
//!   making a GET request to the LNURL endpoint. We refer to each intermediate
//!   by its associated LNURL tag:
//!   - `LnurlPayRequest` for `payRequest` tag
//!   - `LnurlWithdrawRequest` for `withdrawRequest` tag
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

use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::{Context, anyhow, ensure};
use lexe_api_core::types::{
    invoice::Invoice,
    lnurl::{
        LnurlCallbackResponse, LnurlErrorWire, LnurlPayRequest,
        LnurlPayRequestMetadata, LnurlPayRequestWire,
    },
};
use lexe_common::{constants, env::DeployEnv, ln::amount::Amount};
use lexe_payment_uri_core::{
    ClaimMethod, Lnurl, LnurlScheme, LnurlTag, LnurlWithdrawRequest,
    LnurlWithdrawRequestWire, PaymentMethod,
};
use lexe_std::Apply;
use lexe_tls_core::rustls::{self, RootCertStore, pki_types::CertificateDer};
use serde::Deserialize;
use tracing::debug;

/// Timeout for LNURL HTTP requests.
// TODO(max): const_assert! that this timeout is shorter than the timeout on the
// API handler for a sidecar `pay_lnurl_pay_request` endpoint?
pub(crate) const LNURL_HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Intermediate LNURL data, derived either directly from the LNURL parameters
/// or from making a GET request to the LNURL endpoint.
///
/// Branches are associated with an LNURL `tag` (see LUD-01). Note that LNURL
/// tags and flows may be different ("LNURL-auth" flow vs. `login` tag).
#[derive(Debug)]
pub enum LnurlIntermediate {
    /// tag: "payRequest"
    Pay(LnurlPayRequest),
    /// tag: "withdrawRequest"
    Withdraw(LnurlWithdrawRequest),
}

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

    /// Reads an LNURL and resolves it into an [`LnurlIntermediate`].
    ///
    /// Uses the flow type indicated by the LNURL scheme or tag, if available.
    pub async fn get_lnurl_intermediate(
        &self,
        lnurl: &Lnurl<'_>,
    ) -> anyhow::Result<LnurlIntermediate> {
        use LnurlScheme::*;

        match (lnurl.scheme, lnurl.tag) {
            // LNURL-pay flow
            (Pay, _) | (_, Some(LnurlTag::PayRequest)) =>
                Ok(LnurlIntermediate::Pay(self.get_pay_request(lnurl).await?)),
            // LNURL-withdraw flow
            (Withdraw, _) | (_, Some(LnurlTag::WithdrawRequest)) =>
                Ok(LnurlIntermediate::Withdraw(
                    self.get_withdraw_request(lnurl).await?,
                )),
            // Ambiguous LNURL flows which must be resolved via GET
            (Https, None) | (HttpOnion, None) => {
                let http_url: &str = &lnurl.http_url;
                debug!("Fetching LNURL response from: {http_url}");

                let json = self
                    .0
                    .get(http_url)
                    .send()
                    .await
                    .context("Failed to make request to LNURL endpoint")?
                    .json::<serde_json::Value>()
                    .await
                    .context("Failed to parse LNURL response")?;

                if let Some(json_tag) = json.get("tag") {
                    let tag = json_tag
                        .as_str()
                        .context("LNURL response missing tag")?
                        .apply(LnurlTag::from_str)
                        .context("Unknown LNURL tag in response")?;
                    match tag {
                        LnurlTag::PayRequest => {
                            let pay_req_wire: LnurlPayRequestWire =
                                serde_json::from_value(json).context(
                                    "Failed to parse LNURL-pay response",
                                )?;
                            Ok(LnurlIntermediate::Pay(LnurlPayRequest::from(
                                pay_req_wire,
                            )))
                        }
                        LnurlTag::WithdrawRequest => {
                            let withdraw_req_wire: LnurlWithdrawRequestWire =
                                serde_json::from_value(json).context(
                                    "Failed to parse LNURL-withdraw response",
                                )?;
                            let withdraw_req =
                                LnurlWithdrawRequest::try_from_wire(
                                    withdraw_req_wire,
                                )
                                .context("Invalid LNURL-withdraw response")?;
                            Ok(LnurlIntermediate::Withdraw(withdraw_req))
                        }
                        other => Err(anyhow!(
                            "Unsupported LNURL tag {other} in response"
                        )),
                    }
                } else if let Ok(error) =
                    serde_json::from_value::<LnurlErrorWire>(json)
                {
                    Err(anyhow!(
                        "LNURL endpoint returned an error: {}",
                        error.reason
                    ))
                } else {
                    Err(anyhow!("Unexpected response format"))
                }
            }
            // Unsupported LNURL flows
            (scheme, tag) => {
                let hint = if let Some(t) = tag {
                    format!("tag {t}")
                } else {
                    format!("scheme {scheme:?}")
                };
                Err(anyhow!("Lnurl {hint} not supported"))
            }
        }
    }

    /// Fetches a [`LnurlPayRequest`] from an LNURL-pay HTTP URL.
    ///
    /// Doesn't verify expected LNURL flow type.
    pub async fn get_pay_request(
        &self,
        lnurl: &Lnurl<'_>,
    ) -> anyhow::Result<LnurlPayRequest> {
        let http_url = lnurl.http_url.as_ref();
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
            tag: _,
        } = pay_req_wire;

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
    ) -> anyhow::Result<Invoice> {
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
                return Err(anyhow!("LNURL-pay callback failed: {reason}")),
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

        // Lots of description hashes don't adhere to the LUDS06 spec,
        // so we are just gonna bypass description hash validation.
        // // Validate description hash
        // let description_hash = invoice
        //     .description_hash()
        //     .context(
        //         "LNURL-pay: returned invoice must use description hash"
        //     )?;
        // ensure!(
        //     description_hash == &pay_req.metadata.description_hash,
        //     "Invoice description hash mismatch"
        // );

        debug!("Resolved LNURL-pay invoice: {invoice}");

        Ok(invoice)
    }

    /// Resolve a given LNURL-withdraw HTTP URL into a [`LnurlWithdrawRequest`].
    /// Supports both regular withdraw (LUD-03) and fast withdraw (LUD-08).
    ///
    /// To resolve specifically regular withdraw or fast withdraw, use
    /// `fetch_withdraw_request` (LUD-03) or
    /// `parse_fast_withdraw_request` (LUD-08).
    ///
    /// Doesn't verify expected LNURL flow type.
    pub async fn get_withdraw_request(
        &self,
        lnurl: &Lnurl<'_>,
    ) -> anyhow::Result<LnurlWithdrawRequest> {
        match lnurl.to_fast_withdraw_request() {
            Ok(req) => Ok(req),
            Err(_) => self.fetch_withdraw_request(lnurl).await,
        }
    }

    /// Fetches an [`LnurlWithdrawRequest`] from an LNURL-withdraw HTTP URL.
    ///
    /// Doesn't verify expected LNURL flow type.
    pub async fn fetch_withdraw_request(
        &self,
        lnurl: &Lnurl<'_>,
    ) -> anyhow::Result<LnurlWithdrawRequest> {
        let http_url: &str = &lnurl.http_url;

        /// The raw LNURL-withdraw response. LNURL doesn't use a consistent
        /// response scheme, so an untagged enum is needed.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawResponse {
            WithdrawRequest(LnurlWithdrawRequestWire),
            Error(LnurlErrorWire),
        }

        let resp = self
            .0
            .get(http_url)
            .send()
            .await
            .context("Failed to fetch LNURL-withdraw endpoint")?
            .json::<RawResponse>()
            .await
            .context("Failed to parse LNURL-withdraw response")?;

        let wire = match resp {
            RawResponse::WithdrawRequest(x) => x,
            RawResponse::Error(LnurlErrorWire { reason, .. }) =>
                return Err(anyhow!("LNURL-withdraw endpoint: {reason}")),
        };

        LnurlWithdrawRequest::try_from_wire(wire)
    }

    /// Completes a withdraw request by sending a GET
    /// request to the callback URL with the required parameters.
    ///
    /// The [`Invoice`] to be sent must be provided and validated by the caller.
    pub async fn resolve_withdraw_request(
        &self,
        withdraw_req: &LnurlWithdrawRequest,
        invoice: Invoice,
    ) -> anyhow::Result<()> {
        let callback = &withdraw_req.callback;

        /// The raw LNURL-withdraw callback response. LNURL doesn't use a
        /// consistent response scheme, so an untagged enum is needed.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum RawResponse {
            // TODO(nicole): this is a repeated success/error response pattern
            // across lnurl flows; consider pulling this out eventually?
            Success { status: SuccessStatus },
            Error(LnurlErrorWire),
        }
        #[derive(Deserialize)]
        enum SuccessStatus {
            #[serde(rename = "OK")]
            Ok,
        }

        let resp = self
            .0
            .get(callback)
            .query(&[("k1", &withdraw_req.k1), ("pr", &invoice.to_string())])
            .send()
            .await
            .context("Failed to send LNURL-withdraw callback request")?
            .json::<RawResponse>()
            .await
            .context("Failed to parse LNURL-withdraw callback response")?;

        match resp {
            RawResponse::Success {
                status: SuccessStatus::Ok,
            } => Ok(()),
            RawResponse::Error(LnurlErrorWire { reason, .. }) =>
                Err(anyhow!("LNURL-withdraw callback failed: {reason}")),
        }
    }

    /// Resolve an [`Lnurl`] into LNURL [`PaymentMethod`]s or [`ClaimMethod`]s.
    ///
    /// Compare with [`resolve`](crate::resolve()).
    pub async fn resolve_lnurl(
        &self,
        lnurl: Lnurl<'static>,
    ) -> anyhow::Result<(Vec<PaymentMethod>, Vec<ClaimMethod>)> {
        let lnurl_intermediate = self
            .get_lnurl_intermediate(&lnurl)
            .await
            .context("Failed to resolve LNURL url")?;
        debug!("Resolved LNURL into intermediate: {lnurl_intermediate:?}");
        match lnurl_intermediate {
            LnurlIntermediate::Pay(pay_request) => Ok((
                vec![PaymentMethod::LnurlPay {
                    lnurl: lnurl.http_url.into_owned(),
                    pay_request,
                }],
                Vec::new(),
            )),
            LnurlIntermediate::Withdraw(withdraw_request) => {
                let mut payments = Vec::with_capacity(2);
                let mut claims = Vec::with_capacity(1);

                // LUD-19 LNURL-withdraw may contain LNURL-pay
                if let Some(pay_link) = &withdraw_request.pay_link {
                    match Lnurl::parse(pay_link) {
                        Ok(pay_lnurl) => {
                            let pay_request =
                                self.get_pay_request(&pay_lnurl).await;
                            match pay_request {
                                Ok(pay_request) =>
                                    payments.push(PaymentMethod::LnurlPay {
                                        lnurl: pay_lnurl.http_url.into_owned(),
                                        pay_request,
                                    }),
                                Err(e) => debug!(
                                    "Failed to resolve LNURL-pay linked \
                                     from LNURL-withdraw: {e:#}"
                                ),
                            }
                        }
                        Err(e) => debug!(
                            "Failed to parse LNURL-pay link from \
                             LNURL-withdraw: {e:#}"
                        ),
                    }
                }

                // LNURL-withdraw
                claims.push(ClaimMethod::LnurlWithdraw {
                    lnurl: lnurl.http_url.into_owned(),
                    withdraw_request,
                });

                Ok((payments, claims))
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use lexe_common::{env::DeployEnv, ln::amount::Amount};
    use lexe_crypto::rng::{RngExt, ThreadFastRng};
    use lexe_hex::hex;
    use tracing::info;

    use super::*;

    /// Live test that resolves D++'s Lightning Address me@dplus.plus.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p lexe-payment-uri test_lightning_address_dplus -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_lightning_address_dplus() {
        lexe_logger::init_for_testing();

        let ln_address = "me@dplus.plus";
        info!("Lightning Address: {ln_address}");

        let payment_uri =
            lexe_payment_uri_core::PaymentUri::parse(ln_address).unwrap();

        let email_like = match payment_uri {
            lexe_payment_uri_core::PaymentUri::EmailLikeAddress(email_like) =>
                email_like,
            other => panic!("Expected EmailLikeAddress, got: {other:?}"),
        };

        let ln_address_url = email_like.lightning_address_url;
        let lnurl = Lnurl::from_http_url(&ln_address_url).unwrap();
        info!("Lightning Address URL: {ln_address_url}");

        let lnurl_client = LnurlClient::new(DeployEnv::Prod).unwrap();

        let pay_request = tokio::time::timeout(
            Duration::from_secs(10),
            lnurl_client.get_pay_request(&lnurl),
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
            let amount_msat = rng.gen_range_u32(
                min_sendable.msat() as u32..max_sendable.msat() as u32,
            );
            Amount::from_msat(amount_msat as u64)
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

    // ```bash
    // $ RUST_LOG=debug \
    //     ADDRESS="..." \
    //     AMOUNT="..." \
    //     COMMENT="..." \
    //     cargo test -p lexe-payment-uri -- dump_lightning_address --nocapture --ignored
    // ```
    #[tokio::test]
    #[ignore]
    async fn dump_lightning_address() {
        lexe_logger::init_for_testing();

        // Parse ADDRESS from env
        let ln_address = std::env::var("ADDRESS").expect("`$ADDRESS` not set");
        println!("Lightning address: {ln_address}");

        // Parse AMOUNT from env
        let amount = std::env::var("AMOUNT")
            .map(|s| str::parse::<u32>(&s).unwrap())
            .ok()
            .unwrap_or(1);
        println!("Amount: {amount}");

        // Parse COMMENT from env
        let comment = std::env::var("COMMENT").ok();
        if let Some(comment) = &comment {
            println!("Comment: {comment}");
        } else {
            println!("No comment found.");
        }

        // Parse URI
        let payment_uri =
            lexe_payment_uri_core::PaymentUri::parse(&ln_address).unwrap();

        let email_like = match payment_uri {
            lexe_payment_uri_core::PaymentUri::EmailLikeAddress(email_like) =>
                email_like,
            other => panic!("Expected EmailLikeAddress, got: {other:?}"),
        };

        let ln_address_url = email_like.lightning_address_url;
        println!("Lightning Address URL: {ln_address_url}");

        // Make pay request
        let lnurl_client = LnurlClient::new(DeployEnv::Prod).unwrap();
        let lnurl = Lnurl::from_http_url(&ln_address_url).unwrap();
        let pay_request = tokio::time::timeout(
            Duration::from_secs(10),
            lnurl_client.get_pay_request(&lnurl),
        )
        .await
        .unwrap()
        .unwrap();

        println!("Lightning Address successfully resolved into payRequest");

        let callback = &pay_request.callback;
        let min_sendable = pay_request.min_sendable;
        let max_sendable = pay_request.max_sendable;
        let description = &pay_request.metadata.description;
        let comment_allowed = pay_request.comment_allowed;
        println!("Callback URL: {callback}");
        println!("Min amount: {min_sendable} sats");
        println!("Max amount: {max_sendable} sats");
        println!("Description: {description}");
        println!("Comment allowed: {comment_allowed:?}");

        // Request invoice
        let amount = Amount::from_sats_u32(amount);
        let comment = comment.as_deref();

        println!("Requesting invoice for {amount} sats");
        let invoice = tokio::time::timeout(
            Duration::from_secs(10),
            lnurl_client.resolve_pay_request(&pay_request, amount, comment),
        )
        .await
        .unwrap()
        .unwrap();

        let amount = invoice.amount();
        let description_str = invoice.description_str();
        let description_hash =
            invoice.description_hash().map(|dh| hex::display(dh));
        let network = invoice.network();
        println!("Successfully received invoice: {invoice}");
        println!("Invoice amount: {amount:?}");
        println!("Invoice description_str: {description_str:?}");
        println!("Invoice description_hash: {description_hash:?}");
        println!("Invoice network: {network}");
    }
}
