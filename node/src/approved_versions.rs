//! Node version approval and revocation. The purpose of this module is to
//! implement a revocation system which prevents Lexe from running old versions
//! which are no longer approved by the user, or which may be vulnerable.
//! This node version approval and revocation system relies on the rollback
//! protection provided by the user's 3rd party cloud.

use std::collections::{btree_map::Entry, BTreeMap};

use anyhow::ensure;
use common::{
    api::user::UserPk,
    constants::{YANKED_NODE_MEASUREMENTS, YANKED_NODE_VERSIONS},
    enclave::Measurement,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::SEMVER_VERSION;

/// The set of versions which are currently approved to run.
/// Contains up to [`MAX_SIZE`] approved versions.
///
/// - *Approval*: When a node version is provisioned, its semver version and
///   measurement are added to the [`approved`] list, if it didn't exist. If the
///   [`ApprovedVersions`] was updated, it is E2E-encrypted and (re)persisted to
///   the user's 3rd party cloud.
/// - *Rolling revocations*: If adding the provisioned node version results in
///   the [`approved`] list having greater than [`MAX_SIZE`] entries, the
///   version(s) which are no longer in the [`MAX_SIZE`] newest versions
///   according to semver precedence are removed from the list (revoked). So
///   Lexe knows not to schedule this version, the corresponding sealed seed is
///   also deleted from Lexe's DB (although the user has no way to verify this).
/// - *Enforcement*: At user node runtime, the [`ApprovedVersions`] is fetched
///   from the 3rd party cloud. If the current version and measurement is not
///   contained in the [`approved`] list, or if the list is not found at all,
///   the user node shuts itself down.
/// - *Yanking*: If there is a vulnerability or critical error discovered in
///   some release, it is added to [`YANKED_NODE_VERSIONS`] and
///   [`YANKED_NODE_MEASUREMENTS`]. The next user node release will contain the
///   updated consts. When the user provisions to the new node release, anything
///   in [`approved`] which is also contained inside these consts is removed.
///   For safety, Lexe, who always has access to the up-to-date consts, will
///   also not schedule versions which are found in these consts.
///
/// [`MAX_SIZE`]: Self::MAX_SIZE
/// [`approved`]: Self::approved
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ApprovedVersions {
    /// List of currently-approved versions, along with their measurements.
    pub(crate) approved: BTreeMap<semver::Version, Measurement>,
}

// Implementation assumption
lexe_std::const_assert!(ApprovedVersions::MAX_SIZE > 0);

impl ApprovedVersions {
    /// The maximum number of approved versions.
    const MAX_SIZE: usize = 3;

    /// Get a new [`ApprovedVersions`] which is completely empty.
    pub(crate) fn new() -> Self {
        let approved = BTreeMap::new();
        Self { approved }
    }

    /// Approve the current version/measurement, and revoke any sufficiently old
    /// or yanked measurements, to be called during provisioning.
    ///
    /// Returns a [`bool`] representing whether this struct has been updated
    /// (and thus whether it should be (re)persisted), along with the version
    /// and measurement of any versions which were revoked.
    ///
    /// Errors if the current version is too old to be approved, or if
    /// [`ApprovedVersions`] contains inconsistent data.
    pub(crate) fn approve_and_revoke(
        &mut self,
        user_pk: &UserPk,
        cur_measurement: Measurement,
    ) -> anyhow::Result<(bool, Vec<(semver::Version, Measurement)>)> {
        let mut updated = false;
        let cur_version =
            semver::Version::parse(SEMVER_VERSION).expect("Checked in tests");

        if self.approved.len() > Self::MAX_SIZE {
            let approved_len = self.approved.len();
            // This will be corrected later
            warn!(
                "Approval list somehow had {approved_len} entries. \
                Did the user modify the data in their cloud?"
            );
        }

        // Try adding the current version to the approved list
        match self.approved.entry(cur_version.clone()) {
            // Already approved; quick check that measurements match
            Entry::Occupied(occupied) => {
                let cur_version = occupied.key();
                let occ_measurement = occupied.get();
                ensure!(
                    *occ_measurement == cur_measurement,
                    "Measurement mismatch for {cur_version}: 
                    expected current {cur_measurement}, found {occ_measurement}"
                );
            }
            // First time approving this version; add to the approved list
            Entry::Vacant(vacant) => {
                let cur_version = vacant.key();
                info!(%user_pk, "Approving version {cur_version}");
                vacant.insert(cur_measurement);
                updated = true;
            }
        }

        // Revoke any versions contained in YANKED_NODE_VERSIONS
        let mut revoked = Vec::with_capacity(1);
        for yanked_version in YANKED_NODE_VERSIONS {
            let yanked_version = semver::Version::parse(yanked_version)
                .expect("Checked in tests");
            if let Some(yanked_measurement) =
                self.approved.remove(&yanked_version)
            {
                info!(%user_pk, "Yank revocation of version {yanked_version}");
                ensure!(
                    YANKED_NODE_MEASUREMENTS.contains(&yanked_measurement),
                    "Yanked measurement not in `YANKED_NODE_MEASUREMENTS`: \
                    {yanked_measurement}"
                );
                revoked.push((yanked_version.clone(), yanked_measurement));
                updated = true;
            }
        }

        // Ensure that we have at most `MAX_SIZE` entries
        while self.approved.len() > Self::MAX_SIZE {
            let (old_version, old_measurement) =
                self.approved.pop_first().expect("Checked by const_assert");
            info!(%user_pk, "Rolling revocation of version {old_version}");
            revoked.push((old_version, old_measurement));
            updated = true;
        }

        // If the current version is not contained in the list at this point,
        // it was added and immediately removed; it is too old to be approved.
        ensure!(
            self.approved.contains_key(&cur_version),
            "Current version {cur_version} is too old to be approved"
        );

        Ok((updated, revoked))
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::{any, Arbitrary},
        collection,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for ApprovedVersions {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let size_range = 0..=ApprovedVersions::MAX_SIZE;
            let any_approved = collection::btree_map(
                arbitrary::any_semver_version(),
                any::<Measurement>(),
                size_range.clone(),
            );

            any_approved.prop_map(|approved| Self { approved }).boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn versions_serde_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<ApprovedVersions>();
    }

    #[test]
    fn const_versions_parse_as_semver() {
        semver::Version::parse(SEMVER_VERSION).unwrap();
        for yanked in YANKED_NODE_VERSIONS {
            semver::Version::parse(yanked).unwrap();
        }
    }

    /// Nonsensical for the current version to be in [`YANKED_NODE_VERSIONS`].
    /// Yanking the current version should be accompanied with a version bump.
    #[test]
    fn cannot_yank_current_version() {
        assert!(!YANKED_NODE_VERSIONS.contains(&SEMVER_VERSION));
    }
}
