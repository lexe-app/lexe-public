use std::{
    fmt::{Debug, Display},
    str::FromStr,
};

use proptest::{
    arbitrary::{any, Arbitrary},
    prop_assert_eq, proptest,
    strategy::Strategy,
    test_runner::Config,
};
use serde::{de::DeserializeOwned, Serialize};

use crate::ed25519;

/// Quickly create a BCS roundtrip proptest.
///
/// ```ignore
/// bcs_roundtrip_proptest::<BearerAuthRequest>();
/// ```
pub fn bcs_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value1: T)| {
        let bytes1 = bcs::to_bytes(&value1).unwrap();
        bcs_roundtrip_ok(&bytes1, &value1);
    });
}

/// Assert that a `T` value canonically roundtrips to/from BCS.
/// 1. `bcs::to_bytes(expected_value) == expected_bytes`
/// 2. `bcs::from_bytes(expected_bytes) == expected_value`
#[track_caller]
pub fn bcs_roundtrip_ok<T>(expected_bytes: &[u8], expected_value: &T)
where
    T: Debug + PartialEq + Serialize + DeserializeOwned,
{
    let actual_bytes = bcs::to_bytes(expected_value).unwrap();
    if actual_bytes != expected_bytes {
        // print hex-encoded bytes for easier debugging
        assert_eq!(hex::encode(&actual_bytes), hex::encode(expected_bytes));
    }

    let actual_value = bcs::from_bytes::<T>(expected_bytes).unwrap();
    assert_eq!(&actual_value, expected_value);
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
/// json_value_roundtrip_proptest::<BearerAuthRequest>();
/// ```
pub fn json_value_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    json_value_custom(any::<T>(), Config::default());
}

/// Create a [`serde_json::Value`] canonical roundtrip proptest using a custom
/// canonical strategy and custom proptest [`Config`]. Useful for testing
/// foreign types for which we cannot implement [`Arbitrary`], or reducing the
/// number of iterations on proptests that would otherwise take too long.
///
/// ```
/// # use common::test_utils::{arbitrary, roundtrip};
/// # use proptest::test_runner::Config;
/// let config = Config::with_cases(1);
/// roundtrip::json_value_custom(arbitrary::any_raw_tx(), config);
/// ```
pub fn json_value_custom<S, T>(strategy: S, config: Config)
where
    S: Strategy<Value = T>,
    T: PartialEq + Serialize + DeserializeOwned + Debug,
{
    proptest!(config, |(value1 in strategy)| {
        let json_value1 = serde_json::to_value(&value1).unwrap();
        let value2 = serde_json::from_value(json_value1.clone()).unwrap();
        let json_value2 = serde_json::to_value(&value2).unwrap();

        prop_assert_eq!(&value1, &value2);
        prop_assert_eq!(&json_value1, &json_value2);
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
    json_string_custom(any::<T>(), Config::default());
}

/// Create a JSON string roundtrip proptest using a custom canonical strategy
/// and custom proptest [`Config`]. Useful for testing foreign types for which
/// we cannot implement [`Arbitrary`], or reducing the number of iterations on
/// proptests that would otherwise take too long.
///
/// ```
/// # use common::api::user::UserPk;
/// # use common::test_utils::roundtrip;
/// # use proptest::arbitrary::{any, Arbitrary};
/// # use proptest::test_runner::Config;
///
/// let config = Config::with_cases(1);
/// roundtrip::json_string_custom(any::<UserPk>(), config);
/// ```
pub fn json_string_custom<S, T>(strategy: S, config: Config)
where
    S: Strategy<Value = T>,
    T: PartialEq + Serialize + DeserializeOwned + Debug,
{
    proptest!(config, |(value1 in strategy)| {
        let json_value1 = serde_json::to_string(&value1).unwrap();
        let value2 = serde_json::from_str::<T>(&json_value1).unwrap();
        prop_assert_eq!(&value1, &value2);
    });
}

/// Quickly create a roundtrip proptest for some `T` which is url-encodable /
/// querystring serializable.
pub fn query_string_roundtrip_proptest<T>()
where
    T: Arbitrary + PartialEq + Serialize + DeserializeOwned,
{
    proptest!(|(value1: T)| {
        let query_str1 = serde_urlencoded::to_string(&value1).unwrap();
        let value2 = serde_urlencoded::from_str(&query_str1).unwrap();
        let query_str2 = serde_urlencoded::to_string(&value2).unwrap();

        prop_assert_eq!(&value1, &value2);
        prop_assert_eq!(&query_str1, &query_str2);

    });
}

/// Quickly create a roundtrip proptest for some `T` which can be signed.
///
/// # Example
/// ```ignore
/// signed_roundtrip_proptest::<BearerAuthRequest>();
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
    fromstr_display_custom(any::<T>(), Config::default());
}

/// Create a roundtrip proptest for a [`FromStr`] / [`Display`] impl using a
/// custom canonical strategy and custom proptest [`Config`]. Useful for testing
/// foreign types for which we cannot implement [`Arbitrary`], or reducing the
/// number of iterations on proptests that would otherwise take too long.
///
/// ```
/// # use common::test_utils::{arbitrary, roundtrip};
/// # use proptest::test_runner::Config;
/// let config = Config::with_cases(1);
/// roundtrip::fromstr_display_custom(arbitrary::any_outpoint(), config);
/// ```
pub fn fromstr_display_custom<S, T>(strategy: S, config: Config)
where
    S: Strategy<Value = T>,
    T: PartialEq + FromStr + Display + Debug,
    <T as FromStr>::Err: Debug,
{
    proptest!(config, |(value1 in strategy)| {
        let value2 = T::from_str(&value1.to_string()).unwrap();
        prop_assert_eq!(value1, value2)
    });
}

/// Quickly create a roundtrip proptest for both [`FromStr`] and json
/// [`Serialize`] impl, and assert that they're both equivalent, i.e., the
/// serialized json representation is just the display wrapped in double-quotes.
pub fn fromstr_json_string_equiv<T>()
where
    T: Arbitrary + PartialEq + Debug,
    T: FromStr + Display,
    T: Serialize + DeserializeOwned,
    <T as FromStr>::Err: Debug,
{
    fromstr_json_string_equiv_custom(any::<T>(), Config::default())
}

/// Create a roundtrip proptest for both [`FromStr`] and json [`Serialize`]
/// impl, and assert that they're both equivalent, i.e., the serialized json
/// representation is just the display wrapped in double-quotes.
pub fn fromstr_json_string_equiv_custom<S, T>(strategy: S, config: Config)
where
    S: Strategy<Value = T>,
    T: PartialEq + Debug,
    T: FromStr + Display,
    T: Serialize + DeserializeOwned,
    <T as FromStr>::Err: Debug,
{
    proptest!(config, |(value in strategy)| {
        let ser_display = value.to_string();
        let ser_json = serde_json::to_string(&value).unwrap();

        prop_assert_eq!(&format!("\"{ser_display}\""), &ser_json);

        let value_fromstr = T::from_str(&ser_display).unwrap();
        let value_json = serde_json::from_str::<T>(&ser_json).unwrap();

        prop_assert_eq!(&value_fromstr, &value);
        prop_assert_eq!(&value_json, &value);
    });
}

/// Exhaustively check that all enum variants have backwards-compatible
/// JSON serialization.
pub fn json_unit_enum_backwards_compat<T>(expected_ser: &str)
where
    T: Clone + PartialEq + Debug,
    T: Serialize + DeserializeOwned,
    T: strum::VariantArray,
{
    // Make bootstrapping the test easier by defaulting to an empty list.
    let expected_ser = if expected_ser.is_empty() {
        "[]"
    } else {
        expected_ser
    };

    let expected_de = T::VARIANTS.to_vec();
    let actual_ser = serde_json::to_string(&expected_de).unwrap();
    let actual_de = serde_json::from_str::<Vec<T>>(expected_ser).unwrap();

    if actual_ser != expected_ser {
        panic!(
            "\n\
             This enum's JSON serialization has changed or a new variant has \n\
             been added/deleted: \n\
             \n\
                actual_ser: '{actual_ser}' \n\
              expected_ser: '{expected_ser}' \n\
             \n\
             It is not safe to remove or rename a variant, as this breaks \n\
             backwards compatibility! Our service won't be able to read data \n\
             persisted in the past! You will need a data migration to do this \n\
             safely. \n\
             \n\
             However, if you've just added a new variant, then this is OK. Just \n\
             update `expected_ser` as below: \n\
             \n\
             ```\n\
             let expected_ser = r#\"{actual_ser}\"#;\n\
             ```\n\
             "
        );
    }
    assert_eq!(actual_de, expected_de);
}
