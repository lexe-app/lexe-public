use rcgen::{DistinguishedName, DnType};

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
