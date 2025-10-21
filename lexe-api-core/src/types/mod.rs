#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// `LxInvoice`, a wrapper around LDK's BOLT11 invoice type.
pub mod invoice;
/// `LxOffer`, a wrapper around LDK's BOLT12 offer type.
pub mod offer;
/// Payments types and newtypes.
pub mod payments;
/// `Port`, `Ports`, `RunPorts`, etc, used in the Runner.
pub mod ports;
/// `SealedSeed` and related types and logic.
pub mod sealed_seed;

/// A unique identifier for a user node lease.
// TODO(max): Find a better home for this.
pub type LeaseId = u32;

/// A struct denoting an empty API request or response.
///
/// This type should serialize/deserialize in such a way that we have room to
/// add optional fields in the future without causing old clients to reject the
/// message (backwards-compatible changes).
///
/// Always prefer this type over `()` (unit) to avoid API upgrade hazards. In
/// JSON, unit will only deserialize from `"null"`, meaning we can't add new
/// optional fields without breaking old clients.
///
/// ```rust
/// # use lexe_api_core::types::Empty;
/// assert_eq!("", serde_urlencoded::to_string(&Empty {}).unwrap());
/// assert_eq!("{}", serde_json::to_string(&Empty {}).unwrap());
/// ```
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct Empty {}

/// A serializable wrapper arround [`anyhow::Error`].
///
/// This type is enables serialization/deserialization of
/// [`anyhow::Error`] in API responses, which can't be done directly due to
/// orphan rules.
///
/// Serliazes only the error's display message. Deserialization produces a
/// generic error with the message.
#[derive(Debug)]
pub struct LxError(pub anyhow::Error);

impl Serialize for LxError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for LxError {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(LxError(anyhow::anyhow!(s)))
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn empty_serde() {
        // query string

        assert_eq!("", serde_urlencoded::to_string(&Empty {}).unwrap());

        assert_eq!(Empty {}, serde_urlencoded::from_str::<Empty>("").unwrap(),);
        assert_eq!(
            Empty {},
            serde_urlencoded::from_str::<Empty>("foo=123").unwrap(),
        );

        roundtrip::query_string_roundtrip_proptest::<Empty>();

        // json

        assert_eq!("{}", serde_json::to_string(&Empty {}).unwrap());

        // empty string is not valid json
        serde_json::from_str::<Empty>("").unwrap_err();
        // reject other invalid json
        serde_json::from_str::<Empty>("asdlfki").unwrap_err();

        assert_eq!(Empty {}, serde_json::from_str::<Empty>("{}").unwrap(),);
        assert_eq!(
            Empty {},
            serde_json::from_str::<Empty>(r#"{"foo":123}"#).unwrap(),
        );

        roundtrip::json_string_roundtrip_proptest::<Empty>();
    }
}
