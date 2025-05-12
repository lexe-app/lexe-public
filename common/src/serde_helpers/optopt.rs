//! This serde helper is useful in APIs where we have an "update" resource
//! endpoint, but want the client to be able to specify "no change" for a field
//! that is itself optional.
//!
//! # Example
//!
//! For example, suppose a resource has several fields, one of which is an
//! optional label (`Option<String>`). With `optopt`, we can allow clients to
//! optionally update the label:
//!
//! 1. "no change" to the label (rust: `None`, json: `"{}"`)
//! 2. "clear" the label (rust: `Some(None)`, json: `{"label": null}`)
//! 3. "set" the label (rust: `Some(Some("foo"))`, json: `{"label": "foo"}`)
//!
//! ```rust
//! use common::serde_helpers::optopt::{self, none};
//! use serde::{Deserialize, Serialize};
//! #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
//! struct UpdateRequest {
//!     #[serde(default, skip_serializing_if = "none", with = "optopt")]
//!     label: Option<Option<String>>,
//!
//!     // other fields...
//! }
//! ```

use serde::{
    de::{Deserialize, Deserializer},
    ser::{Serialize, Serializer},
};

/// Shorthand for `Option::is_none` so serde attributes stay on one line.
#[inline]
pub fn none<T>(value: &Option<T>) -> bool {
    value.is_none()
}

/// Deserialize maybe-defined optional value
#[inline]
pub fn deserialize<'de, T, D>(
    deserializer: D,
) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// Serialize maybe-defined optional value
pub fn serialize<S, T>(
    values: &Option<Option<T>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    match values {
        None => {
            debug_assert!(
                false,
                "You forgot a `skip_serializing_if = \"none\"` attribute"
            );
            serializer.serialize_unit()
        }
        Some(None) => serializer.serialize_none(),
        Some(Some(v)) => serializer.serialize_some(&v),
    }
}

#[cfg(test)]
mod test {
    use serde::{Deserialize, Serialize};

    use crate::serde_helpers::optopt::{self, none};

    #[test]
    fn test_json() {
        #[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
        struct Foo {
            #[serde(default, skip_serializing_if = "none", with = "optopt")]
            a: Option<Option<u32>>,
        }

        #[track_caller]
        fn test(foo: &Foo, s: &str) {
            let s2 = serde_json::to_string(foo).unwrap();
            assert_eq!(s2, s);
            let foo2: Foo = serde_json::from_str(s).unwrap();
            assert_eq!(foo, &foo2);
        }

        test(&Foo { a: Some(Some(1)) }, r#"{"a":1}"#);
        test(&Foo { a: Some(None) }, r#"{"a":null}"#);
        test(&Foo { a: None }, r#"{}"#);
    }
}
