//! `serde` serialize and deserialize helpers for types that should be
//! hex-encoded for human-readable formats and raw-bytes for binary codecs.
//!
//! ## Example:
//!
//! ```rust
//! use lexe_serde::hexstr_or_bytes;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Foo(#[serde(with = "hexstr_or_bytes")] Vec<u8>);
//! ```

// TODO(phlip9): use `serde_bytes` for more efficient ser/de with binary codecs.
// TODO(phlip9): add `[u8; N]` impls to `serde_bytes`...

use std::{fmt, marker::PhantomData};

use lexe_hex::hex::{self, FromHex};
use serde_core::{Deserializer, Serializer, de, ser};

// --- #[serde(with = "hexstr_or_bytes")] --- //

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

// --- impl_{ser,deser}_hexstr_or_bytes!(Type) macros --- //

/// A macro_rules-based `#[derive(Serialize, Deserialize)]` for simple
/// new-types. Prefer this in dependency-minimized foundational crates that
/// want to avoid proc-macros.
///
/// Example:
///
/// (before)
///
/// ```ignore
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct PublicKey(#[serde(with = "hexstr_or_bytes")] [u8; 32])
/// ```
///
/// (after)
///
/// ```ignore
/// struct PublicKey([u8; 32]);
///
/// lexe_serde::impl_serde_hexstr_or_bytes!(PublicKey);
/// ```
#[macro_export]
macro_rules! impl_serde_hexstr_or_bytes {
    ($Type:ty) => {
        $crate::impl_deser_hexstr_or_bytes!($Type);
        $crate::impl_ser_hexstr_or_bytes!($Type);
    };
}

/// A macro_rules-based `#[derive(Deserialize)]` for simple new-types. Prefer
/// this in dependency-minimized foundational crates that want to avoid
/// proc-macros.
#[macro_export]
macro_rules! impl_deser_hexstr_or_bytes {
    ($Type:ty) => {
        impl<'de> $crate::serde_core::Deserialize<'de> for $Type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: $crate::serde_core::Deserializer<'de>,
            {
                $crate::hexstr_or_bytes::deserialize(deserializer).map(Self)
            }
        }
    };
}

/// A macro_rules-based `#[derive(Serialize)]` for simple new-types. Prefer this
/// in dependency-minimized foundational crates that want to avoid
/// proc-macros.
#[macro_export]
macro_rules! impl_ser_hexstr_or_bytes {
    ($Type:ty) => {
        impl $crate::serde_core::Serialize for $Type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: $crate::serde_core::Serializer,
            {
                $crate::hexstr_or_bytes::serialize(&self.0, serializer)
            }
        }
    };
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;

    use serde::{Deserialize, Serialize};

    use crate::hexstr_or_bytes;

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
        }

        let foo = Foo {
            a: [0x42; 32],
            b: vec![1, 2, 5, 6, 9, 0, 0x42],
            c: Cow::Borrowed(b"asdf"),
            d: 1234,
        };

        let actual = serde_json::to_value(&foo).unwrap();

        assert_eq!(
            &actual,
            &serde_json::json!({
                "a": "4242424242424242424242424242424242424242424242424242424242424242",
                "b": "01020506090042",
                "c": "61736466",
                "d": 1234,
            })
        );

        let s = serde_json::to_string(&foo).unwrap();
        let foo2: Foo = serde_json::from_str(&s).unwrap();

        assert_eq!(foo, foo2);
    }

    #[test]
    fn test_impl_serde_hexstr_or_bytes() {
        #[derive(Debug, Eq, PartialEq)]
        struct MyKey([u8; 32]);

        impl_serde_hexstr_or_bytes!(MyKey);

        let hex_str = r#""4242424242424242424242424242424242424242424242424242424242424242""#;

        // Deserialize
        let key: MyKey = serde_json::from_str(hex_str).unwrap();
        assert_eq!(key, MyKey([0x42; 32]));

        // Serialize
        let serialized = serde_json::to_string(&key).unwrap();
        assert_eq!(serialized, hex_str);
    }
}
