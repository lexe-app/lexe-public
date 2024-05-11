use std::{fmt, str::FromStr};

use bitcoin::{secp256k1, secp256k1::Secp256k1};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{
    arbitrary::{any, Arbitrary},
    strategy::{BoxedStrategy, Strategy},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(any(test, feature = "test-utils"))]
use crate::rng::WeakRng;
#[cfg(any(test, feature = "test-utils"))]
use crate::root_seed::RootSeed;
use crate::{
    array, const_ref_cast,
    ed25519::{self, Signable},
    hex::{self, FromHex},
    hexstr_or_bytes,
    rng::Crng,
    sha256,
};

// TODO(max): Consider extracting these into a `lexe-api` crate: `error`,
// `rest`, `server`, `trace`. Only some Lexe crates actually need these.
/// Authentication and User Signup.
pub mod auth;
/// Data types used in APIs for top level commands.
pub mod command;
/// Traits defining the various REST API interfaces.
pub mod def;
/// Enums for the API errors returned by the various services.
pub mod error;
/// Data types returned from the fiat exchange rate API.
pub mod fiat_rates;
/// API models which don't fit anywhere else.
pub mod models;
/// `Port`, `Ports`, `RunPorts`, etc.
pub mod ports;
/// Data types specific to provisioning.
pub mod provision;
/// Data types used to serialize / deserialize query strings.
pub mod qs;
/// A client and helpers that enforce common REST semantics across Lexe crates.
pub mod rest;
/// Webserver utilities.
pub mod server;
/// API tracing utilities for both client and server.
pub mod trace;
/// Data types implementing vfs-based node persistence.
pub mod vfs;

/// A struct denoting an empty API request or response.
///
/// This type should serialize/deserialize in such a way that we have room to
/// add optional fields in the future without causing old clients to reject the
/// message (backwards-compatible changes).
///
/// Always prefer this type over `()` (unit) to avoid API upgrade hazards. In
/// JSON, unit will only deserialize from `"null"`, meaning we can't add new
/// optional fields without breaking old clients.
///
/// ```rust
/// # use common::api::Empty;
/// assert_eq!("", serde_urlencoded::to_string(&Empty {}).unwrap());
/// assert_eq!("{}", serde_json::to_string(&Empty {}).unwrap());
/// ```
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct Empty {}

#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
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
/// - We use [`PublicKey`]'s [`FromStr`] / [`fmt::Display`] impls for similar
///   reasons. Nevertheless, we still run proptests to check for correctness.
///
/// [`PublicKey`]: secp256k1::PublicKey
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
#[derive(Serialize, Deserialize, RefCast)]
#[repr(transparent)]
pub struct NodePk(pub secp256k1::PublicKey);

/// A Proof-of-Key-Possession for a given [`NodePk`].
///
/// Used to ensure a user's signup request contains a [`NodePk`] actually owned
/// by the user.
///
/// Like the outer [`UserSignupRequest`], this PoP is vulnerable to replay
/// attacks in the general case.
///
/// [`UserSignupRequest`]: crate::api::auth::UserSignupRequest
#[derive(Clone, Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct NodePkProof {
    node_pk: NodePk,
    sig: secp256k1::ecdsa::Signature,
}

#[derive(Debug, Error)]
#[error("invalid node pk proof signature")]
pub struct InvalidNodePkProofSignature;

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub user_pk: UserPk,
    pub node_pk: NodePk,
}

/// A newtype for the `short_channel_id` (`scid`) used throughout LDK.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct Scid(pub u64);

/// Represents an entry in the `node_scid` table.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct NodeScid {
    pub node_pk: NodePk,
    pub scid: Scid,
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

impl fmt::Display for UserPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.0.as_slice()))
    }
}

impl fmt::Debug for UserPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UserPk({self})")
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

impl fmt::Display for NodePk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Call into secp256k1::PublicKey's Display impl
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for NodePk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodePk({self})")
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
        any::<WeakRng>()
            .prop_map(|mut rng| {
                RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng)
            })
            .boxed()
    }
}

// -- impl NodePkProof -- //

impl NodePkProof {
    // msg := H(H(DSV) || node_pk)
    fn message(node_pk: &NodePk) -> secp256k1::Message {
        let node_pk_bytes = node_pk.0.serialize();
        secp256k1::Message::from(sha256::digest_many(&[
            &NodePkProof::DOMAIN_SEPARATOR,
            &node_pk_bytes,
        ]))
    }

    /// Given a [`secp256k1::KeyPair`], sign a new [`NodePkProof`]
    /// Proof-of-Key-Possession for your key pair.
    pub fn sign<R: Crng>(
        rng: &mut R,
        node_key_pair: &secp256k1::KeyPair,
    ) -> Self {
        let node_pk = NodePk::from(node_key_pair.public_key());
        let msg = Self::message(&node_pk);
        let sig = rng
            .gen_secp256k1_ctx_signing()
            .sign_ecdsa(&msg, &node_key_pair.secret_key());

        Self { node_pk, sig }
    }

    /// Verify a [`NodePkProof`], getting the verified [`NodePk`] contained
    /// inside on success.
    pub fn verify(&self) -> Result<&NodePk, InvalidNodePkProofSignature> {
        let msg = Self::message(&self.node_pk);
        Secp256k1::verification_only()
            .verify_ecdsa(&msg, &self.sig, &self.node_pk.0)
            .map(|()| &self.node_pk)
            .map_err(|_| InvalidNodePkProofSignature)
    }
}

impl Signable for NodePkProof {
    const DOMAIN_SEPARATOR: [u8; 32] = array::pad(*b"LEXE-REALM::NodePkProof");
}

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for NodePkProof {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        any::<WeakRng>()
            .prop_map(|mut rng| {
                let key_pair =
                    RootSeed::from_rng(&mut rng).derive_node_key_pair(&mut rng);
                NodePkProof::sign(&mut rng, &key_pair)
            })
            .boxed()
    }
}

// --- impl Scid --- //

impl FromStr for Scid {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u64::from_str(s).map(Self)
    }
}

impl fmt::Display for Scid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod test {
    use proptest::{prop_assume, proptest};

    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn empty_serde() {
        // query string

        assert_eq!("", serde_urlencoded::to_string(&Empty {}).unwrap());

        assert_eq!(Empty {}, serde_urlencoded::from_str::<Empty>("").unwrap(),);
        assert_eq!(
            Empty {},
            serde_urlencoded::from_str::<Empty>("foo=123").unwrap(),
        );

        roundtrip::query_string_roundtrip_proptest::<Empty>();

        // json

        assert_eq!("{}", serde_json::to_string(&Empty {}).unwrap());

        // empty string is not valid json
        serde_json::from_str::<Empty>("").unwrap_err();
        // reject other invalid json
        serde_json::from_str::<Empty>("asdlfki").unwrap_err();

        assert_eq!(Empty {}, serde_json::from_str::<Empty>("{}").unwrap(),);
        assert_eq!(
            Empty {},
            serde_json::from_str::<Empty>(r#"{"foo":123}"#).unwrap(),
        );

        roundtrip::json_string_roundtrip_proptest::<Empty>();
    }

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

    #[test]
    fn node_pk_proof_bcs() {
        roundtrip::bcs_roundtrip_proptest::<NodePkProof>();
    }

    #[test]
    fn node_pk_proofs_verify() {
        let arb_mutation = any::<Vec<u8>>()
            .prop_filter("can't be empty or all zeroes", |m| {
                !m.is_empty() && !m.iter().all(|x| x == &0u8)
            });

        proptest!(|(
            mut rng: WeakRng,
            mut_offset in any::<usize>(),
            mut mutation in arb_mutation,
        )| {
            let node_key_pair = RootSeed::from_rng(&mut rng)
                .derive_node_key_pair(&mut rng);
            let node_pk1 = NodePk::from(node_key_pair.public_key());

            let proof1 = NodePkProof::sign(&mut rng, &node_key_pair);
            let proof2 = NodePkProof::sign(&mut rng, &node_key_pair);

            // signing should be deterministic
            assert_eq!(proof1, proof2);

            // valid proof should always verify
            let node_pk2 = proof1.verify().unwrap();
            assert_eq!(&node_pk1, node_pk2);

            let mut proof_bytes = bcs::to_bytes(&proof1).unwrap();
            // println!("{}", hex::encode(&proof_bytes));

            // mutation must not be idempotent (otherwise the proof won't change
            // and will actually verify).
            mutation.truncate(proof_bytes.len());
            prop_assume!(
                !mutation.is_empty() && !mutation.iter().all(|x| x == &0)
            );

            // xor in the mutation bytes to the proof to modify it. any modified
            // bit should cause the verification to fail.
            for (idx_mut, m) in mutation.into_iter().enumerate() {
                let idx_sig = idx_mut
                    .wrapping_add(mut_offset) % proof_bytes.len();
                proof_bytes[idx_sig] ^= m;
            }

            // mutated proof should always fail to deserialize or verify.
            bcs::from_bytes::<NodePkProof>(&proof_bytes)
                .map_err(anyhow::Error::new)
                .and_then(|proof| {
                    proof.verify()
                        .map(|_| ())
                        .map_err(anyhow::Error::new)
                })
                .unwrap_err();
        });
    }

    #[test]
    fn scid_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<Scid>();
        roundtrip::fromstr_display_roundtrip_proptest::<Scid>();
    }

    #[test]
    fn node_scid_roundtrips() {
        roundtrip::json_value_roundtrip_proptest::<NodeScid>();
    }
}
