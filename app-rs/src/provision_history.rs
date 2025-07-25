use std::{collections::BTreeSet, io};

use anyhow::{anyhow, Context};
use common::api::version::NodeRelease;

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

    // /// Delete the provision history file from storage.
    // pub fn delete_from_ffs(app_data_ffs: &impl Ffs) -> anyhow::Result<()> {
    //     match app_data_ffs.delete(Self::FFS_FILENAME) {
    //         Ok(()) => Ok(()),
    //         Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
    //         Err(e) => Err(anyhow!("Ffs::delete failed: {e:#}")),
    //     }
    // }
}
