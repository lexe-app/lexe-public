//! [`serde`] serialize and deserialize helpers for [`Option`] types that should
//! be base64 for human-readable formats and raw-bytes for binary codecs.
//!
//! ## Example:
//!
//! ```rust
//! use common::base64_or_bytes_opt;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo(#[serde(with = "base64_or_bytes_opt")] Option<Vec<u8>>);
//! ```

// TODO(phlip9): use `serde_bytes` for more efficient ser/de with binary codecs.

use std::{fmt, marker::PhantomData};

use base64::Engine;
use serde::{de, ser, Deserialize, Deserializer, Serializer};

use crate::base64_or_bytes::FromBase64;

pub fn serialize<S, T>(
    maybe_data: &Option<T>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: ser::Serialize + AsRef<[u8]>,
{
    match maybe_data {
        Some(ref data) =>
            if serializer.is_human_readable() {
                let s = base64::engine::general_purpose::STANDARD
                    .encode(data.as_ref());
                serializer.serialize_str(&s)
            } else {
                data.serialize(serializer)
            },
        None => serializer.serialize_none(),
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: de::Deserialize<'de> + FromBase64,
{
    struct Base64Visitor<T>(PhantomData<T>);

    impl<T: FromBase64> de::Visitor<'_> for Base64Visitor<T> {
        type Value = Option<T>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("expecting base64 string or null")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            T::from_base64(s).map(Some).map_err(de::Error::custom)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    if deserializer.is_human_readable() {
        deserializer.deserialize_any(Base64Visitor(PhantomData))
    } else {
        Option::<T>::deserialize(deserializer)
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use bytes::Bytes;
    use proptest_derive::Arbitrary;
    use serde::{Deserialize, Serialize};

    use crate::{
        base64_or_bytes_opt,
        test_utils::{arbitrary, roundtrip},
    };

    // TODO(phlip9): test w/ binary codec

    #[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Arbitrary)]
    struct Foo {
        #[serde(with = "base64_or_bytes_opt")]
        a1: Option<Vec<u8>>,
        #[serde(with = "base64_or_bytes_opt")]
        a2: Option<Vec<u8>>,

        #[serde(with = "base64_or_bytes_opt")]
        b1: Option<Cow<'static, [u8]>>,
        #[serde(with = "base64_or_bytes_opt")]
        b2: Option<Cow<'static, [u8]>>,

        #[serde(with = "base64_or_bytes_opt")]
        #[proptest(strategy = "arbitrary::any_option_bytes()")]
        c1: Option<Bytes>,
        #[serde(with = "base64_or_bytes_opt")]
        #[proptest(strategy = "arbitrary::any_option_bytes()")]
        c2: Option<Bytes>,
    }

    #[test]
    fn test_base64_or_bytes_opt() {
        let foo = Foo {
            a1: Some(vec![1, 2, 5, 6, 9, 0, 0x42]),
            a2: None,
            b1: Some(Cow::Borrowed(b"asdf")),
            b2: None,
            c1: Some(Bytes::from(vec![5, 4, 3, 2, 1, 0, 0x42])),
            c2: None,
        };

        let actual = serde_json::to_value(&foo).unwrap();

        assert_eq!(
            &actual,
            &serde_json::json!({
                "a1": "AQIFBgkAQg==",
                "a2": null,
                "b1": "YXNkZg==",
                "b2": null,
                "c1": "BQQDAgEAQg==",
                "c2": null,
            })
        );

        let s = serde_json::to_string(&foo).unwrap();
        let foo2: Foo = serde_json::from_str(&s).unwrap();

        assert_eq!(foo, foo2);
    }

    #[test]
    fn base64_or_bytes_opt_json_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<Foo>();
    }
}
