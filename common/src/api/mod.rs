use std::fmt::{self, Display};
use std::str::FromStr;

use bitcoin::secp256k1::PublicKey;
#[cfg(test)]
use proptest::arbitrary::{any, Arbitrary};
#[cfg(test)]
use proptest::strategy::{BoxedStrategy, Strategy};
#[cfg(test)]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};

use crate::hex::{self, FromHex};
#[cfg(test)]
use crate::rng::SmallRng;
#[cfg(test)]
use crate::root_seed::RootSeed;
use crate::{const_ref_cast, ed25519, hexstr_or_bytes};

/// Authentication and User Signup.
pub mod auth;
/// Traits defining the various REST API interfaces.
pub mod def;
/// Enums for the API errors returned by the various services.
pub mod error;
/// Minor data types defining what is returned by APIs exposed by the node.
/// Bigger / more fundamental LN types should go under [`crate::ln`].
pub mod node;
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

#[cfg_attr(test, derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize, RefCast)]
#[repr(transparent)]
pub struct UserPk(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

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
    pub fn from_i64(i: i64) -> Self {
        // Convert i64 to [u8; 8]
        let i_bytes = i.to_ne_bytes();

        // Fill the first 8 bytes with the i64 bytes
        let mut inner = [0u8; 32];
        inner[0..8].clone_from_slice(&i_bytes);

        Self(inner)
    }

    /// Used to compare inner `i64` values set during tests
    pub fn to_i64(self) -> i64 {
        let mut i_bytes = [0u8; 8];
        i_bytes.clone_from_slice(&self.0[0..8]);
        i64::from_ne_bytes(i_bytes)
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

/// A simple wrapper around [`PublicKey`] which allows for `Arbitrary` and other
/// custom impls.
///
/// # Notes
///
/// - We do not represent the inner value as `[u8; 33]` (the output of
///   [`PublicKey::serialize`]) because not all `[u8; 33]`s are valid pubkeys.
/// - We use [`PublicKey`]'s [`Serialize`] / [`Deserialize`] impls because it
///   calls into `secp256k1` which does complicated validation to ensure that
///   [`PublicKey`] is always valid.
/// - We use [`PublicKey`]'s [`FromStr`] / [`Display`] impls for similar
///   reasons. Nevertheless, we still run proptests to check for correctness.
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize, RefCast)]
#[repr(transparent)]
pub struct NodePk(pub PublicKey);

impl NodePk {
    pub fn inner(&self) -> PublicKey {
        self.0
    }
}

impl FromStr for NodePk {
    type Err = bitcoin::secp256k1::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Delegate the FromStr impl
        PublicKey::from_str(s).map(Self)
    }
}

impl Display for NodePk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Call into PublicKey's Display impl
        write!(f, "{}", self.0)
    }
}

impl From<PublicKey> for NodePk {
    fn from(public_key: PublicKey) -> Self {
        Self(public_key)
    }
}

impl From<NodePk> for PublicKey {
    fn from(node_pk: NodePk) -> PublicKey {
        node_pk.0
    }
}

#[cfg(test)]
impl Arbitrary for NodePk {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        any::<SmallRng>()
            .prop_map(|mut rng| {
                let root_seed = RootSeed::from_rng(&mut rng);
                let inner = root_seed.derive_node_pk(&mut rng);
                Self(inner)
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
    fn user_pk_bcs() {
        roundtrip::bcs_roundtrip_proptest::<UserPk>();
    }

    #[test]
    fn node_pk_human_readable() {
        roundtrip::fromstr_display_roundtrip_proptest::<NodePk>();
    }

    #[test]
    fn node_pk_bcs() {
        roundtrip::bcs_roundtrip_proptest::<NodePk>();
    }
}
