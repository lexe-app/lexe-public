use rcgen::{DistinguishedName, DnType};

use crate::api::runner::Port;

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:4040";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";
/// The default url that user nodes use for P2P connections to the LSP
pub const DEFAULT_LSP_USER_NODE_URL: &str = "http://127.0.0.1:6060";
/// The default url that external LN nodes use to connect to the LSP
pub const DEFAULT_LSP_EXTERNAL_URL: &str = "http://127.0.0.1:9735";

/// The standard port used for Lightning Network P2P connections
pub const STANDARD_LIGHTNING_P2P_PORT: Port = 9735;

/// Fake DNS name used by the node reverse proxy to route owner requests to a
/// node awaiting provisioning. This DNS name doesn't actually resolve.
pub const NODE_PROVISION_DNS: &str = "provision.lexe.tech";

/// Fake DNS name used by the node reverse proxy to route owner requests to a
/// running node. This DNS name doesn't actually resolve.
pub const NODE_RUN_DNS: &str = "run.lexe.tech";

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-tech");
    name
}
