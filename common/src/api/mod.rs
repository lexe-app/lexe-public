use std::fmt::{self, Display};
use std::str::FromStr;

use bitcoin::secp256k1;
#[cfg(any(test, feature = "test-utils"))]
use proptest::arbitrary::{any, Arbitrary};
#[cfg(any(test, feature = "test-utils"))]
use proptest::strategy::{BoxedStrategy, Strategy};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};

use crate::hex::{self, FromHex};
#[cfg(any(test, feature = "test-utils"))]
use crate::rng::SmallRng;
#[cfg(any(test, feature = "test-utils"))]
use crate::root_seed::RootSeed;
use crate::{const_ref_cast, ed25519, hexstr_or_bytes};

/// Authentication and User Signup.
pub mod auth;
/// Data types used in APIs for top level commands.
pub mod command;
/// Traits defining the various REST API interfaces.
pub mod def;
/// Enums for the API errors returned by the various services.
pub mod error;
/// `Port`, `Ports`, `UserPorts`, `RunPorts`, etc.
pub mod ports;
/// Data types specific to provisioning.
pub mod provision;
/// Data types used to serialize / deserialize query strings.
pub mod qs;
/// A client and helpers that enforce common REST semantics across Lexe crates.
pub mod rest;
/// Data types implementing vfs-based node persistence.
pub mod vfs;

#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize, RefCast)]
#[repr(transparent)]
pub struct UserPk(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// A simple wrapper around [`secp256k1::PublicKey`] which allows for
/// `Arbitrary` and other custom impls.
///
/// # Notes
///
/// - We do not represent the inner value as `[u8; 33]` (the output of
///   [`secp256k1::PublicKey::serialize`]) because not all `[u8; 33]`s are valid
///   pubkeys.
/// - We use [`PublicKey`]'s [`Serialize`] / [`Deserialize`] impls because it
///   calls into `secp256k1` which does complicated validation to ensure that
///   [`PublicKey`] is always valid.
/// - We use [`PublicKey`]'s [`FromStr`] / [`Display`] impls for similar
///   reasons. Nevertheless, we still run proptests to check for correctness.
///
/// [`PublicKey`]: secp256k1::PublicKey
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize, RefCast)]
#[repr(transparent)]
pub struct NodePk(pub secp256k1::PublicKey);

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub user_pk: UserPk,
    pub node_pk: NodePk,
}

// --- impl UserPk --- //

impl UserPk {
    pub const fn new(inner: [u8; 32]) -> Self {
        Self(inner)
    }

    pub const fn from_ref(inner: &[u8; 32]) -> &Self {
        const_ref_cast(inner)
    }

    pub fn inner(&self) -> [u8; 32] {
        self.0
    }

    pub const fn as_ed25519(&self) -> &ed25519::PublicKey {
        ed25519::PublicKey::from_ref(&self.0)
    }

    /// Used to quickly construct `UserPk`s for tests.
    pub fn from_u64(v: i64) -> Self {
        // Convert i64 to [u8; 8]
        let bytes = v.to_le_bytes();

        // Fill the first 8 bytes with the i64 bytes
        let mut inner = [0u8; 32];
        inner[0..8].copy_from_slice(&bytes);

        Self(inner)
    }

    /// Used to compare inner `u64` values set during tests
    pub fn to_u64(self) -> u64 {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&self.0[0..8]);
        u64::from_le_bytes(bytes)
    }
}

impl FromStr for UserPk {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self::new)
    }
}

impl From<ed25519::PublicKey> for UserPk {
    fn from(pk: ed25519::PublicKey) -> Self {
        Self::new(pk.into_inner())
    }
}

impl Display for UserPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.0.as_slice()))
    }
}

// --- impl NodePk --- //

impl NodePk {
    pub fn inner(&self) -> secp256k1::PublicKey {
        self.0
    }

    pub fn as_inner(&self) -> &secp256k1::PublicKey {
        &self.0
    }
}

impl FromStr for NodePk {
    type Err = bitcoin::secp256k1::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Delegate the FromStr impl
        secp256k1::PublicKey::from_str(s).map(Self)
    }
}

impl Display for NodePk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Call into secp256k1::PublicKey's Display impl
        write!(f, "{}", self.0)
    }
}

impl From<secp256k1::PublicKey> for NodePk {
    fn from(public_key: secp256k1::PublicKey) -> Self {
        Self(public_key)
    }
}

impl From<NodePk> for secp256k1::PublicKey {
    fn from(node_pk: NodePk) -> secp256k1::PublicKey {
        node_pk.0
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for NodePk {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        any::<SmallRng>()
            .prop_map(|mut rng| {
                RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng)
            })
            .boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn user_pk_consistent() {
        let user_pk1 = UserPk::new(hex::decode_const(
            b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        let user_pk2 = UserPk::new(hex::decode_const(
            b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        assert_eq!(user_pk1, user_pk2);
    }

    #[test]
    fn user_pk_human_readable() {
        roundtrip::fromstr_display_roundtrip_proptest::<UserPk>();
    }

    #[test]
    fn user_pk_json() {
        roundtrip::json_string_roundtrip_proptest::<UserPk>();
    }

    #[test]
    fn node_pk_human_readable() {
        roundtrip::fromstr_display_roundtrip_proptest::<NodePk>();
    }

    #[test]
    fn node_pk_json() {
        roundtrip::json_string_roundtrip_proptest::<NodePk>();
    }
}
