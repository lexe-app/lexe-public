use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io,
};

use anyhow::{anyhow, Context};
use common::{
    api::version::{CurrentEnclaves, NodeEnclave},
    constants,
    env::DeployEnv,
    releases::Release,
};
use serde::Deserialize;

use crate::ffs::Ffs;

/// Tracks all node releases that have even been provisioned.
#[derive(Debug, Default)]
pub(crate) struct ProvisionHistory {
    /// All node releases which have previously been provisioned.
    pub provisioned: BTreeSet<NodeEnclave>,
}

impl ProvisionHistory {
    /// The FFS filename for the file storing the provision history.
    /// NOTE: on version 0.8.8 file was renamed from "provision_history"
    /// to "provision_history_v2".
    pub const FFS_FILENAME: &'static str = "provision_history_v2";

    /// Create a new empty provision history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the provision history from a [`Ffs`].
    /// Returns an empty [`ProvisionHistory`] if the file didn't exist.
    pub fn read_from_ffs(provision_ffs: &impl Ffs) -> anyhow::Result<Self> {
        match provision_ffs.read(Self::FFS_FILENAME) {
            Ok(json_bytes) => Self::from_json_bytes(&json_bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(anyhow!("Ffs::read failed: {e:#}")),
        }
    }

    /// Persist this provision history to storage.
    pub fn write_to_ffs(&self, provision_ffs: &impl Ffs) -> anyhow::Result<()> {
        provision_ffs
            .write(Self::FFS_FILENAME, &self.to_json_bytes())
            .context("Ffs::write failed")
    }

    /// Serialize the provision history to JSON bytes.
    fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&self.provisioned)
            .expect("Serialization should never fail")
    }

    /// Deserialize the provision history from JSON bytes.
    fn from_json_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let provisioned = serde_json::from_slice(bytes)
            .context("Failed to deserialize provision history")?;
        Ok(Self { provisioned })
    }

    /// Marks a enclave as having been successfully provisioned,
    /// and persists the updated [`ProvisionHistory`] to storage.
    ///
    /// Returns true if the release was newly inserted.
    pub fn update_and_persist(
        &mut self,
        enclave: NodeEnclave,
        provision_ffs: &impl Ffs,
    ) -> anyhow::Result<bool> {
        let was_inserted = self.provisioned.insert(enclave);
        self.write_to_ffs(provision_ffs)?;
        Ok(was_inserted)
    }

    /// Given the current enclaves from the API, returns the subset of them
    /// which are:
    ///
    /// 1) trusted (contained in the hard-coded releases.json),
    /// 2) not yet provisioned (not in the provision history), and
    /// 3) recent (contained in the three latest trusted releases)
    ///
    /// In dev, all releases are allowed.
    // We use `current_enclaves` instead of a `latest_releases` API because this
    // gives old app clients (which may not trust any of the last N releases) a
    // chance to still provision the latest releases that they trust.
    pub fn releases_to_provision(
        &self,
        deploy_env: DeployEnv,
        current_enclaves: CurrentEnclaves,
    ) -> BTreeSet<NodeEnclave> {
        let trusted_releases = trusted_releases();

        // Only consider the three latest trusted releases.
        // There is no need to provision anything older than this.
        let latest_trusted_measurements = trusted_releases
            .values()
            .rev()
            .take(constants::RELEASE_WINDOW_SIZE)
            .map(|release| release.measurement)
            .collect::<HashSet<_>>();

        current_enclaves
            .enclaves
            .into_iter()
            // If we're in staging or prod, only consider trusted releases
            .filter(|enclave| {
                if deploy_env.is_staging_or_prod() {
                    latest_trusted_measurements.contains(&enclave.measurement)
                } else {
                    true
                }
            })
            // Filter out any enclaves which have already been provisioned
            .filter(|enclave| !self.provisioned.contains(enclave))
            .collect()
    }
}

/// Returns the set of trusted node releases (populated from releases.json).
/// The user trusts these releases simply by installing the open-source app
/// which has these values hard-coded. This prevents Lexe from pushing out
/// unilateral node updates without the user's consent.
pub fn trusted_releases() -> BTreeMap<semver::Version, Release> {
    const RELEASES_JSON: &str = include_str!("../../releases.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct ReleasesJson(BTreeMap<String, BTreeMap<semver::Version, Release>>);

    serde_json::from_str::<ReleasesJson>(RELEASES_JSON)
        .expect("Checked in tests")
        .0
        .remove("node")
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use common::enclave;

    use super::*;

    #[test]
    fn test_trusted_releases() {
        trusted_releases();
    }

    #[test]
    fn provision_history_snapshot() {
        // Dummy provision history
        let provision_history = {
            let provisioned = BTreeSet::from_iter([
                NodeEnclave {
                    version: semver::Version::from_str("0.1.0").unwrap(),
                    measurement: enclave::Measurement::new([0x11; 32]),
                    machine_id: enclave::MachineId::MOCK,
                },
                NodeEnclave {
                    version: semver::Version::from_str("0.2.0-beta.1").unwrap(),
                    measurement: enclave::Measurement::new([0x22; 32]),
                    machine_id: enclave::MachineId::MOCK,
                },
                NodeEnclave {
                    version: semver::Version::from_str("1.0.0").unwrap(),
                    measurement: enclave::Measurement::new([0x33; 32]),
                    machine_id: enclave::MachineId::MOCK,
                },
                NodeEnclave {
                    version: semver::Version::from_str("1.0.0-rc.1+build.123")
                        .unwrap(),
                    measurement: enclave::Measurement::new([0x44; 32]),
                    machine_id: enclave::MachineId::MOCK,
                },
            ]);

            ProvisionHistory { provisioned }
        };

        // Serialize to JSON
        let json_bytes = provision_history.to_json_bytes();
        let json_str = String::from_utf8(json_bytes.clone()).unwrap();

        // Expected serialization (hard-coded snapshot)
        let json_snapshot = r#"[{"version":"0.1.0","measurement":"1111111111111111111111111111111111111111111111111111111111111111","machine_id":"52bc575eb9618084083ca7b3a45a2a76"},{"version":"0.2.0-beta.1","measurement":"2222222222222222222222222222222222222222222222222222222222222222","machine_id":"52bc575eb9618084083ca7b3a45a2a76"},{"version":"1.0.0-rc.1+build.123","measurement":"4444444444444444444444444444444444444444444444444444444444444444","machine_id":"52bc575eb9618084083ca7b3a45a2a76"},{"version":"1.0.0","measurement":"3333333333333333333333333333333333333333333333333333333333333333","machine_id":"52bc575eb9618084083ca7b3a45a2a76"}]"#;

        assert_eq!(json_str, json_snapshot);

        // Verify deserialization works correctly
        let deserialized =
            ProvisionHistory::from_json_bytes(&json_bytes).unwrap();
        assert_eq!(deserialized.provisioned, provision_history.provisioned);

        // Also verify deserialization from the hard-coded string
        let from_snapshot =
            ProvisionHistory::from_json_bytes(json_snapshot.as_bytes())
                .unwrap();
        assert_eq!(from_snapshot.provisioned, provision_history.provisioned);
    }
}
