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

/// API-upgradeable struct for a [`BTreeSet<NodeEnclave>`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct CurrentEnclaves {
    /// All current node enclaves.
    /// TODO(maurice): remove rename after v0.8.7 is gone.
    #[serde(rename = "releases", alias = "enclaves")]
    pub enclaves: BTreeSet<NodeEnclave>,
}

impl CurrentEnclaves {
    /// Returns the latest (most recent) node enclave, if any.
    pub fn latest(&self) -> Option<&NodeEnclave> {
        self.enclaves.last()
    }
}

/// The subset of node enclaves that a specific user needs to provision to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct EnclavesToProvision {
    pub enclaves: BTreeSet<NodeEnclave>,
}

/// The machine_id, semver version and measurement of a node enclave.
///
/// [`Ord`]ered by [`semver::Version`] precedence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct NodeEnclave {
    /// e.g. "0.1.0", "0.0.0-dev.1"
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_semver_version()"))]
    pub version: semver::Version,
    pub measurement: Measurement,
    pub machine_id: MachineId,
}

impl Ord for NodeEnclave {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.version
            .cmp_precedence(&other.version)
            .then_with(|| self.machine_id.cmp(&other.machine_id))
    }
}

impl PartialOrd for NodeEnclave {
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
    fn node_enclave_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<NodeEnclave>();
    }
}
