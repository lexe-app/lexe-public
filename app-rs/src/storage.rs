use std::io;

use anyhow::{anyhow, Context};
use common::api::models::NodeRelease;

use crate::ffs::Ffs;

/// The FFS filename for the file storing the latest release we've provisioned.
const LATEST_PROVISIONED_FILENAME: &str = "latest_provisioned";

/// Read the latest provisioned [`NodeRelease`].
/// Returns [`Ok(None)`] if the file didn't exist.
pub(crate) fn read_latest_provisioned(
    app_data_ffs: &impl Ffs,
) -> anyhow::Result<Option<NodeRelease>> {
    match app_data_ffs.read(LATEST_PROVISIONED_FILENAME) {
        Ok(json_bytes) => serde_json::from_slice(&json_bytes)
            .context("Deserialization failed"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!("Ffs::read failed: {e:#}")),
    }
}

/// Persist the latest provisioned [`NodeRelease`].
pub(crate) fn write_latest_provisioned(
    app_data_ffs: &impl Ffs,
    latest_provisioned: &NodeRelease,
) -> anyhow::Result<()> {
    let json_bytes =
        serde_json::to_vec(&latest_provisioned).expect("Serialization failed?");
    app_data_ffs
        .write(LATEST_PROVISIONED_FILENAME, &json_bytes)
        .context("Ffs::write failed")
}
