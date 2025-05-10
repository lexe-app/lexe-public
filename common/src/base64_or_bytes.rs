//! [`serde`] serialize and deserialize helpers for types that should be
//! base64-encoded for human-readable formats and raw-bytes for binary codecs.
//!
//! ## Example:
//!
//! ```rust
//! use common::base64_or_bytes;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo(#[serde(with = "base64_or_bytes")] Vec<u8>);
//! ```

// TODO(phlip9): use `serde_bytes` for more efficient ser/de with binary codecs.

use std::{borrow::Cow, fmt, marker::PhantomData};

use base64::Engine;
use serde::{de, ser, Deserializer, Serializer};

/// A trait to deserialize something from a base64-encoded string slice.
///
/// Examples:
///
/// ```
/// # use std::borrow::Cow;
/// use common::base64_or_bytes::FromBase64;
/// let s = String::from("gVX5KuLzr9SI4grp0P1mq1ABHCXXleQA/rPIqofhlxE=");
///
/// let vec = <Vec<u8>>::from_base64(&s).unwrap();
/// let cow = <Cow<'_, [u8]>>::from_base64(&s).unwrap();
/// ```
pub trait FromBase64: Sized {
    fn from_base64(s: &str) -> Result<Self, base64::DecodeError>;
}

impl FromBase64 for Vec<u8> {
    fn from_base64(s: &str) -> Result<Self, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(s)
    }
}

impl FromBase64 for bytes::Bytes {
    fn from_base64(s: &str) -> Result<Self, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map(Self::from)
    }
}

impl FromBase64 for Cow<'_, [u8]> {
    fn from_base64(s: &str) -> Result<Self, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map(Cow::Owned)
    }
}

pub fn serialize<S, T>(data: T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: ser::Serialize + AsRef<[u8]>,
{
    if serializer.is_human_readable() {
        let s = base64::engine::general_purpose::STANDARD.encode(data.as_ref());
        serializer.serialize_str(&s)
    } else {
        data.serialize(serializer)
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: de::Deserialize<'de> + FromBase64,
{
    struct Base64Visitor<T>(PhantomData<T>);

    impl<T: FromBase64> de::Visitor<'_> for Base64Visitor<T> {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("expecting base64 string")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            T::from_base64(s).map_err(de::Error::custom)
        }
    }

    if deserializer.is_human_readable() {
        deserializer.deserialize_str(Base64Visitor(PhantomData))
    } else {
        T::deserialize(deserializer)
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use bytes::Bytes;
    use proptest_derive::Arbitrary;
    use serde::{Deserialize, Serialize};

    use crate::{
        base64_or_bytes,
        test_utils::{arbitrary, roundtrip},
    };

    // TODO(phlip9): test w/ binary codec

    #[derive(Debug, Eq, PartialEq, Serialize, Deserialize, Arbitrary)]
    struct Foo {
        #[serde(with = "base64_or_bytes")]
        a: Vec<u8>,

        #[serde(with = "base64_or_bytes")]
        b: Cow<'static, [u8]>,

        #[serde(with = "base64_or_bytes")]
        #[proptest(strategy = "arbitrary::any_bytes()")]
        c: Bytes,
    }

    #[test]
    fn test_base64_or_bytes() {
        let foo = Foo {
            a: vec![1, 2, 5, 6, 9, 0, 0x42],
            b: Cow::Borrowed(b"asdf"),
            c: Bytes::from(vec![5, 4, 3, 2, 1, 0, 0x42]),
        };

        let actual = serde_json::to_value(&foo).unwrap();

        assert_eq!(
            &actual,
            &serde_json::json!({
                "a": "AQIFBgkAQg==",
                "b": "YXNkZg==",
                "c": "BQQDAgEAQg==",
            })
        );

        let s = serde_json::to_string(&foo).unwrap();
        let foo2: Foo = serde_json::from_str(&s).unwrap();

        assert_eq!(foo, foo2);
    }

    #[test]
    fn base64_or_bytes_json_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<Foo>();
    }
}
