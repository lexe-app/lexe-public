use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io,
};

use anyhow::{anyhow, Context};
use common::{
    api::version::{CurrentReleases, NodeRelease},
    constants,
    env::DeployEnv,
    releases::Release,
};
use serde::Deserialize;

use crate::ffs::Ffs;

/// Tracks all node releases that have even been provisioned.
// TODO(max): Should track provisioned machine ids too, so we can replicate to
// new machines even if we've already provisioned a release.
#[derive(Debug, Default)]
pub(crate) struct ProvisionHistory {
    /// All node releases which have previously been provisioned.
    pub provisioned: BTreeSet<NodeRelease>,
}

impl ProvisionHistory {
    /// The FFS filename for the file storing the provision history.
    pub const FFS_FILENAME: &'static str = "provision_history";

    /// Create a new empty provision history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Read the provision history from a [`Ffs`].
    /// Returns an empty [`ProvisionHistory`] if the file didn't exist.
    pub fn read_from_ffs(app_data_ffs: &impl Ffs) -> anyhow::Result<Self> {
        match app_data_ffs.read(Self::FFS_FILENAME) {
            Ok(json_bytes) => {
                let provisioned = serde_json::from_slice(&json_bytes)
                    .context("Deserialization failed")?;
                Ok(Self { provisioned })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::new()),
            Err(e) => Err(anyhow!("Ffs::read failed: {e:#}")),
        }
    }

    /// Persist this provision history to storage.
    pub fn write_to_ffs(&self, app_data_ffs: &impl Ffs) -> anyhow::Result<()> {
        let json_bytes = serde_json::to_vec(&self.provisioned)
            .expect("Serialization failed?");
        app_data_ffs
            .write(Self::FFS_FILENAME, &json_bytes)
            .context("Ffs::write failed")
    }

    /// Marks a release as having been successfully provisioned,
    /// and persists the updated [`ProvisionHistory`] to storage.
    ///
    /// Returns true if the release was newly inserted.
    pub fn update_and_persist(
        &mut self,
        release: NodeRelease,
        app_data_ffs: &impl Ffs,
    ) -> anyhow::Result<bool> {
        let was_inserted = self.provisioned.insert(release);
        self.write_to_ffs(app_data_ffs)?;
        Ok(was_inserted)
    }

    /// Given the current releases from the API, returns the subset of them
    /// which are:
    ///
    /// 1) trusted (contained in the hard-coded releases.json),
    /// 2) not yet provisioned (not in the provision history), and
    /// 3) recent (contained in the three latest trusted releases)
    ///
    /// In dev, all releases are allowed.
    // We use `current_releases` instead of a `latest_releases` API because this
    // gives old app clients (which may not trust any of the last N releases) a
    // chance to still provision the latest releases that they trust.
    pub fn releases_to_provision(
        &self,
        deploy_env: DeployEnv,
        current_releases: CurrentReleases,
    ) -> BTreeSet<NodeRelease> {
        let trusted_releases = trusted_releases();

        // Only consider the three latest trusted releases.
        // There is no need to provision anything older than this.
        let latest_trusted_measurements = trusted_releases
            .values()
            .rev()
            .take(constants::RELEASE_WINDOW_SIZE)
            .map(|release| release.measurement)
            .collect::<HashSet<_>>();

        current_releases
            .releases
            .into_iter()
            // If we're in staging or prod, only consider trusted releases
            .filter(|release| {
                if deploy_env.is_staging_or_prod() {
                    latest_trusted_measurements.contains(&release.measurement)
                } else {
                    true
                }
            })
            // Filter out any releases which have already been provisioned
            .filter(|release| !self.provisioned.contains(release))
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
    use super::*;

    #[test]
    fn test_trusted_releases() {
        trusted_releases();
    }
}
