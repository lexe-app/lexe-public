use std::{fmt, str::FromStr};

use bitcoin::{secp256k1, secp256k1::Secp256k1};
use byte_array::ByteArray;
use lexe_std::array;
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
use crate::rng::FastRng;
#[cfg(any(test, feature = "test-utils"))]
use crate::root_seed::RootSeed;
use crate::{
    ed25519::{self, Signable},
    rng::Crng,
    serde_helpers::hexstr_or_bytes,
};

/// A Lexe user, as represented in the DB.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub user_pk: UserPk,
    pub node_pk: NodePk,
}

/// An upgradeable version of [`Option<User>`].
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct MaybeUser {
    pub maybe_user: Option<User>,
}

/// A Lexe user's primary identifier - their `ed25519::PublicKey`.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct UserPk(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// A [`UserPk`] shortened to its first four bytes (8 hex chars).
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Hash, Eq, PartialEq, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ShortUserPk(#[serde(with = "hexstr_or_bytes")] [u8; 4]);

/// Upgradeable API struct for a user pk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct UserPkStruct {
    pub user_pk: UserPk,
}

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
#[derive(RefCast, Serialize, Deserialize)]
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
#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct NodePkProof {
    node_pk: NodePk,
    sig: secp256k1::ecdsa::Signature,
}

/// Upgradeable API struct for a node pk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NodePkStruct {
    pub node_pk: NodePk,
}

#[derive(Debug, Error)]
#[error("invalid node pk proof signature")]
pub struct InvalidNodePkProofSignature;

/// A newtype for the `short_channel_id` (`scid`) used throughout LDK.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct Scid(pub u64);

/// Upgradeable API struct for a scid.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ScidStruct {
    pub scid: Scid,
}

/// Upgradeable API struct for multiple scids.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct Scids {
    pub scids: Vec<Scid>,
}

/// An upgradeable version of [`Option<Scid>`].
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct MaybeScid {
    pub maybe_scid: Option<Scid>,
}

/// Represents an entry in the `user_scid` table.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserScid {
    pub node_pk: NodePk,
    pub scid: Scid,
}

/// Upgradable API struct representing multiple `user_scid` entries.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserScids {
    pub user_scids: Vec<UserScid>,
}

/// A request to get at least `min_scids` [`Scid`]s from the LSP, inclusive
/// of any existing [`Scid`]s. The node requests this from the LSP when it
/// detects that it needs the LSP to generate a few more.
///
/// Example:
/// - Node detects it has 3 [`Scid`]s, but it wants 2 new scids to make 5 total.
/// - Node sets `min_scids` to 5.
/// - LSP sees that the node already has 3, generates 2 new ones, and returns 5.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetNewScidsRequest {
    pub node_pk: NodePk,
    pub min_scids: usize,
}

// --- impl UserPk --- //

impl UserPk {
    pub const fn new(inner: [u8; 32]) -> Self {
        Self(inner)
    }

    pub const fn from_ref(inner: &[u8; 32]) -> &Self {
        const_utils::const_ref_cast(inner)
    }

    pub const fn as_ed25519(&self) -> &ed25519::PublicKey {
        ed25519::PublicKey::from_ref(&self.0)
    }

    pub fn short(&self) -> ShortUserPk {
        ShortUserPk::from(self)
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

byte_array::impl_byte_array!(UserPk, 32);
byte_array::impl_fromstr_from_hexstr!(UserPk);
byte_array::impl_debug_display_as_hex!(UserPk);

// --- impl ShortUserPk --- //

impl ShortUserPk {
    pub const fn new(bytes: [u8; 4]) -> Self {
        Self(bytes)
    }

    /// Whether this [`ShortUserPk`] is a prefix of the given [`UserPk`].
    pub fn is_prefix_of(&self, long: &UserPk) -> bool {
        self.0 == long.0[..4]
    }
}

byte_array::impl_byte_array!(ShortUserPk, 4);
byte_array::impl_fromstr_from_hexstr!(ShortUserPk);
byte_array::impl_debug_display_as_hex!(ShortUserPk);

impl From<&UserPk> for ShortUserPk {
    fn from(long: &UserPk) -> Self {
        (long.0)[..4].try_into().map(Self).unwrap()
    }
}

impl From<ed25519::PublicKey> for UserPk {
    fn from(pk: ed25519::PublicKey) -> Self {
        Self::new(pk.into_inner())
    }
}

// --- impl NodePk --- //

impl NodePk {
    pub fn inner(self) -> secp256k1::PublicKey {
        self.0
    }

    pub fn as_inner(&self) -> &secp256k1::PublicKey {
        &self.0
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, secp256k1::Error> {
        secp256k1::PublicKey::from_slice(bytes).map(Self)
    }

    pub fn from_inner_ref(pk: &secp256k1::PublicKey) -> &Self {
        Self::ref_cast(pk)
    }

    pub fn to_array(&self) -> [u8; 33] {
        self.0.serialize()
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.serialize().to_vec()
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
        any::<FastRng>()
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
        let hash = sha256::digest_many(&[
            &NodePkProof::DOMAIN_SEPARATOR,
            &node_pk_bytes,
        ]);
        secp256k1::Message::from_digest(hash.into_inner())
    }

    /// Given a [`secp256k1::Keypair`], sign a new [`NodePkProof`]
    /// Proof-of-Key-Possession for your key pair.
    pub fn sign<R: Crng>(
        rng: &mut R,
        node_key_pair: &secp256k1::Keypair,
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

    /// Dump this `NodePkProof` to a hex-encoded string. Please don't use this.
    pub fn to_hex_string(&self) -> String {
        hex::encode(&bcs::to_bytes(self).expect("Failed to serialize"))
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
        any::<FastRng>()
            .prop_map(|mut rng| {
                let key_pair =
                    RootSeed::from_rng(&mut rng).derive_node_key_pair(&mut rng);
                NodePkProof::sign(&mut rng, &key_pair)
            })
            .boxed()
    }
}

// --- impl Scid --- //

impl Scid {
    /// Some platforms don't support unsigned ints, but they support [`i64`],
    /// so this can be used to convert to the type suitable for that platform.
    pub fn to_i64(&self) -> i64 {
        let bytes = self.0.to_le_bytes();
        i64::from_le_bytes(bytes)
    }

    /// Some platforms don't support unsigned ints, but they support [`i64`],
    /// so this can be used to convert from the type suitable for that platform.
    pub fn from_i64(bytes_i64: i64) -> Self {
        let bytes = bytes_i64.to_le_bytes();
        Self(u64::from_le_bytes(bytes))
    }
}

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

impl From<i64> for Scid {
    fn from(i: i64) -> Self {
        Self::from_i64(i)
    }
}
impl From<Scid> for i64 {
    fn from(scid: Scid) -> Self {
        scid.to_i64()
    }
}

#[cfg(test)]
mod test {
    use proptest::{prop_assume, proptest};

    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn user_node_pk_ser_examples() {
        let mut rng = FastRng::from_u64(811011698);
        let root_seed = RootSeed::from_rng(&mut rng);
        let user_pk = root_seed.derive_user_pk();
        let node_key_pair = root_seed.derive_node_key_pair(&mut rng);
        let node_pk = NodePk(node_key_pair.public_key());
        let node_pk_proof = NodePkProof::sign(&mut rng, &node_key_pair);

        assert_eq!(
            "52b999003525a3d905f9916eff26cee6625a3976fc25270ce5b3e79aa3c16f45",
            user_pk.to_string()
        );
        assert_eq!(
            "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b",
            node_pk.to_string(),
        );
        assert_eq!(
            "024de9a91aaf32588a7b0bb97ba7fad3db22fcfe62a52bc2b2d389c5fa9d946e1b46304402206f762d23d206f3af2ffa452a71a11bca3df68838408851ab77931d7eb7fa1ef6022057141408428d6885d00ca6ca50e6d702aeab227c1550135be5fce4af4e726736",
            node_pk_proof.to_hex_string(),
        );
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
            mut rng: FastRng,
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
    fn scid_basic() {
        let scid = Scid(69);
        assert_eq!(serde_json::to_string(&scid).unwrap(), "69");
    }

    #[test]
    fn scid_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<Scid>();
        roundtrip::fromstr_display_roundtrip_proptest::<Scid>();
    }

    #[test]
    fn user_scid_roundtrips() {
        roundtrip::json_value_roundtrip_proptest::<UserScid>();
    }

    #[test]
    fn user_pk_struct_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<UserPkStruct>();
    }

    #[test]
    fn node_pk_struct_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<NodePkStruct>();
    }

    #[test]
    fn scid_qs_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<ScidStruct>();
        roundtrip::query_string_roundtrip_proptest::<GetNewScidsRequest>();
    }
}
