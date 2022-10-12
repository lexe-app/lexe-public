use std::fmt::{Debug, Display};
use std::str::FromStr;

use proptest::arbitrary::Arbitrary;
use proptest::{prop_assert_eq, proptest};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ed25519;

/// Quickly create a BCS roundtrip proptest.
///
/// ```ignore
/// bcs_roundtrip_proptest::<UserAuthRequest>();
/// ```
pub fn bcs_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value1: T)| {
        let bcs_value1 = bcs::to_bytes(&value1).unwrap();
        let value2 = bcs::from_bytes::<T>(&bcs_value1).unwrap();
        let bcs_value2 = bcs::to_bytes(&value2).unwrap();
        prop_assert_eq!(&value1, &value2);
        // Serialized form should be canonical too
        prop_assert_eq!(&bcs_value1, &bcs_value2);
    });
}

/// Quickly create a [`serde_json::Value`] canonical roundtrip proptest. This
/// test is useful for dictionary-like types that serialize to/from a JSON
/// object.
///
/// This proptest verifies that `T` semi-canonically roundtrips to/from json,
/// though it uses [`serde_json::Value`] as the serialized representation,
/// rather than the standard json string. We use `Value` since the serialized
/// json string doesn't guarantee that order is preserved when ser/de'ing,
/// whereas the `Value` representation will still compare successfully.
///
/// This semi-canonical roundtrip property is also not guaranteed to be true for
/// all serializable types, since json value serializations are not always
/// canonical, even if our comparison is field order-invariant.
///
/// ```ignore
/// json_value_canonical_proptest::<UserAuthRequest>();
/// ```
pub fn json_value_canonical_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value: T)| {
        let json_value = serde_json::to_value(&value).unwrap();
        let value2 = serde_json::from_value(json_value.clone()).unwrap();
        let json_value2 = serde_json::to_value(&value2).unwrap();

        prop_assert_eq!(&value, &value2);
        prop_assert_eq!(&json_value, &json_value2);
    });
}

/// Quickly create a JSON string roundtrip proptest. This test is useful for
/// simple data types that map to/from a single base JSON type (string, int, ..)
///
/// ```ignore
/// json_string_roundtrip_proptest::<UserPk>();
/// ```
pub fn json_string_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value1: T)| {
        let json_value1 = serde_json::to_string(&value1).unwrap();
        let value2 = serde_json::from_str::<T>(&json_value1).unwrap();
        prop_assert_eq!(&value1, &value2);
    });
}

/// Quickly create a roundtrip proptest for some `T` which can be signed.
///
/// # Example
/// ```ignore
/// signed_roundtrip_proptest::<UserAuthRequest>();
/// ```
pub fn signed_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned + ed25519::Signable,
{
    proptest!(|(seed: [u8; 32], value: T)| {
        let key_pair = ed25519::KeyPair::from_seed(&seed);
        let pubkey = key_pair.public_key();

        let (ser_value, signed_value) = key_pair.sign_struct(&value).unwrap();
        let signed_value2 = pubkey.verify_self_signed_struct(&ser_value).unwrap();
        let (ser_value2, _) = key_pair.sign_struct(signed_value2.inner()).unwrap();

        prop_assert_eq!(signed_value, signed_value2.as_ref());
        prop_assert_eq!(&ser_value, &ser_value2);
    });
}

/// Quickly create a roundtrip proptest for a [`FromStr`] / [`Display`] impl.
///
/// ```ignore
/// fromstr_display_roundtrip_proptest::<NodePk>();
/// ```
pub fn fromstr_display_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + FromStr + Display,
    <T as FromStr>::Err: Debug,
{
    proptest!(|(value1: T)| {
        let value2 = T::from_str(&value1.to_string()).unwrap();
        prop_assert_eq!(value1, value2)
    });
}
