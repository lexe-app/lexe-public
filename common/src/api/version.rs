#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::enclave::Measurement;
#[cfg(test)]
use crate::test_utils::arbitrary;

/// Upgradeable API struct for a measurement.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct MeasurementStruct {
    pub measurement: Measurement,
}

/// The semver version and measurement of a node release.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct NodeRelease {
    /// e.g. "0.1.0", "0.0.0-dev.1"
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_semver_version()"))]
    pub version: semver::Version,
    pub measurement: Measurement,
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
