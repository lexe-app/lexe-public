//! [`serde`] serialize and deserialize helpers for [`Option`] types that should
//! be hex-encoded for human-readable formats and raw-bytes for binary codecs.
//!
//! ## Example:
//!
//! ```rust
//! use common::serde_helpers::hexstr_or_bytes_opt;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo(#[serde(with = "hexstr_or_bytes_opt")] Option<Vec<u8>>);
//! ```

// TODO(phlip9): use `serde_bytes` for more efficient ser/de with binary codecs.
// TODO(phlip9): add `[u8; N]` impls to `serde_bytes`...

use std::{fmt, marker::PhantomData};

use hex::FromHex;
use serde::{de, ser, Deserialize, Deserializer, Serializer};

pub fn serialize<S, T>(
    data: &Option<T>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: ser::Serialize + AsRef<[u8]>,
{
    match data {
        Some(value) =>
            if serializer.is_human_readable() {
                let s = hex::encode(value.as_ref());
                serializer.serialize_str(&s)
            } else {
                value.serialize(serializer)
            },
        None => serializer.serialize_none(),
    }
}

pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: de::Deserialize<'de> + FromHex,
{
    struct HexVisitor<T>(PhantomData<T>);

    impl<T: FromHex> de::Visitor<'_> for HexVisitor<T> {
        type Value = Option<T>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("expecting a hex string or null")
        }

        fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
            T::from_hex(s).map(Some).map_err(de::Error::custom)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
    }

    if deserializer.is_human_readable() {
        deserializer.deserialize_any(HexVisitor(PhantomData))
    } else {
        Option::<T>::deserialize(deserializer)
    }
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use serde::{Deserialize, Serialize};

    use crate::serde_helpers::hexstr_or_bytes_opt;

    // TODO(phlip9): test w/ binary codec

    #[test]
    fn test_hexstr_or_bytes_opt() {
        #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
        struct Foo {
            #[serde(with = "hexstr_or_bytes_opt")]
            a1: Option<[u8; 32]>,
            #[serde(with = "hexstr_or_bytes_opt")]
            a2: Option<[u8; 32]>,

            #[serde(with = "hexstr_or_bytes_opt")]
            b1: Option<Vec<u8>>,
            #[serde(with = "hexstr_or_bytes_opt")]
            b2: Option<Vec<u8>>,

            #[serde(with = "hexstr_or_bytes_opt")]
            c1: Option<Cow<'static, [u8]>>,
            #[serde(with = "hexstr_or_bytes_opt")]
            c2: Option<Cow<'static, [u8]>>,

            d: u64,
        }

        let foo1 = Foo {
            a1: Some([0x42; 32]),
            a2: None,
            b1: Some(vec![1, 2, 5, 6, 9, 0, 0x42]),
            b2: None,
            c1: Some(Cow::Borrowed(b"asdf")),
            c2: None,
            d: 1234,
        };

        let actual = serde_json::to_value(&foo1).unwrap();

        assert_eq!(
            &actual,
            &serde_json::json!({
                "a1": "4242424242424242424242424242424242424242424242424242424242424242",
                "a2": null,
                "b1": "01020506090042",
                "b2": null,
                "c1": "61736466",
                "c2": null,
                "d": 1234,
            })
        );

        let s = serde_json::to_string(&foo1).unwrap();
        let foo2: Foo = serde_json::from_str(&s).unwrap();

        assert_eq!(foo1, foo2);
    }
}
