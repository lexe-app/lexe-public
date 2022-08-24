//! Shared Bitcoin / Lightning Lexe newtypes.
//!
//! ## Guidelines
//!
//! Most types defined in or reexported by the [`bitcoin`] crate already have
//! `Serialize` and `Deserialize` impls that serialize to str or bytes depending
//! on whether `is_human_readable()` is true. Use these impls when possible.
//! Whenever it is required to serialize / deserialize to / from `String`, use
//! the `Display` (`format!("{}", foo)`, `to_string()`) and `FromStr`
//! (`Foo::from_str()`) impls if they are provided (as opposed to `FromHex` /
//! `ToHex`); otherwise, implement `Display` and `FromStr`, perhaps with a
//! serialization round trip test.

pub mod channel;
