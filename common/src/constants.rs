use std::{include_bytes, time::Duration};

use crate::{
    api::ports::Port,
    enclave::{Measurement, MrShort},
};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;
pub const SMALLER_CHANNEL_SIZE: usize = 16;

/// If a node release needs to be yanked, add its semver version and measurement
/// here. See `node::approved_versions` for more info.
// e.g. "0.1.0", "0.2.1-alpha.1".
pub const YANKED_NODE_VERSIONS: [&str; 0] = [];
pub const YANKED_NODE_MEASUREMENTS: [Measurement; 0] = [];
const_utils::const_assert!(
    YANKED_NODE_VERSIONS.len() == YANKED_NODE_MEASUREMENTS.len()
);

/// The amount of time user node tasks have to finish after a graceful shutdown
/// signal is received before the program is forced to exit.
pub const USER_NODE_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// The default number of persist retries for important objects.
pub const IMPORTANT_PERSIST_RETRIES: usize = 5;
/// The vfs directory name used by singleton objects.
pub const SINGLETON_DIRECTORY: &str = ".";
/// The vfs filename used for the `WalletDb`.
pub const WALLET_DB_FILENAME: &str = "bdk_wallet_db";
/// The vfs filename used for the user's password-encrypted root seed backup.
pub const PW_ENC_ROOT_SEED_FILENAME: &str = "password_encrypted_root_seed";

/// Reject backend requests for payments that are too large.
pub const MAX_PAYMENTS_BATCH_SIZE: u16 = 100;
pub const DEFAULT_PAYMENTS_BATCH_SIZE: u16 = 50;

/// Reject payment notes that are too large.
pub const MAX_PAYMENT_NOTE_BYTES: usize = 512;

/// The standard port used for Lightning Network P2P connections
pub const STANDARD_LIGHTNING_P2P_PORT: Port = 9735;

// Mainnet Esplora urls
pub const MAINNET_BLOCKSTREAM_ESPLORA: &str = "https://blockstream.info/api";
pub const MAINNET_KUUTAMO_ESPLORA: &str = "https://esplora.kuutamo.cloud";
pub const MAINNET_ESPLORA_WHITELIST: [&str; 2] =
    [MAINNET_BLOCKSTREAM_ESPLORA, MAINNET_KUUTAMO_ESPLORA];

// Testnet Esplora urls
// Quickly test these by appending /fee-estimates and opening in browser,
// e.g. "https://testnet.ltbl.io/api/fee-estimates"
pub const TESTNET_BLOCKSTREAM_ESPLORA: &str =
    "https://blockstream.info/testnet/api";
pub const TESTNET_KUUTAMO_ESPLORA: &str =
    "https://esplora.testnet.kuutamo.cloud";
pub const TESTNET_LTBL_ESPLORA: &str = "https://testnet.ltbl.io/api";
pub const TESTNET_LEXE_ESPLORA: &str = "http://testnet.esplora.lexe.app:3001";
pub const TESTNET_ESPLORA_WHITELIST: [&str; 4] = [
    TESTNET_BLOCKSTREAM_ESPLORA,
    TESTNET_KUUTAMO_ESPLORA,
    TESTNET_LTBL_ESPLORA,
    TESTNET_LEXE_ESPLORA,
];

/// Fake DNS names used by the reverse proxy to route requests to user nodes.
/// Provision mode uses "{mr_short}.provision.lexe.app" and run mode uses
/// "run.lexe.app". These DNS names don't actually resolve.
pub const NODE_RUN_DNS: &str = "run.lexe.app";
pub fn node_provision_dns(mr_short: &MrShort) -> String {
    format!("{mr_short}{NODE_PROVISION_DNS_SUFFIX}")
}
pub const NODE_PROVISION_DNS_SUFFIX: &str = ".provision.lexe.app";

/// User nodes and the LSP will reject new inbound channels with total channel
/// value larger than this value in satoshis.
pub const CHANNEL_MAX_FUNDING_SATS: u32 = 5 * 1_0000_0000; // 5 BTC

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

/// The Lexe CA responsible for `staging.lexe.app` and `staging.lx`.
// Serial Number : 30:ef:fb:a0:ba:ca:82:0b:7f:49:9a:46:b7:8d:05:18:23:91:62:17
//    Not Before : Jul 30 02:15:24 2024 GMT
//     Not After : Aug  6 02:15:24 2034 GMT
pub const LEXE_STAGING_CA_CERT_DER: &[u8] =
    include_bytes!("../data/lexe-staging-root-ca-cert.der");

/// Google Trust Services Root R1, used by `googleapis.com`, `blockstream.info`,
/// and `kuutamo.cloud`.
// Serial Number : 02:03:E5:93:6F:31:B0:13:49:88:6B:A2:17
//    Not Before : Jun 22 00:00:00 2016 GMT
//     Not After : Jun 22 00:00:00 2036 GMT
pub const GTS_ROOT_R1_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-root-r1-ca-cert.der");

/// ISRG Root X1, used by `coincap.io`.
// Serial Number : 82:10:cf:b0:d2:40:e3:59:44:63:e0:bb:63:82:8b:00
//    Not Before : Jun  4 11:04:38 2015 GMT
//     Not After : Jun  4 11:04:38 2035 GMT
pub const ISRG_ROOT_X1_CA_CERT_DER: &[u8] =
    include_bytes!("../data/isrg-root-x1-ca-cert.der");

/// The root CA cert for Amazon's Root CA 1, used by `ltbl.io`.
// Serial Number : 06:6c:9f:cf:99:bf:8c:0a:39:e2:f0:78:8a:43:e6:96:36:5b:ca
//    Not Before : May 26 00:00:00 2015 GMT
//     Not After : Jan 17 00:00:00 2038 GMT
pub const AMAZON_ROOT_CA_1_CERT_DER: &[u8] =
    include_bytes!("../data/amazon-root-ca-1-cert.der");

#[cfg(test)]
mod test {
    use reqwest::tls::Certificate;

    use super::*;

    #[test]
    fn test_parse_ca_certs() {
        Certificate::from_der(LEXE_STAGING_CA_CERT_DER).unwrap();
        Certificate::from_der(GTS_ROOT_R1_CA_CERT_DER).unwrap();
        Certificate::from_der(AMAZON_ROOT_CA_1_CERT_DER).unwrap();
    }
}
