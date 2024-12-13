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
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
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

    /// Get this unix timestamp as an [`i64`] in milliseconds from unix epoch.
    #[inline]
    pub fn as_i64(self) -> i64 {
        self.0
    }

    /// Get this unix timestamp as a [`u64`] in milliseconds from unix epoch.
    /// The conversion is infallible as the inner [`i64`] is guaranteed to be
    /// non-negative and in-range.
    pub fn into_u64(self) -> u64 {
        u64::try_from(self.0)
            .expect("The inner value is guaranteed to be non-negative")
    }

    /// Get this unix timestamp as a [`Duration`] from the unix epoch.
    #[inline]
    pub fn into_duration(self) -> Duration {
        Duration::from_millis(self.into_u64())
    }

    #[inline]
    pub fn into_system_time(self) -> SystemTime {
        // This add is infallible -- it doesn't panic even with Self::MAX.
        UNIX_EPOCH + self.into_duration()
    }

    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        let dur_ms = i64::try_from(duration.as_millis()).ok()?;
        let added = self.0.checked_add(dur_ms)?;
        Self::try_from(added).ok()
    }

    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        let dur_ms = i64::try_from(duration.as_millis()).ok()?;
        let subtracted = self.0.checked_sub(dur_ms)?;
        Self::try_from(subtracted).ok()
    }
}

impl From<TimestampMs> for Duration {
    #[inline]
    fn from(t: TimestampMs) -> Self {
        t.into_duration()
    }
}

impl From<TimestampMs> for SystemTime {
    #[inline]
    fn from(t: TimestampMs) -> Self {
        t.into_system_time()
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

/// Attempt to convert an [`i64`] in milliseconds since unix epoch into a
/// [`TimestampMs`].
impl TryFrom<i64> for TimestampMs {
    type Error = Error;
    #[inline]
    fn try_from(ms: i64) -> Result<Self, Self::Error> {
        if ms >= Self::MIN.0 {
            Ok(Self(ms))
        } else {
            Err(Error::Negative)
        }
    }
}

/// Attempt to convert a [`u64`] in milliseconds since unix epoch into a
/// [`TimestampMs`].
impl TryFrom<u64> for TimestampMs {
    type Error = Error;
    #[inline]
    fn try_from(ms: u64) -> Result<Self, Self::Error> {
        Self::try_from(Duration::from_millis(ms))
    }
}

/// Construct a [`TimestampMs`] from a [`u32`]. Useful in tests.
impl From<u32> for TimestampMs {
    #[inline]
    fn from(ms: u32) -> Self {
        Self(i64::from(ms))
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
    use proptest::proptest;

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

    // Value conversions should roundtrip.
    fn assert_conversion_roundtrips(t: TimestampMs) {
        assert_eq!(TimestampMs::try_from(t.as_i64()), Ok(t));
        assert_eq!(TimestampMs::try_from(t.into_u64()), Ok(t));
        assert_eq!(TimestampMs::try_from(t.into_duration()), Ok(t));
        assert_eq!(TimestampMs::try_from(t.into_system_time()), Ok(t));
    }

    #[test]
    fn timestamp_conversions_roundtrip() {
        assert_conversion_roundtrips(TimestampMs::MIN);
        assert_conversion_roundtrips(TimestampMs::MAX);

        proptest!(|(t: TimestampMs)| {
            assert_conversion_roundtrips(t);
        });
    }

    #[test]
    fn timestamp_diff() {
        proptest!(|(ts1: TimestampMs, ts2: TimestampMs)| {
            // Determine which timestamp is lesser/greater
            let (lesser, greater) = if ts1 <= ts2 {
                (ts1, ts2)
            } else {
                (ts2, ts1)
            };

            let diff =
                Duration::from_millis(greater.into_u64() - lesser.into_u64());

            let added = lesser.checked_add(diff).unwrap();
            assert_eq!(added, greater);

            let subtracted = greater.checked_sub(diff).unwrap();
            assert_eq!(subtracted, lesser);
        })
    }
}
