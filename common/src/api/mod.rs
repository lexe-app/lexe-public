#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

// TODO(max): All of these modules should be moved to `lexe_api[_core]`.

/// Authentication and User Signup.
// TODO(max): `error` depends on `auth`
pub mod auth;
/// Data types returned from the fiat exchange rate API.
pub mod fiat_rates;
/// API models which don't fit anywhere else.
pub mod models;
/// Data types specific to provisioning.
pub mod provision;
/// Revocable clients.
pub mod revocable_clients;
/// `TestEvent`.
pub mod test_event;
/// User ID-like types: `User`, `UserPk`, `NodePk`, `Scid`
pub mod user;
/// Data types which relate to node versions: `NodeRelease`, `MeasurementStruct`
pub mod version;

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
/// # use common::api::Empty;
/// assert_eq!("", serde_urlencoded::to_string(&Empty {}).unwrap());
/// assert_eq!("{}", serde_json::to_string(&Empty {}).unwrap());
/// ```
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct Empty {}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

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
