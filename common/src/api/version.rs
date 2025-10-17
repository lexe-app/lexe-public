use std::collections::BTreeSet;

#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::enclave::{MachineId, Measurement};
#[cfg(test)]
use crate::test_utils::arbitrary;

/// Upgradeable API struct for a measurement.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct MeasurementStruct {
    pub measurement: Measurement,
}

/// API-upgradeable struct for a [`BTreeSet<NodeRelease>`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct CurrentReleases {
    /// All current node releases.
    pub releases: BTreeSet<NodeRelease>,
}

impl CurrentReleases {
    /// Returns the latest (most recent) node release, if any.
    pub fn latest(&self) -> Option<&NodeRelease> {
        self.releases.last()
    }
}

/// The machine_id, semver version and measurement of a node release.
///
/// [`Ord`]ered by [`semver::Version`] precedence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct NodeRelease {
    /// e.g. "0.1.0", "0.0.0-dev.1"
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_semver_version()"))]
    pub version: semver::Version,
    pub measurement: Measurement,
    pub machine_id: MachineId,
}

impl Ord for NodeRelease {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.version
            .cmp_precedence(&other.version)
            .then_with(|| self.machine_id.cmp(&other.machine_id))
    }
}

impl PartialOrd for NodeRelease {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn measurement_struct_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<MeasurementStruct>();
    }

    #[test]
    fn node_release_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NodeRelease>();
    }
}
