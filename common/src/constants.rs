use std::include_bytes;

use crate::{
    api::ports::Port,
    const_assert,
    enclave::{Measurement, MrShort},
};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;
pub const SMALLER_CHANNEL_SIZE: usize = 16;

/// If a node release needs to be yanked, add its semver version and measurement
/// here. See `node::approved_versions` for more info.
// e.g. "0.1.0", "0.2.1-alpha.1".
pub const YANKED_NODE_VERSIONS: [&str; 0] = [];
pub const YANKED_NODE_MEASUREMENTS: [Measurement; 0] = [];
const_assert!(YANKED_NODE_VERSIONS.len() == YANKED_NODE_MEASUREMENTS.len());

/// The default number of persist retries for important objects.
pub const IMPORTANT_PERSIST_RETRIES: usize = 5;
/// The vfs directory name used by singleton objects.
pub const SINGLETON_DIRECTORY: &str = ".";
/// The vfs filename used for the `WalletDb`.
pub const WALLET_DB_FILENAME: &str = "bdk_wallet_db";

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
pub const TESTNET_BLOCKSTREAM_ESPLORA: &str =
    "https://blockstream.info/testnet/api";
pub const TESTNET_KUUTAMO_ESPLORA: &str =
    "https://esplora.testnet.kuutamo.cloud";
pub const TESTNET_LTBL_ESPLORA: &str = "https://testnet-electrs.ltbl.io:3004";
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

/// The Lexe CA responsible for "staging.lexe.app".
// Serial Number: 30:ef:fb:a0:ba:ca:82:0b:7f:49:9a:46:b7:8d:05:18:23:91:62:17
// Not Before: Jun  7 20:37:57 2024 GMT
// Not After : Jun 14 20:37:57 2034 GMT
pub const LEXE_STAGING_CA_CERT_DER: &[u8] =
    include_bytes!("../data/lexe-staging-root-ca-cert.der");

/// Google Trust Services Root R1, used by googleapis.com, blockstream.info,
/// and kuutamo.cloud.
// Serial Number=02:03:E5:93:6F:31:B0:13:49:88:6B:A2:17
// Not Valid Before=Wednesday, June 22, 2016 at 8:00:00 AM China Standard Time
// Not Valid After=Sunday, June 22, 2036 at 8:00:00 AM China Standard Time
pub const GTS_ROOT_R1_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-root-r1-ca-cert.der");
/// Google Trust Services Root R4, used by coincap.io.
pub const GTS_ROOT_R4_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-root-r4-ca-cert.der");

/// The root CA cert for Amazon's Root CA 1, used by `ltbl.io`.
// Serial Number=82:10:cf:b0:d2:40:e3:59:44:63:e0:bb:63:82:8b:00
// Not Valid Before=Thursday, June 4, 2015 at 7:04:38 PM China Standard Time
// Not Valid After=Monday, June 4, 2035 at 7:04:38 PM China Standard Time
pub const LETSENCRYPT_ROOT_CA_CERT_DER: &[u8] =
    include_bytes!("../data/letsencrypt-isrg-root-x1-cert.der");

#[cfg(test)]
mod test {
    use reqwest::tls::Certificate;

    use super::*;

    #[test]
    fn test_parse_ca_certs() {
        Certificate::from_der(LEXE_STAGING_CA_CERT_DER).unwrap();
        Certificate::from_der(GTS_ROOT_R1_CA_CERT_DER).unwrap();
        Certificate::from_der(LETSENCRYPT_ROOT_CA_CERT_DER).unwrap();
    }
}
