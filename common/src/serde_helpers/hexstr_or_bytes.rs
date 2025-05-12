//! [`serde`] serialize and deserialize helpers for types that should be
//! hex-encoded for human-readable formats and raw-bytes for binary codecs.
//!
//! ## Example:
//!
//! ```rust
//! use common::serde_helpers::hexstr_or_bytes;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo(#[serde(with = "hexstr_or_bytes")] Vec<u8>);
//! ```

// TODO(phlip9): use `serde_bytes` for more efficient ser/de with binary codecs.
// TODO(phlip9): add `[u8; N]` impls to `serde_bytes`...

use std::{fmt, marker::PhantomData};

use hex::FromHex;
use serde::{de, ser, Deserializer, Serializer};

pub fn serialize<S, T>(data: T, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: ser::Serialize + AsRef<[u8]>,
{
    if serializer.is_human_readable() {
        let s = hex::encode(data.as_ref());
        serializer.serialize_str(&s)
    } else {
        data.serialize(serializer)
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: de::Deserialize<'de> + FromHex,
{
    struct HexVisitor<T>(PhantomData<T>);

    impl<T: FromHex> de::Visitor<'_> for HexVisitor<T> {
        type Value = T;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("expecting hex string")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            T::from_hex(s).map_err(de::Error::custom)
        }
    }

    if deserializer.is_human_readable() {
        deserializer.deserialize_str(HexVisitor(PhantomData))
    } else {
        T::deserialize(deserializer)
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use bytes::Bytes;
    use serde::{Deserialize, Serialize};

    use crate::serde_helpers::hexstr_or_bytes;

    // TODO(phlip9): test w/ binary codec

    #[test]
    fn test_hexstr_or_bytes() {
        #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
        struct Foo {
            #[serde(with = "hexstr_or_bytes")]
            a: [u8; 32],

            #[serde(with = "hexstr_or_bytes")]
            b: Vec<u8>,

            #[serde(with = "hexstr_or_bytes")]
            c: Cow<'static, [u8]>,

            d: u64,

            #[serde(with = "hexstr_or_bytes")]
            e: Bytes,
        }

        let foo = Foo {
            a: [0x42; 32],
            b: vec![1, 2, 5, 6, 9, 0, 0x42],
            c: Cow::Borrowed(b"asdf"),
            d: 1234,
            e: Bytes::from(vec![5, 4, 3, 2, 1, 0, 0x42]),
        };

        let actual = serde_json::to_value(&foo).unwrap();

        assert_eq!(
            &actual,
            &serde_json::json!({
                "a": "4242424242424242424242424242424242424242424242424242424242424242",
                "b": "01020506090042",
                "c": "61736466",
                "d": 1234,
                "e": "05040302010042",
            })
        );

        let s = serde_json::to_string(&foo).unwrap();
        let foo2: Foo = serde_json::from_str(&s).unwrap();

        assert_eq!(foo, foo2);
    }
}
