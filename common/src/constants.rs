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

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:4040";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";
/// NOTE: This is the url that *user nodes* use to establish LN P2P connections
/// with the LSP. External LN nodes use the [`STANDARD_LIGHTNING_P2P_PORT`].
pub const DEFAULT_LSP_URL: &str = "http://127.0.0.1:6060";
pub const DEFAULT_ESPLORA_URL: &str = "http://127.0.0.1:7070";

/// The standard port used for Lightning Network P2P connections
pub const STANDARD_LIGHTNING_P2P_PORT: Port = 9735;

// Blockstream Esplora API
pub const BLOCKSTREAM_ESPLORA_MAINNET_URL: &str =
    "https://blockstream.info/api";
pub const BLOCKSTREAM_ESPLORA_TESTNET_URL: &str =
    "https://blockstream.info/testnet/api";

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
