#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::enclave::Measurement;
#[cfg(test)]
use crate::test_utils::arbitrary;

/// The measurement of the latest available release, and its semver version.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct LatestRelease {
    pub measurement: Measurement,
    /// e.g. "0.1.0", "0.0.0-dev.1"
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub version: String,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn latest_release_roundtrip() {
        roundtrip::json_value_canonical_proptest::<LatestRelease>();
    }
}
