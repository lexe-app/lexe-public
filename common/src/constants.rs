use std::include_bytes;

use rcgen::{DistinguishedName, DnType};

use crate::api::ports::Port;

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

// Blockstream Esplora API
pub const BLOCKSTREAM_ESPLORA_MAINNET_URL: &str =
    "https://blockstream.info/api";
pub const LEXE_ESPLORA_TESTNET_URL: &str =
    "http://esplora-testnet.lexe.tech:3001";

/// Fake DNS name used by the node reverse proxy to route owner requests to a
/// node awaiting provisioning. This DNS name doesn't actually resolve.
pub const NODE_PROVISION_DNS: &str = "provision.lexe.tech";
pub const NODE_PROVISION_HTTPS: &str = "https://provision.lexe.tech";

/// Fake DNS name used by the node reverse proxy to route owner requests to a
/// running node. This DNS name doesn't actually resolve.
pub const NODE_RUN_DNS: &str = "run.lexe.tech";
pub const NODE_RUN_HTTPS: &str = "https://run.lexe.tech";

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-tech");
    name
}

/// The certificate for Google Trust Services, i.e. blockstream.info's CA. Since
/// we trust 0 roots by default, it is necessary to include this cert in our TLS
/// config whenever we make a request to blockstream.info. For added security,
/// don't use the GTS-trusting [`reqwest::Client`] for requests to other sites.
// Not Valid Before=Thursday, June 18, 2020 at 5:00:42 PM Pacific Daylight Time
// Not Valid After=Thursday, January 27, 2028 at 4:00:42 PM Pacific Standard
// Time Tip: You can see the full human-readable cert info with macOS Quick
// Look.
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
