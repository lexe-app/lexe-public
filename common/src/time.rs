use std::{
    fmt::{self, Display},
    str::FromStr,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{de, Serialize};

/// The number of milliseconds since the [`UNIX_EPOCH`].
///
/// - Internally represented by a non-negative [`i64`] to ease interoperability
///   with some platforms we use which don't support unsigned ints well
///   (Postgres and Dart/Flutter).
/// - Can represent any time from January 1st, 1970 00:00:00.000 UTC to roughly
///   292 million years in the future.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize)]
pub struct TimestampMs(i64);

/// Errors that can occur when attempting to construct a [`TimestampMs`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("timestamp value is negative")]
    Negative,

    #[error("timestamp is more than 292 million years past epoch")]
    TooLarge,

    #[error("timestamp is before January 1st, 1970")]
    BeforeEpoch,

    #[error("failed to parse timestamp: {0}")]
    Parse(#[from] std::num::ParseIntError),
}

impl TimestampMs {
    pub const MIN: Self = TimestampMs(0);
    pub const MAX: Self = TimestampMs(i64::MAX);

    /// Creates a new [`TimestampMs`] from the current [`SystemTime`].
    ///
    /// Panics if the current time is not within bounds.
    pub fn now() -> Self {
        Self::try_from(SystemTime::now()).unwrap()
    }

    /// Returns the contained [`i64`].
    #[inline]
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

/// Get a [`SystemTime`] corresponding to this timestamp.
impl From<TimestampMs> for SystemTime {
    fn from(timestamp: TimestampMs) -> Self {
        let timestamp_u64 = u64::try_from(timestamp.0)
            .expect("Non-negative invariant was violated");
        let duration_since_epoch = Duration::from_millis(timestamp_u64);
        UNIX_EPOCH + duration_since_epoch
    }
}

/// Attempts to convert a [`SystemTime`] into a [`TimestampMs`].
///
/// Returns an error if the [`SystemTime`] is not within bounds.
impl TryFrom<SystemTime> for TimestampMs {
    type Error = Error;
    fn try_from(system_time: SystemTime) -> Result<Self, Self::Error> {
        let duration = system_time
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::BeforeEpoch)?;
        Self::try_from(duration)
    }
}

/// Attempts to convert a [`Duration`] since the UNIX epoch into a
/// [`TimestampMs`].
///
/// Returns an error if the [`Duration`] is too large.
impl TryFrom<Duration> for TimestampMs {
    type Error = Error;
    fn try_from(duration_since_epoch: Duration) -> Result<Self, Self::Error> {
        i64::try_from(duration_since_epoch.as_millis())
            .map(Self)
            .map_err(|_| Error::TooLarge)
    }
}

/// Attempt to convert an [`i64`] into a [`TimestampMs`].
impl TryFrom<i64> for TimestampMs {
    type Error = Error;
    #[inline]
    fn try_from(inner: i64) -> Result<Self, Self::Error> {
        if inner >= Self::MIN.0 {
            Ok(Self(inner))
        } else {
            Err(Error::Negative)
        }
    }
}

/// Construct a [`TimestampMs`] from a [`u32`]. Useful in tests.
impl From<u32> for TimestampMs {
    #[inline]
    fn from(inner: u32) -> Self {
        Self(i64::from(inner))
    }
}

impl FromStr for TimestampMs {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(i64::from_str(s)?)
    }
}

impl Display for TimestampMs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        i64::fmt(&self.0, f)
    }
}

impl<'de> de::Deserialize<'de> for TimestampMs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        i64::deserialize(deserializer)
            .and_then(|x| Self::try_from(x).map_err(de::Error::custom))
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::Arbitrary,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for TimestampMs {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (Self::MIN.0..Self::MAX.0).prop_map(Self).boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn timestamp_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<TimestampMs>();
        roundtrip::json_string_roundtrip_proptest::<TimestampMs>();
    }

    #[test]
    fn deserialize_enforces_nonnegative() {
        // We deserialize from JSON numbers; note that it is NOT e.g. "\"42\""
        assert_eq!(serde_json::from_str::<TimestampMs>("42").unwrap().0, 42);
        assert_eq!(serde_json::from_str::<TimestampMs>("0").unwrap().0, 0);
        assert!(serde_json::from_str::<TimestampMs>("-42").is_err());
    }
}
