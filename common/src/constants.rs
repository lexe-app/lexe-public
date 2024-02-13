use std::include_bytes;

use rcgen::{DistinguishedName, DnType};

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
    format!("{mr_short}.{NODE_PROVISION_DNS_SUFFIX}")
}
pub const NODE_PROVISION_DNS_SUFFIX: &str = "provision.lexe.app";

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-app");
    name
}

// --- Root CA certs --- //
//
// This section contains DER-encoded TLS certs for the root CAs used by various
// websites that we make requests to. For security, we only allow using reqwest
// with `rustls-tls-manual-roots`, which trusts 0 roots by default. Thus, it is
// necessary to manually include a root CA cert in our TLS config whenever we
// make a request to an external site. For additional security, once a
// `reqwest::Client` has been configured to trust a CA root, it should not be
// reused for requests to other sites with different roots.
//
// ### Instructions for adding or updating a root cert
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

/// The root CA cert for Google Trust Services Root R1, used by googleapis.com,
/// blockstream.info, and kuutamo.cloud.
// Not Valid Before=Wednesday, June 22, 2016 at 8:00:00 AM China Standard Time
// Not Valid After=Sunday, June 22, 2036 at 8:00:00 AM China Standard Time
pub const GTS_ROOT_R1_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-root-r1-ca-cert.der");
/// The root CA cert for Amazon's Root CA 1, used by `ltbl.io`.
// Not Valid Before=Thursday, June 4, 2015 at 7:04:38 PM China Standard Time
// Not Valid After=Monday, June 4, 2035 at 7:04:38 PM China Standard Time
pub const LETSENCRYPT_ROOT_CA_CERT_DER: &[u8] =
    include_bytes!("../data/letsencrypt-isrg-root-x1-cert.der");
/// The root CA cert for Baltimore CyberTrust Root, used by `coincap.io`.
// Not Valid Before=Saturday, May 13, 2000 at 2:46:00 AM China Standard Time
// Not Valid After=Tuesday, May 13, 2025 at 7:59:00 AM China Standard Time
pub const BALTIMORE_CYBERTRUST_ROOT_CA_CERT_DER: &[u8] =
    include_bytes!("../data/baltimore-cybertrust-root-ca-cert.der");

#[cfg(test)]
mod test {
    use reqwest::tls::Certificate;

    use super::*;

    #[test]
    fn test_parse_ca_certs() {
        Certificate::from_der(GTS_ROOT_R1_CA_CERT_DER).unwrap();
        Certificate::from_der(LETSENCRYPT_ROOT_CA_CERT_DER).unwrap();
        Certificate::from_der(BALTIMORE_CYBERTRUST_ROOT_CA_CERT_DER).unwrap();
    }
}
