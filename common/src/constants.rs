use std::net::SocketAddr;

use once_cell::sync::Lazy;
use rcgen::{DistinguishedName, DnType};
use secrecy::Secret;

use crate::api::ports::Port;
use crate::api::NodePk;
use crate::ln::peer::ChannelPeer;
use crate::rng::SysRng;
use crate::root_seed::RootSeed;

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

/// The node pubkey the user node expects when connecting to the LSP.
// TODO: Replace this with the LSP's real NodePk.
pub static DEFAULT_LSP_NODE_PK: Lazy<NodePk> = Lazy::new(|| {
    let mut rng = SysRng::new();
    let root_seed = RootSeed::new(Secret::new([42u8; 32]));
    root_seed.derive_node_pk(&mut rng)
});

/// The node pubkey and [`SocketAddr`] of the LSP. Note that the socket address
/// is used by *user nodes* to establish a P2P connection to the LSP. External
/// LN nodes use the [`STANDARD_LIGHTNING_P2P_PORT`].
// TODO: Replace this with the LSP's real NodePk.
pub static DEFAULT_LSP: Lazy<ChannelPeer> = Lazy::new(|| {
    let node_pk = *DEFAULT_LSP_NODE_PK;
    let addr = SocketAddr::from(([127, 0, 0, 1], 6060));
    ChannelPeer { node_pk, addr }
});

/// The standard port used for Lightning Network P2P connections
pub const STANDARD_LIGHTNING_P2P_PORT: Port = 9735;

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
