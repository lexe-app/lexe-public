use std::{include_bytes, time::Duration};

use crate::enclave::{Measurement, MrShort};

// --- General --- //

/// If a node release needs to be yanked, add its semver version and measurement
/// here. See `node::approved_versions` for more info.
// e.g. "0.1.0", "0.2.1-alpha.1".
pub const YANKED_NODE_VERSIONS: [&str; 0] = [];
pub const YANKED_NODE_MEASUREMENTS: [Measurement; 0] = [];
const_utils::const_assert!(
    YANKED_NODE_VERSIONS.len() == YANKED_NODE_MEASUREMENTS.len()
);

// Tokio channels
pub const DEFAULT_CHANNEL_SIZE: usize = 256;
pub const SMALLER_CHANNEL_SIZE: usize = 16;

/// Reject backend requests for payments that are too large.
pub const MAX_PAYMENTS_BATCH_SIZE: u16 = 100;
pub const DEFAULT_PAYMENTS_BATCH_SIZE: u16 = 50;

/// Reject payment notes that are too large.
pub const MAX_PAYMENT_NOTE_BYTES: usize = 512;

/// The amount of time user node tasks have to finish after a graceful shutdown
/// signal is received before the program is forced to exit.
pub const USER_NODE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

// --- Channels and liquidity --- //

/// Our dust limit for e.g. our channel close txo's. If our channel balance,
/// after paying close fees, is <= this value, we will not get a txo and this
/// value is lost (goes to fees).
///
/// LDK just uses a fixed value here.
///
/// See: [`MIN_CHAN_DUST_LIMIT_SATOSHIS`](https://github.com/lightningdevkit/rust-lightning/blob/70add1448b5c36368b8f1c17d672d8871cee14de/lightning/src/ln/channel.rs#L697)
pub const LDK_DUST_LIMIT_SATS: u64 = 354;

/// The amount of liquidity (in sats) that Lexe supplies to us for free, which
/// we are not expected to pay interest on. This is also the amount of liquidity
/// Lexe's LSP will supply to a user in their first JIT zeroconf channel open.
// 50k sats = 0.0005 BTC = $25 at $50k/BTC or $50 at $100k/BTC
pub const FREE_LIQUIDITY_SAT: u32 = 50_000;

/// The maximum amount of liquidity that Lexe will supply to a user in one tx.
pub const MAX_LIQUIDITY_SAT: u32 = 10_000_000; // 0.1 BTC

/// User nodes and the LSP will reject new inbound channels with total channel
/// value larger than this value in satoshis.
pub const CHANNEL_MAX_FUNDING_SATS: u32 = 5 * 1_0000_0000; // 5 BTC

/// User nodes require the LSP to reserve this proportion of the channel value
/// (in millionths) as potential punishment. LDK clamps the actual reserve
/// amount to at least 1000 sats. Since the LSP can't send this amount to the
/// user, the user's inbound liquidity is also reduced by this amount. Used for:
/// [`lightning::util::config::ChannelHandshakeConfig::their_channel_reserve_proportional_millionths`]
pub const LSP_RESERVE_PROP_MILLIONTHS: u32 = 10_000; // 1%

/// The LSP will only accept new inbound channels with channel value at or above
/// this limit in satoshis.
// 0.00005000 BTC = $2.50 at $50k/BTC or $5 at $100k/BTC
pub const LSP_CHANNEL_MIN_FUNDING_SATS: u32 = 5_000;

/// See: [`lightning::util::config::ChannelConfig::force_close_avoidance_max_fee_satoshis`]
//
// 1,000 sats = $1.00 assuming $100k/BTC
pub const FORCE_CLOSE_AVOIDANCE_MAX_FEE_SATS: u64 = 1_000;

// --- VFS --- //

/// The default number of persist retries for important objects.
pub const IMPORTANT_PERSIST_RETRIES: usize = 5;

/// The vfs directory name used by singleton objects.
pub const SINGLETON_DIRECTORY: &str = ".";
/// The vfs directory name used to persist events.
pub const EVENTS_DIR: &str = "events";

pub const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
pub const NETWORK_GRAPH_FILENAME: &str = "network_graph";
pub const PW_ENC_ROOT_SEED_FILENAME: &str = "password_encrypted_root_seed";
pub const SCORER_FILENAME: &str = "scorer";
// Filename history:
// - "bdk_wallet_db" for our pre BDK 1.0 wallet DB.
// - "bdk_wallet_db_v1" for our BDK 1.0.0-alpha.X wallet DB.
// - "bdk_wallet_changeset" since BDK 1.0.0-beta.X.
pub const WALLET_CHANGESET_FILENAME: &str = "bdk_wallet_changeset";

// --- Networking --- //

/// Fake DNS names used by the reverse proxy to route requests to user nodes.
/// Provision mode uses "{mr_short}.provision.lexe.app" and run mode uses
/// "run.lexe.app". These DNS names don't actually resolve.
pub const NODE_RUN_DNS: &str = "run.lexe.app";
pub fn node_provision_dns(mr_short: &MrShort) -> String {
    format!("{mr_short}{NODE_PROVISION_DNS_SUFFIX}")
}
pub const NODE_PROVISION_DNS_SUFFIX: &str = ".provision.lexe.app";

// --- Esplora --- //

// Mainnet Esplora urls
pub const MAINNET_LEXE_MEMPOOL_ESPLORA: &str = "https://lexe.mempool.space/api";
pub const MAINNET_BLOCKSTREAM_ESPLORA: &str = "https://blockstream.info/api";
pub const MAINNET_ESPLORA_WHITELIST: [&str; 2] =
    [MAINNET_LEXE_MEMPOOL_ESPLORA, MAINNET_BLOCKSTREAM_ESPLORA];

// Testnet Esplora urls
// Quickly test these by appending /fee-estimates and opening in browser,
// e.g. "https://testnet.ltbl.io/api/fee-estimates"
pub const TESTNET3_BLOCKSTREAM_ESPLORA: &str =
    "https://blockstream.info/testnet/api";
pub const TESTNET3_LTBL_ESPLORA: &str = "https://testnet.ltbl.io/api";
pub const TESTNET3_LEXE_ESPLORA: &str = "http://testnet.esplora.lexe.app:3001";
pub const TESTNET3_ESPLORA_WHITELIST: [&str; 3] = [
    TESTNET3_BLOCKSTREAM_ESPLORA,
    TESTNET3_LTBL_ESPLORA,
    TESTNET3_LEXE_ESPLORA,
];

// --- Root CA certs --- //
//
// This section contains DER-encoded TLS certs for the root CAs used by various
// websites that we make requests to, including Lexe itself. For security, we
// only allow using reqwest with `rustls-tls-manual-roots`, which trusts 0 roots
// by default. Thus, it is necessary to manually include a root CA cert in our
// TLS config whenever we make a request to Lexe or an external site. For
// extra security, once a `reqwest::Client` has been configured to trust a CA
// root, it should not be reused for requests to sites with different roots.
//
// ### Instructions for adding or updating an external root cert
// (written for Brave / Chrome)
//
// - Visit the website in your browser via HTTPS.
// - Click the HTTPS lock icon and view certificate details.
// - Navigate up the certificate chain until you have selected the root cert.
// - Press "Export..."
// - When prompted for the format, use "DER-encoded binary, single certificate".
// - Save it to "common/data/<root_ca_name>_root-ca-cert.der"
// - Add a `include_bytes!()` entry below with a corresponding test.
// - Tip: You can see the full human-readable cert info with macOS Quick Look.
//
// ### Inspecting a DER-encoded certificate
//
// macOS Finder's "Quick Look" works for some certificates.
// Alternatively, view the cert from the command line:
//
// ```bash
// openssl x509 -inform der -in <certificate-name>.der -text -noout
// ```

/// The Lexe CA responsible for `lexe.app` and `.lx`.
// Serial Number : 73:bb:2d:b0:13:58:d7:1c:ca:a5:d3:56:a7:f3:33:5b:4c:3c:60:8e
//    Not Before : Nov 27 21:44:46 2024 GMT
//     Not After : Dec  4 21:44:46 2034 GMT
pub const LEXE_PROD_CA_CERT_DER: &[u8] =
    include_bytes!("../data/lexe-prod-root-ca-cert.der");

/// The Lexe CA responsible for `staging.lexe.app` and `staging.lx`.
// Serial Number : 30:ef:fb:a0:ba:ca:82:0b:7f:49:9a:46:b7:8d:05:18:23:91:62:17
//    Not Before : Jul 30 02:15:24 2024 GMT
//     Not After : Aug  6 02:15:24 2034 GMT
pub const LEXE_STAGING_CA_CERT_DER: &[u8] =
    include_bytes!("../data/lexe-staging-root-ca-cert.der");

/// Google Trust Services Root R1, used by `googleapis.com`.
// Serial Number : 02:03:E5:93:6F:31:B0:13:49:88:6B:A2:17
//    Not Before : Jun 22 00:00:00 2016 GMT
//     Not After : Jun 22 00:00:00 2036 GMT
pub const GTS_ROOT_R1_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-root-r1-ca-cert.der");

#[cfg(test)]
mod test {
    use reqwest::tls::Certificate;

    use super::*;

    #[test]
    fn test_parse_ca_certs() {
        Certificate::from_der(LEXE_PROD_CA_CERT_DER).unwrap();
        Certificate::from_der(LEXE_STAGING_CA_CERT_DER).unwrap();
        Certificate::from_der(GTS_ROOT_R1_CA_CERT_DER).unwrap();
    }
}
