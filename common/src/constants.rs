use std::include_bytes;

use rcgen::{DistinguishedName, DnType};

use crate::{api::ports::Port, enclave::MrShort};

pub const DEFAULT_CHANNEL_SIZE: usize = 256;
pub const SMALLER_CHANNEL_SIZE: usize = 16;

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
pub const TESTNET_LEXE_ESPLORA: &str = "http://esplora-testnet.lexe.tech:3001";
pub const TESTNET_ESPLORA_WHITELIST: [&str; 3] = [
    TESTNET_BLOCKSTREAM_ESPLORA,
    TESTNET_KUUTAMO_ESPLORA,
    TESTNET_LEXE_ESPLORA,
];

/// Fake DNS names used by the reverse proxy to route requests to user nodes.
/// Provision mode uses "{mr_short}.provision.lexe.tech" and run mode uses
/// "run.lexe.tech". These DNS names don't actually resolve.
pub const NODE_RUN_DNS: &str = "run.lexe.tech";
pub fn node_provision_dns(mr_short: &MrShort) -> String {
    format!("{mr_short}.{NODE_PROVISION_DNS_SUFFIX}")
}
pub const NODE_PROVISION_DNS_SUFFIX: &str = "provision.lexe.tech";

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-app");
    name
}

/// The certificate for Google Trust Services, i.e. the CA for blockstream.info
/// and kuutamo.cloud. Since we trust 0 roots by default, it is necessary to
/// include this cert in our TLS config whenever we make a request to either of
/// these sites. For added security, don't use the GTS-trusting
/// [`reqwest::Client`] for requests to other sites.
// Not Valid Before=Thursday, June 18, 2020 at 5:00:42 PM PDT
// Not Valid After=Thursday, January 27, 2028 at 4:00:42 PM PST
// Tip: You can see the full human-readable cert info with macOS Quick Look.
pub const GOOGLE_CA_CERT_DER: &[u8] =
    include_bytes!("../data/google-trust-services-ca-cert.der");

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn google_ca_cert_der_parses() {
        use reqwest::tls::Certificate;
        Certificate::from_der(GOOGLE_CA_CERT_DER).unwrap();
    }
}
