//! BIP 353 (DNS Payment Instructions) resolution.
//!
//! This module handles resolution of BIP 353 human-readable names (like
//! `₿alice@example.com`) to Bitcoin payment instructions using DNS TXT
//! records with DNSSEC validation.
//!
//! ## BIP 353 Protocol Flow
//!
//! BIP 353 maps human-readable names like `₿alice@example.com` to Bitcoin
//! payment instructions using DNS TXT records with DNSSEC.
//!
//! 1. **Parse address**: `₿alice@example.com` → Extract `alice` and
//!    `example.com`
//!
//! 2. **Construct DNS name**: `alice.user._bitcoin-payment.example.com.`
//!
//! 3. **Query DNS TXT record** (with DNSSEC validation) → Receive:
//! ```text
//! "bitcoin:bc1qexample?lightning=lnbc..."
//! ```
//!
//! 4. **Handle BIP 21 URI**: Parse as BIP 321 URI, extract payment method.
//! ```json
//! {
//!   "address": "bc1qexample",          // Onchain fallback (optional)
//!   "lightning": "lnbc...",            // BOLT11 invoice
//!   "lno": "lno1...",                  // BOLT12 offer (optional)
//!   "amount": "0.01"                   // Amount in BTC (optional)
//! }
//! ```
//!
//! Key requirements:
//! - MUST validate full DNSSEC chain to DNS root
//! - MUST NOT cache longer than DNS TTL
//! - TXT record MUST start with "bitcoin:"
//! - Multiple TXT strings are concatenated (for >255 char URIs)
//! - Display format: `₿alice@example.com` (₿ prefix for display only)

use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, anyhow, bail, ensure};
use base64::Engine;
use dnssec_prover::{
    query::{ProofBuilder, QueryBuf},
    rr::{Name, RR, TXT_TYPE},
};
use lexe_tls_core::rustls::{self, RootCertStore, pki_types::CertificateDer};
pub use payment_uri_core::*;
use tracing::debug;

/// DNS-over-HTTPS (DOH) endpoint for Google's public DNS resolver.
/// Recommended: The client only trusts Google's root CAs.
pub const GOOGLE_DOH_ENDPOINT: &str = "https://dns.google/dns-query";
/// DNS-over-HTTPS (DOH) endpoint for Cloudflare's public DNS resolver.
/// Not recommended: The client trusts all webpki roots.
// This is because Cloudflare doesn't use a consistent CA provider:
// $ dig CAA cloudflare-dns.com
pub const CLOUDFLARE_DOH_ENDPOINT: &str =
    "https://cloudflare-dns.com/dns-query";

/// Timeout for DNS-over-HTTPS queries.
const DOH_QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// A client for resolving BIP353 addresses using DNS-over-HTTPS.
#[derive(Clone)]
pub struct Bip353Client {
    client: reqwest::Client,
    doh_endpoint: &'static str,
}

impl Bip353Client {
    pub fn new(doh_endpoint: &'static str) -> anyhow::Result<Self> {
        // If using the Google DOH endpoint, trust only Google's root CAs.
        // Otherwise, trust all webpki roots, as Cloudflare's CAs are unstable.
        let root_certs = if doh_endpoint == GOOGLE_DOH_ENDPOINT {
            let mut certs = RootCertStore::empty();
            for cert_der in [
                // Google roots
                common::constants::GTS_ROOT_R1_CA_CERT_DER,
                common::constants::GTS_ROOT_R2_CA_CERT_DER,
                common::constants::GTS_ROOT_R3_CA_CERT_DER,
                common::constants::GTS_ROOT_R4_CA_CERT_DER,
                common::constants::GS_ROOT_R4_CA_CERT_DER,
            ] {
                let cert = CertificateDer::from_slice(cert_der);
                certs
                    .add(cert)
                    .context("Failed to add Google root certificate")?;
            }
            Arc::new(certs)
        } else {
            lexe_tls_core::WEBPKI_ROOT_CERTS.clone()
        };

        // We must use the default ring `CryptoProvider` because unfortunately
        // Google does not support our preferred ciphersuite.
        #[allow(clippy::disallowed_methods)]
        let tls_config = rustls::ClientConfig::builder_with_protocol_versions(
            lexe_tls_core::LEXE_TLS_PROTOCOL_VERSIONS,
        )
        .with_root_certificates(root_certs)
        .with_no_client_auth();

        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(DOH_QUERY_TIMEOUT)
            .use_preconfigured_tls(tls_config)
            .build()
            .context("Failed to build reqwest client")?;

        Ok(Self {
            client,
            doh_endpoint,
        })
    }

    /// Resolves a BIP353 FQDN (e.g. "satoshi.user._bitcoin-payment.lexe.app.")
    /// into [`PaymentMethod`]s using DNS-over-HTTPS.
    ///
    /// DNS-over-HTTPS is robust on VPNs, unlike direct DNS which is often
    /// blocked and leaks queries to your ISP.
    // NOTE: The recursive DNS resolver can see who we're paying.
    // Consider proxying the request over Tor, or using some other scheme.
    pub(super) async fn resolve_bip353_fqdn(
        &self,
        bip353_fqdn: String,
    ) -> anyhow::Result<Vec<PaymentMethod>> {
        // Name::try_from prefers an owned String
        let dns_name = Name::try_from(bip353_fqdn)
            .map_err(|()| anyhow!("BIP353 FQDN is invalid DNS name"))?;

        let dnssec_proof = self
            .get_dnssec_proof_dns_over_https(&dns_name)
            .await
            .context("Failed to get DNS proof over DNS-over-HTTPS")?;

        // NOTE: This BIP353 DNSSEC proof can be stored and later used to prove
        // the link between a BIP 353 human readable name and the
        // resolved payment URI as part of a proof of payment. We're
        // going to punt on this until and unless a user asks for it.
        let _ = dnssec_proof;

        let bip321_uri =
            Bip353Client::dnssec_proof_to_uri(&dns_name, &dnssec_proof)
                .context("Couldn't get payment URI from DNSSEC proof")?;
        debug!("Resolved BIP353 address: {bip321_uri}");

        // Convert the BIP-321 URI to payment methods.
        let payment_methods = bip321_uri.flatten();

        ensure!(
            !payment_methods.is_empty(),
            "Resolved BIP353 address did not contain any supported payment methods",
        );

        Ok(payment_methods)
    }

    /// Fetches the DNSSEC proof for the given DNS name using DNS-over-HTTPS.
    /// This should work even for users on VPNs.
    async fn get_dnssec_proof_dns_over_https(
        &self,
        dns_name: &Name,
    ) -> anyhow::Result<Vec<u8>> {
        // Make DOH queries until there are no more queries left to process, as
        // query answers may require further queries.

        let (mut proof_builder, initial_query) =
            ProofBuilder::new(dns_name, TXT_TYPE);
        let mut pending_queries = vec![initial_query];

        while let Some(query) = pending_queries.pop() {
            let body = self.send_doh_query(&query).await?;

            let answer = {
                let mut buf = QueryBuf::new_zeroed(0);
                buf.extend_from_slice(&body[..]);
                buf
            };

            match proof_builder.process_response(&answer) {
                // More queries to process
                Ok(new_queries) => pending_queries.extend(new_queries),
                Err(e) =>
                    bail!("Error processing DNS-over-HTTPS response: {e:#}"),
            }
        }

        let (dnssec_proof, _ttl) =
            proof_builder.finish_proof().map_err(|()| {
                anyhow!("Failed to build DNSSEC proof from responses")
            })?;

        Ok(dnssec_proof)
    }

    /// Send a single DNS-over-HTTPS query and return the response.
    ///
    /// Should conform to RFC 8484: <https://datatracker.ietf.org/doc/html/rfc8484>
    async fn send_doh_query(&self, query: &[u8]) -> anyhow::Result<Vec<u8>> {
        // Per RFC 8484, the query should be base64url encoded without padding.
        let base64_query =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(query);
        let doh_endpoint = &self.doh_endpoint;
        let url = format!("{doh_endpoint}?dns={base64_query}");

        let body = self
            .client
            .get(url)
            .header("accept", "application/dns-message")
            .send()
            .await
            .context("Failed to send DNS-over-HTTPS request")?
            .bytes()
            .await
            .context("Failed to read DNS-over-HTTPS response body")?;

        Ok(Vec::from(body))
    }

    /// Validates the DNSSEC proof, extracts the BIP 321 URI string from the
    /// 'bitcoin:' TXT record, and parses the result into a [`Bip321Uri`].
    fn dnssec_proof_to_uri(
        dns_name: &Name,
        dnssec_proof: &[u8],
    ) -> anyhow::Result<Bip321Uri> {
        // Parse
        let rrs = dnssec_prover::ser::parse_rr_stream(dnssec_proof).map_err(
            |()| anyhow!("`build_txt_proof_async` generated invalid proof"),
        )?;

        // Verify
        let verified_rrs = dnssec_prover::validation::verify_rr_stream(&rrs)
            .map_err(|_| anyhow!("DNSSEC signatures were invalid"))?;
        let now_ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("Your clock is very wrong!")?
            .as_secs();
        if now_ts < verified_rrs.valid_from {
            return Err(anyhow!("At least one DNSSEC record isn't yet valid."));
        }
        if now_ts > verified_rrs.expires {
            return Err(anyhow!("At least one DNSSEC record is expired."));
        }

        // Resolve any CNAME records contained within
        let resolved_rrs = verified_rrs.resolve_name(dns_name);

        // Find the BIP353 TXT record. Per BIP353, we must ignore any TXT
        // records that don't start with "bitcoin:" (case-insensitive),
        // and there must be exactly one TXT record that starts with
        // "bitcoin:".
        const BITCOIN_PREFIX: &str = "bitcoin:";
        const BITCOIN_PREFIX_LEN: usize = BITCOIN_PREFIX.len();

        let mut bitcoin_txt_records = resolved_rrs
            .into_iter()
            // Only consider TXT records containing valid UTF-8
            .filter_map(|rr| match rr {
                RR::Txt(txt) => String::from_utf8(txt.data.as_vec()).ok(),
                _ => None,
            })
            // Only consider TXT records starting with "bitcoin:"
            // (case-insensitive)
            .filter_map(|txt_str| {
                let (txt_prefix, _) = txt_str
                    .as_bytes()
                    .split_first_chunk::<BITCOIN_PREFIX_LEN>()?;

                if txt_prefix.eq_ignore_ascii_case(BITCOIN_PREFIX.as_bytes()) {
                    Some(txt_str)
                } else {
                    None
                }
            });

        let bip321_uri_str =
            match (bitcoin_txt_records.next(), bitcoin_txt_records.next()) {
                (Some(uri_str), None) => uri_str,
                (None, _) => bail!("No 'bitcoin:' TXT record for BIP353 name"),
                (Some(_), Some(_)) =>
                    bail!("Invalid: Found multiple 'bitcoin:' TXT records"),
            };

        Bip321Uri::parse(&bip321_uri_str)
            .with_context(|| bip321_uri_str)
            .context(
                "Could not parse 'bitcoin:' payment uri from DNS TXT record",
            )
    }
}

/// An implementation for resolving BIP 353 DNS names directly "direct DNS"
/// which we can fall back to if we encounter problems with DNS-over-HTTPS.
#[allow(dead_code)]
#[cfg(test)]
mod direct_dns {
    use std::net::{
        Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6,
    };

    use anyhow::anyhow;
    use dnssec_prover::rr::Name;

    /// Cloudflare Recursive DNS resolver (IPv4): 1.1.1.1
    pub const CLOUDFLARE_DNS_V4: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 1, 1, 1), 53));
    /// Cloudflare Recursive DNS resolver (IPv6): 2606:4700:4700::1111
    pub const CLOUDFLARE_DNS_V6: SocketAddr =
        SocketAddr::V6(SocketAddrV6::new(
            Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111),
            53,
            0,
            0,
        ));

    /// Google Recursive DNS resolver (IPv4): 8.8.8.8
    pub const GOOGLE_DNS_V4: SocketAddr =
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(8, 8, 8, 8), 53));

    /// Google Recursive DNS resolver (IPv6): 2001:4860:4860::8888
    pub const GOOGLE_DNS_V6: SocketAddr = SocketAddr::V6(SocketAddrV6::new(
        Ipv6Addr::new(0x2001, 0x4860, 0x4860, 0, 0, 0, 0, 0x8888),
        53,
        0,
        0,
    ));

    /// Fetches the DNSSEC proof for the given DNS name using direct DNS.
    /// NOTE: This can be blocked by VPNs, as it leaks your DNS queries to your
    /// ISP.
    pub async fn get_dnssec_proof_direct_dns(
        resolver_addr: SocketAddr,
        dns_name: &Name,
    ) -> anyhow::Result<Vec<u8>> {
        dnssec_prover::query::build_txt_proof_async(resolver_addr, dns_name)
            .await
            .map(|(proof, _ttl)| proof)
            .map_err(|e| anyhow!("Failed to collect DNSSEC proof: {e:#}"))
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use common::ln::network::LxNetwork;
    use tracing::info;

    use super::*;

    /// Live test that resolves philip's prod BIP353 address using
    /// DNS-over-HTTPS. This should work even when using a VPN.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_philip_prod_doh -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_philip_prod_doh() {
        do_bip353_resolve_doh(LxNetwork::Mainnet, "philip@lexe.app").await;
    }

    /// Live test that resolves philip's prod BIP353 address using direct DNS.
    ///
    /// NOTE: If you are running Mullvad VPN, queries to Google or Cloudflare
    /// will fail. Turn off your VPN before testing.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_philip_prod_direct -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_philip_prod_direct() {
        do_bip353_resolve_direct(LxNetwork::Mainnet, "philip@lexe.app").await;
    }

    /// Live test that resolves lexetestuser's staging BIP353 address using
    /// DNS-over-HTTPS. This should work even when using a VPN.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_lexetestuser_staging_doh -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_lexetestuser_staging_doh() {
        do_bip353_resolve_doh(
            LxNetwork::Testnet3,
            "lexetestuser@staging.lexe.app",
        )
        .await;
    }

    /// Live test that resolves lexetestuser's staging BIP353 address using
    /// direct DNS.
    ///
    /// NOTE: If you are running Mullvad VPN, queries to Google or Cloudflare
    /// will fail. Turn off your VPN before testing.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_lexetestuser_staging_direct -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_lexetestuser_staging_direct() {
        do_bip353_resolve_direct(
            LxNetwork::Testnet3,
            "lexetestuser@staging.lexe.app",
        )
        .await;
    }

    /// Live test that resolves Matt's BIP353 address using DNS-over-HTTPS.
    /// This should work even when using a VPN.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_bluematt_doh -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_bluematt_doh() {
        do_bip353_resolve_doh(LxNetwork::Mainnet, "matt@mattcorallo.com").await;
    }

    /// Live test that resolves Matt's BIP353 address using direct DNS.
    ///
    /// NOTE: If you are running Mullvad VPN, queries to Google or Cloudflare
    /// will fail. Turn off your VPN before testing.
    ///
    /// ```bash
    /// $ RUST_LOG=debug just cargo-test -p payment-uri test_bip353_bluematt_direct -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore]
    async fn test_bip353_bluematt_direct() {
        do_bip353_resolve_direct(LxNetwork::Mainnet, "matt@mattcorallo.com")
            .await;
    }

    async fn do_bip353_resolve_doh(network: LxNetwork, uri: &str) {
        logger::init_for_testing();

        let payment_uri = PaymentUri::parse(uri).unwrap();

        let email_like = match payment_uri {
            PaymentUri::EmailLikeAddress(email_like) => email_like,
            other => panic!("Expected EmailLikeAddress, got: {other:?}"),
        };

        // Extract BIP353 FQDN
        let bip353_fqdn = email_like
            .bip353_fqdn
            .expect("matt@mattcorallo.com should be valid BIP353");

        info!("Resolving BIP353 FQDN via DNS-over-HTTPS: {bip353_fqdn}");

        let bip353_client = Bip353Client::new(GOOGLE_DOH_ENDPOINT).unwrap();
        let payment_methods = tokio::time::timeout(
            Duration::from_secs(5),
            bip353_client.resolve_bip353_fqdn(bip353_fqdn),
        )
        .await
        .expect("Timed out")
        .unwrap();

        // All should be compatible w/ `network`
        assert!(payment_methods.iter().all(|m| m.supports_network(network)));

        // Should contain a BOLT12 offer
        let num_offers = payment_methods
            .iter()
            .filter(|m| matches!(m, PaymentMethod::Offer(_)))
            .count();
        assert_eq!(num_offers, 1, "Expected exactly one BOLT12 offer");
    }

    async fn do_bip353_resolve_direct(network: LxNetwork, uri: &str) {
        let payment_uri = PaymentUri::parse(uri).unwrap();

        let email_like = match payment_uri {
            PaymentUri::EmailLikeAddress(email_like) => email_like,
            other => panic!("Expected EmailLikeAddress, got: {other:?}"),
        };

        // Extract BIP353 FQDN
        let bip353_fqdn = email_like
            .bip353_fqdn
            .expect("matt@mattcorallo.com should be valid BIP353");

        info!("Resolving BIP353 FQDN via direct DNS: {bip353_fqdn}");

        // Manually call direct DNS function from test module
        let dns_name = Name::try_from(bip353_fqdn.clone())
            .map_err(|()| anyhow!("BIP353 FQDN is invalid DNS name"))
            .unwrap();

        let dnssec_proof = tokio::time::timeout(
            Duration::from_secs(5),
            direct_dns::get_dnssec_proof_direct_dns(
                direct_dns::GOOGLE_DNS_V4,
                &dns_name,
            ),
        )
        .await
        .expect("Timed out")
        .unwrap();

        // Validate and extract URI
        let bip321_uri =
            Bip353Client::dnssec_proof_to_uri(&dns_name, &dnssec_proof)
                .unwrap();
        debug!("Resolved BIP353 address: {bip321_uri}");

        // Convert to payment methods
        let payment_methods = bip321_uri.flatten();

        // All should be compatible w/ `network`
        assert!(payment_methods.iter().all(|m| m.supports_network(network)));

        // Should contain a BOLT12 offer
        let num_offers = payment_methods
            .iter()
            .filter(|m| matches!(m, PaymentMethod::Offer(_)))
            .count();
        assert_eq!(num_offers, 1, "Expected exactly one BOLT12 offer");
    }
}
