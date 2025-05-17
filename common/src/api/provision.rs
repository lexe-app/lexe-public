use std::fmt;

#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    env::DeployEnv, ln::network::LxNetwork, root_seed::RootSeed,
    serde_helpers::hexstr_or_bytes_opt,
};

/// The client sends this request to the provisioning node.
#[derive(Serialize, Deserialize)]
// Only impl PartialEq in tests since root seed comparison is not constant time.
#[cfg_attr(test, derive(PartialEq, Arbitrary))]
pub struct NodeProvisionRequest {
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
    /// The [`DeployEnv`] that this [`RootSeed`] should be bound to.
    pub deploy_env: DeployEnv,
    /// The [`LxNetwork`] that this [`RootSeed`] should be bound to.
    pub network: LxNetwork,
    /// The auth `code` which can used to obtain a set of GDrive credentials.
    /// - Applicable only in staging/prod.
    /// - If provided, the provisioning node will acquire the full set of
    ///   GDrive credentials and persist them (encrypted ofc) in Lexe's DB.
    /// - If NOT provided, the provisioning node will ensure that a set of
    ///   GDrive credentials has already been persisted in Lexe's DB.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub google_auth_code: Option<String>,
    /// Whether this provision instance is allowed to access the user's
    /// `GoogleVfs`. In order to ensure that different provision instances do
    /// not overwrite each other's updates to the `GoogleVfs`, this paramater
    /// must only be `true` for at most one provision instance at a time.
    ///
    /// - The mobile app must always set this to `true`, and must ensure that
    ///   it is only (re-)provisioning one instance at a time. Node version
    ///   approval and revocation (which requires mutating the `GoogleVfs`) can
    ///   only be handled if this is set to `true`.
    /// - Running nodes, which initiate root seed replication, must always set
    ///   this to `false`, so that replicating instances will not overwrite
    ///   updates made by (re-)provisioning instances.
    ///
    /// NOTE that it is always possible that while this instance is
    /// provisioning, the user's node is also running. Even when this parameter
    /// is `true`, the provision instance must be careful not to mutate
    /// `GoogleVfs` data which can also be mutated by a running user node,
    /// unless a persistence race between the provision and run modes is
    /// acceptable.
    ///
    /// See `GoogleVfs::gid_cache` for more info on GVFS consistency.
    pub allow_gvfs_access: bool,
    /// The password-encrypted [`RootSeed`] which should be backed up in
    /// GDrive.
    /// - Applicable only in staging/prod.
    /// - Requires `allow_gvfs_access=true` if `Some`; errors otherwise.
    /// - If `Some`, the provision instance will back up this encrypted
    ///   [`RootSeed`] in Google Drive. If a backup already exists, it is not
    ///   overwritten.
    /// - If `None`, then this will error if we are missing the backup.
    /// - The mobile app should set this to `Some` at least on the very first
    ///   provision. The mobile app can also pass `None` to avoid unnecessary
    ///   work when it is known that the user already has a root seed backup.
    /// - Replication (from running nodes) should always set this to `None`.
    /// - We require the client to password-encrypt prior to sending the
    ///   provision request to prevent leaking the length of the password. It
    ///   also shifts the burden of running the 600K HMAC iterations from the
    ///   provision instance to the mobile app.
    #[serde(with = "hexstr_or_bytes_opt")]
    pub encrypted_seed: Option<Vec<u8>>,
}

impl fmt::Debug for NodeProvisionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NodeProvisionRequest { .. }")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{rng::FastRng, test_utils::roundtrip};

    #[test]
    fn test_node_provision_request_sample() {
        let mut rng = FastRng::from_u64(12345);
        let req = NodeProvisionRequest {
            root_seed: RootSeed::from_rng(&mut rng),
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            google_auth_code: Some("auth_code".to_owned()),
            allow_gvfs_access: false,
            encrypted_seed: None,
        };
        let actual = serde_json::to_value(&req).unwrap();
        let expected = serde_json::json!({
            "root_seed": "0a7d28d375bc07250ca30e015a808a6d70d43c5a55c4d5828cdeacca640191a1",
            "deploy_env": "dev",
            "network": "regtest",
            "google_auth_code": "auth_code",
            "allow_gvfs_access": false,
            "encrypted_seed": null,
        });
        assert_eq!(&actual, &expected);
    }

    #[test]
    fn test_node_provision_request_json_canonical() {
        roundtrip::json_value_roundtrip_proptest::<NodeProvisionRequest>();
    }
}
