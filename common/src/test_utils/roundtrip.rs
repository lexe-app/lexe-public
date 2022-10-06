use std::fmt::{Debug, Display};
use std::str::FromStr;

use proptest::arbitrary::Arbitrary;
use proptest::{prop_assert_eq, proptest};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::ed25519;

/// Quickly create a serde roundtrip proptest.
///
/// ```ignore
/// serde_roundtrip_proptest::<UserAuthRequest>();
/// ```
#[cfg_attr(target_env = "sgx", allow(dead_code))]
pub fn serde_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value1: T)| {
        // BCS: non-human readable
        let bcs_value1 = bcs::to_bytes(&value1).unwrap();
        let value2 = bcs::from_bytes::<T>(&bcs_value1).unwrap();
        let bcs_value2 = bcs::to_bytes(&value2).unwrap();
        prop_assert_eq!(&value1, &value2);
        prop_assert_eq!(&bcs_value1, &bcs_value2);

        // JSON: human readable
        let json_value1 = serde_json::to_string(&value1).unwrap();
        let value2 = serde_json::from_str::<T>(&json_value1).unwrap();
        let json_value2 = serde_json::to_string(&value2).unwrap();
        prop_assert_eq!(&value1, &value2);
        prop_assert_eq!(&json_value1, &json_value2);
    });
}

/// Quickly create a roundtrip proptest for some `T` which can be signed.
///
/// # Example
/// ```ignore
/// signed_roundtrip_proptest::<UserAuthRequest>();
/// ```
#[cfg_attr(target_env = "sgx", allow(dead_code))]
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
#[cfg_attr(target_env = "sgx", allow(dead_code))]
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
