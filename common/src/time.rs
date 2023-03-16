use std::convert::TryFrom;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{de, Deserialize, Deserializer, Serialize};

/// The number of milliseconds since the [`UNIX_EPOCH`].
///
/// - Internally represented by a non-negative [`i64`] to ease interoperability
///   with some platforms we use which don't support unsigned ints.
/// - Can represent any time from January 1st, 1970 00:00:00.000 UTC to roughly
///   292 million years in the future.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize)]
pub struct TimestampMillis(i64);

impl TimestampMillis {
    /// Creates a new [`TimestampMillis`] from the current [`SystemTime`].
    ///
    /// Panics if the current time is not within bounds.
    pub fn now() -> Self {
        Self::try_from(SystemTime::now()).unwrap()
    }

    /// Returns the contained [`i64`].
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

/// Get a [`SystemTime`] corresponding to this timestamp.
impl From<TimestampMillis> for SystemTime {
    fn from(timestamp: TimestampMillis) -> Self {
        let timestamp_u64 = u64::try_from(timestamp.0)
            .expect("Non-negative invariant was violated");
        let duration_since_epoch = Duration::from_millis(timestamp_u64);
        UNIX_EPOCH + duration_since_epoch
    }
}

/// Attempts to convert a [`SystemTime`] into a [`TimestampMillis`].
///
/// Returns an error if the [`SystemTime`] is not within bounds.
impl TryFrom<SystemTime> for TimestampMillis {
    type Error = anyhow::Error;
    fn try_from(system_time: SystemTime) -> anyhow::Result<Self> {
        system_time
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .map(i64::try_from)
            .map(|res| res.map(Self))
            .context("Current time is before January 1st, 1970")?
            .context("Current time is more than 292 million years past epoch")
    }
}

/// Enforces that the inner [`i64`] is non-negative.
impl<'de> Deserialize<'de> for TimestampMillis {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = i64::deserialize(deserializer)?;
        if value >= 0 {
            Ok(TimestampMillis(value))
        } else {
            Err(de::Error::invalid_value(
                de::Unexpected::Signed(value),
                &"Unix timestamp must be non-negative",
            ))
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::Arbitrary;
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;
    use crate::test_utils::roundtrip;

    impl Arbitrary for TimestampMillis {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (0..i64::MAX).prop_map(Self).boxed()
        }
    }

    #[test]
    fn timestamp_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<TimestampMillis>();
    }

    #[test]
    fn deserialize_enforces_nonnegative() {
        assert_eq!(serde_json::from_str::<TimestampMillis>("42").unwrap().0, 42);
        assert_eq!(serde_json::from_str::<TimestampMillis>("0").unwrap().0, 0);
        assert!(serde_json::from_str::<TimestampMillis>("-42").is_err());
    }
}
