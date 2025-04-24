//! This module contains [`Arbitrary`]-like [`Strategy`]s for generating various
//! non-Lexe types.
//!
//! [`Arbitrary`]: proptest::arbitrary::Arbitrary
//! [`Strategy`]: proptest::strategy::Strategy

use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    ops::RangeInclusive,
    time::Duration,
};

use bitcoin::{
    absolute,
    address::NetworkUnchecked,
    blockdata::{script, transaction},
    hashes::{sha256d, Hash},
    script::PushBytesBuf,
    secp256k1, Address, Network, OutPoint, ScriptBuf, ScriptHash, Sequence,
    TxIn, TxOut, Txid, Witness,
};
use bytes::Bytes;
use chrono::Utc;
use lightning::{
    routing::{
        gossip::RoutingFees,
        router::{RouteHint, RouteHintHop},
    },
    util::ser::Hostname,
};
use lightning_invoice::Fallback;
use proptest::{
    arbitrary::any,
    collection::vec,
    option, prop_oneof,
    strategy::{Just, Strategy, ValueTree},
    test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
};
use rand::Rng;
use rust_decimal::Decimal;
use semver::{BuildMetadata, Prerelease};

use crate::{
    api::user::NodePk,
    rng::{FastRng, RngExt},
};

// --- `std` types --- ///

/// Like [`any::<String>()`], but is available inside SGX.
///
/// Generated strings have anywhere from 0 to 256 characters.
///
/// ```
/// use common::test_utils::arbitrary;
/// use proptest_derive::Arbitrary;
///
/// #[derive(Debug, Arbitrary)]
/// struct Foo {
///     #[proptest(strategy = "arbitrary::any_string()")]
///     name: String,
/// }
/// ```
pub fn any_string() -> impl Strategy<Value = String> {
    // Maximum length = 256
    vec(any::<char>(), 0..=256).prop_map(String::from_iter)
}

/// An [`Option`] version of [`any_string`].
///
/// The option has a 50% probability of being [`Some`].
///
/// ```
/// use common::test_utils::arbitrary;
/// use proptest_derive::Arbitrary;
///
/// #[derive(Debug, Arbitrary)]
/// struct MaybeFoo {
///     #[proptest(strategy = "arbitrary::any_option_string()")]
///     maybe_name: Option<String>,
/// }
/// ```
pub fn any_option_string() -> impl Strategy<Value = Option<String>> {
    option::of(any_string())
}

/// A strategy for simple (i.e. alphanumeric) strings, useful when the contents
/// of a [`String`] aren't the interesting thing to test.
pub fn any_simple_string() -> impl Strategy<Value = String> {
    static RANGES: &[RangeInclusive<char>] = &['0'..='9', 'A'..='Z', 'a'..='z'];

    let any_alphanum_char = proptest::char::ranges(RANGES.into());

    vec(any_alphanum_char, 0..=256).prop_map(String::from_iter)
}

/// An [`Option`] version of [`any_simple_string`].
///
/// The option has a 50% probability of being [`Some`].
pub fn any_option_simple_string() -> impl Strategy<Value = Option<String>> {
    option::of(any_simple_string())
}

/// A [`Vec`] version of [`any_simple_string`]. Contains 0-8 strings.
pub fn any_vec_simple_string() -> impl Strategy<Value = Vec<String>> {
    vec(any_simple_string(), 0..=8)
}

/// `Hostname` is a DNS-like string with 1..=255 alphanumeric + '-' + '.' chars.
pub fn any_hostname() -> impl Strategy<Value = Hostname> {
    static RANGES: &[RangeInclusive<char>; 4] = &[
        '0'..='9',
        'A'..='Z',
        'a'..='z',
        // This range conveniently contains only these two chars.
        '-'..='.',
    ];

    let any_valid_char = proptest::char::ranges(RANGES.into());

    vec(any_valid_char, 1..=255)
        .prop_map(String::from_iter)
        .prop_map(|s| Hostname::try_from(s).unwrap())
}

/// An `Arbitrary`-like [`Strategy`] for [`Ipv4Addr`] that works in SGX.
pub fn any_ipv4_addr() -> impl Strategy<Value = Ipv4Addr> {
    any::<[u8; 4]>().prop_map(Ipv4Addr::from)
}

/// An `Arbitrary`-like [`Strategy`] for [`Ipv6Addr`] that works in SGX.
pub fn any_ipv6_addr() -> impl Strategy<Value = Ipv6Addr> {
    any::<[u8; 16]>().prop_map(Ipv6Addr::from)
}

/// An `Arbitrary`-like [`Strategy`] for [`SocketAddr`]s which are guaranteed to
/// roundtrip via the `FromStr` / `Display` impls. Useful when implementing
/// `Arbitrary` for structs that contain a [`SocketAddr`] field and whose
/// `FromStr` / `Display` impls must roundtrip.
// [`SocketAddr`]'s `FromStr` / `Display` impls fail to roundtrip due to the
// IPv6 flowinfo field (which we don't care about) not being represented in
// serialized form. To fix this, we simply always set the flowinfo field to 0.
pub fn any_socket_addr() -> impl Strategy<Value = SocketAddr> {
    // We don't use `any::<SocketAddr>().prop_map(...)` because
    // `any::<SocketAddr>()` is not available inside SGX.
    let any_ipv4 = any_ipv4_addr();
    let any_ipv6 = any_ipv6_addr();
    let any_port = any::<u16>();
    let flowinfo = 0;
    let any_scope_id = any::<u32>();

    let any_sockv4 =
        (any_ipv4, any_port).prop_map(|(ip, port)| SocketAddrV4::new(ip, port));
    let any_sockv6 = (any_ipv6, any_port, any_scope_id).prop_map(
        move |(ip, port, scope_id)| {
            SocketAddrV6::new(ip, port, flowinfo, scope_id)
        },
    );

    prop_oneof! {
        any_sockv4.prop_map(SocketAddr::V4),
        any_sockv6.prop_map(SocketAddr::V6),
    }
}

/// An `Arbitrary`-like [`Strategy`] for [`Duration`]s that works inside SGX.
pub fn any_duration() -> impl Strategy<Value = Duration> {
    (any::<u64>(), any::<u32>())
        .prop_map(|(secs, nanos)| Duration::new(secs, nanos))
}

/// An [`Option`] version of [`any_duration`] that works inside SGX.
pub fn any_option_duration() -> impl Strategy<Value = Option<Duration>> {
    option::of(any_duration())
}

// --- General --- //

pub fn any_bytes() -> impl Strategy<Value = Bytes> {
    any::<Vec<u8>>().prop_map(Bytes::from)
}

/// Does not include prerelease or build metadata components.
pub fn any_semver_version() -> impl Strategy<Value = semver::Version> {
    (0..=u64::MAX, 0..=u64::MAX, 0..=u64::MAX).prop_map(
        |(major, minor, patch)| {
            let pre = Prerelease::EMPTY;
            let build = BuildMetadata::EMPTY;
            semver::Version {
                major,
                minor,
                patch,
                pre,
                build,
            }
        },
    )
}

/// Does not include leap seconds.
pub fn any_chrono_datetime() -> impl Strategy<Value = chrono::DateTime<Utc>> {
    let min_utc_secs = chrono::DateTime::<Utc>::MIN_UTC.timestamp();
    let max_utc_secs = chrono::DateTime::<Utc>::MAX_UTC.timestamp();
    let secs_range = min_utc_secs..max_utc_secs;
    let nanos_range = 0..1_000_000_000u32;
    (secs_range, nanos_range)
        .prop_filter_map("Invalid chrono::DateTime<Utc>", |(secs, nanos)| {
            chrono::DateTime::from_timestamp(secs, nanos)
        })
}

pub fn any_decimal() -> impl Strategy<Value = Decimal> {
    (
        any::<u32>(),
        any::<u32>(),
        any::<u32>(),
        any::<bool>(),
        // Scale must be between 0 and 28 (inclusive).
        0u32..=28,
    )
        .prop_map(|(lo, mid, hi, negative, scale)| {
            Decimal::from_parts(lo, mid, hi, negative, scale)
        })
}

// --- Bitcoin types --- //

pub fn any_network() -> impl Strategy<Value = bitcoin::Network> {
    prop_oneof![
        Just(bitcoin::Network::Bitcoin),
        Just(bitcoin::Network::Testnet),
        Just(bitcoin::Network::Signet),
        Just(bitcoin::Network::Regtest),
    ]
}

pub fn any_amount() -> impl Strategy<Value = bitcoin::Amount> {
    any::<u64>().prop_map(bitcoin::Amount::from_sat)
}

pub fn any_secp256k1_pubkey() -> impl Strategy<Value = secp256k1::PublicKey> {
    any::<NodePk>().prop_map(|node_pk| node_pk.0)
}

pub fn any_bitcoin_pubkey() -> impl Strategy<Value = bitcoin::PublicKey> {
    any_secp256k1_pubkey().prop_map(|inner| bitcoin::PublicKey {
        compressed: true,
        inner,
    })
}

pub fn any_compressed_pubkey(
) -> impl Strategy<Value = bitcoin::CompressedPublicKey> {
    any_secp256k1_pubkey().prop_map(bitcoin::CompressedPublicKey)
}

pub fn any_x_only_pubkey() -> impl Strategy<Value = bitcoin::key::XOnlyPublicKey>
{
    any::<NodePk>()
        .prop_map(secp256k1::PublicKey::from)
        .prop_map(bitcoin::key::XOnlyPublicKey::from)
}

pub fn any_opcode() -> impl Strategy<Value = bitcoin::Opcode> {
    any::<u8>().prop_map(bitcoin::Opcode::from)
}

pub fn any_script() -> impl Strategy<Value = ScriptBuf> {
    #[derive(Clone, Debug)]
    enum PushOp {
        Int(i64),
        Slice(Vec<u8>),
        Key(bitcoin::PublicKey),
        XOnlyPublicKey(bitcoin::key::XOnlyPublicKey),
        Opcode(bitcoin::Opcode),
        OpVerify,
        LockTime(absolute::LockTime),
        Sequence(Sequence),
    }

    impl PushOp {
        fn push_into(self, builder: script::Builder) -> script::Builder {
            match self {
                Self::Int(i) => builder.push_int(i),
                Self::Slice(data) => builder.push_slice(
                    PushBytesBuf::try_from(data)
                        .expect("Vec contains more than 2^32 bytes?"),
                ),
                Self::Key(pubkey) => builder.push_key(&pubkey),
                Self::XOnlyPublicKey(x_only_pubkey) =>
                    builder.push_x_only_key(&x_only_pubkey),
                Self::Opcode(opcode) => builder.push_opcode(opcode),
                Self::OpVerify => builder.push_verify(),
                Self::LockTime(locktime) => builder.push_lock_time(locktime),
                Self::Sequence(sequence) => builder.push_sequence(sequence),
            }
        }
    }

    let any_slice = vec(any::<u8>(), 0..=32);

    let any_push_op = prop_oneof![
        any::<i64>().prop_map(PushOp::Int),
        any_slice.prop_map(PushOp::Slice),
        any_bitcoin_pubkey().prop_map(PushOp::Key),
        any_x_only_pubkey().prop_map(PushOp::XOnlyPublicKey),
        any_opcode().prop_map(PushOp::Opcode),
        Just(PushOp::OpVerify),
        any_locktime().prop_map(PushOp::LockTime),
        any::<u32>()
            .prop_map(transaction::Sequence)
            .prop_map(PushOp::Sequence),
    ];

    // Include anywhere from 0 to 8 instructions in the script
    vec(any_push_op, 0..=8).prop_map(|vec_of_push_ops| {
        let mut builder = script::Builder::new();
        for push_op in vec_of_push_ops {
            builder = push_op.push_into(builder);
        }
        builder.into_script()
    })
}

pub fn any_witness() -> impl Strategy<Value = Witness> {
    // The `Vec<Vec<u8>>`s from any::<Vec<u8>>() are too big,
    // so we limit to 8x8 = 64 bytes.
    let any_vec_u8 = vec(any::<u8>(), 0..=8);
    let any_vec_vec_u8 = vec(any_vec_u8, 0..=8);
    any_vec_vec_u8.prop_map(|vec_vec| Witness::from_slice(vec_vec.as_slice()))
}

pub fn any_sequence() -> impl Strategy<Value = Sequence> {
    any::<u32>().prop_map(Sequence)
}

pub fn any_locktime() -> impl Strategy<Value = absolute::LockTime> {
    use bitcoin::absolute::{Height, LockTime, Time};
    prop_oneof![
        (Height::MIN.to_consensus_u32()..=Height::MAX.to_consensus_u32())
            .prop_map(|n| LockTime::Blocks(Height::from_consensus(n).unwrap())),
        (Time::MIN.to_consensus_u32()..=Time::MAX.to_consensus_u32())
            .prop_map(|n| LockTime::Seconds(Time::from_consensus(n).unwrap()))
    ]
}

pub fn any_txin() -> impl Strategy<Value = TxIn> {
    (any_outpoint(), any_script(), any_sequence(), any_witness()).prop_map(
        |(previous_output, script_sig, sequence, witness)| TxIn {
            previous_output,
            script_sig,
            sequence,
            witness,
        },
    )
}

pub fn any_txout() -> impl Strategy<Value = TxOut> {
    (any_amount(), any_script()).prop_map(|(value, script_pubkey)| TxOut {
        value,
        script_pubkey,
    })
}

pub fn any_tx_version() -> impl Strategy<Value = transaction::Version> {
    any::<i32>().prop_map(transaction::Version)
}

pub fn any_raw_tx() -> impl Strategy<Value = bitcoin::Transaction> {
    let any_version = any_tx_version();
    let any_lock_time = any_locktime();
    // Txns include anywhere from 1 to 2 inputs / outputs
    let any_vec_of_txins = vec(any_txin(), 1..=2);
    let any_vec_of_txouts = vec(any_txout(), 1..=2);
    (
        any_version,
        any_lock_time,
        any_vec_of_txins,
        any_vec_of_txouts,
    )
        .prop_map(|(version, lock_time, input, output)| {
            bitcoin::Transaction {
                version,
                lock_time,
                input,
                output,
            }
        })
}

/// An `Arbitrary`-like [`Strategy`] for a [`Txid`].
///
/// NOTE that it is often preferred to generate a [`bitcoin::Transaction`]
/// first, and then get the [`Txid`] via [`bitcoin::Transaction::txid`].
pub fn any_txid() -> impl Strategy<Value = Txid> {
    // In order to generate txids which are more likely to shrink() to a value
    // that corresponds with an actual raw transaction, we can generate txids by
    // simply generating raw transactions and computing their txid. However, the
    // following appears to cause stack overflows:

    // any_raw_tx().prop_map(|raw_tx| raw_tx.txid())

    // The below doesn't cause stack overflows, but due to SHA256's collision
    // resistance, the generated txids do not correspond to any tx at all:
    // /*
    any::<[u8; 32]>()
        .prop_map(sha256d::Hash::from_byte_array)
        .prop_map(Txid::from_raw_hash)
        .no_shrink()
    // */
}

pub fn any_outpoint() -> impl Strategy<Value = OutPoint> {
    (any_txid(), any::<u32>()).prop_map(|(txid, vout)| OutPoint { txid, vout })
}

pub fn any_script_hash() -> impl Strategy<Value = ScriptHash> {
    any::<[u8; 20]>()
        .prop_map(|hash| ScriptHash::from_slice(&hash).unwrap())
        .no_shrink()
}

pub fn any_blockhash() -> impl Strategy<Value = bitcoin::BlockHash> {
    any::<[u8; 32]>()
        .prop_map(bitcoin::BlockHash::from_byte_array)
        .no_shrink()
}

pub fn any_mainnet_addr() -> impl Strategy<Value = Address> {
    const NETWORK: Network = Network::Bitcoin;

    prop_oneof![
        // P2PKH
        any_bitcoin_pubkey().prop_map(|pk| Address::p2pkh(pk, NETWORK)),
        // P2SH
        any_script_hash().prop_map(|sh| Address::p2sh_from_hash(sh, NETWORK)),
        // P2WSH
        any_script().prop_map(|script| Address::p2wsh(&script, NETWORK)),
        // P2SHWSH
        any_script().prop_map(|script| Address::p2shwsh(&script, NETWORK)),
        // P2SHWPKH
        any_compressed_pubkey().prop_map(|pk| Address::p2shwpkh(&pk, NETWORK)),
        // P2WPKH
        any_compressed_pubkey().prop_map(|pk| Address::p2wpkh(&pk, NETWORK)),
        // TODO(phlip9): taproot
    ]
}

pub fn any_mainnet_addr_unchecked(
) -> impl Strategy<Value = Address<NetworkUnchecked>> {
    // TODO(max): Upstream an `Address::into_unchecked` to avoid clone
    any_mainnet_addr().prop_map(|addr| addr.as_unchecked().clone())
}

/// Generate an on-chain confirmations value that's in a reasonable range more
/// frequently.
pub fn any_tx_confs() -> impl Strategy<Value = u32> {
    prop_oneof![
        3 => Just(0_u32),
        3 => 1_u32..=12,
        3 => 13_u32..=1008,
        1 => 1009_u32..=u32::MAX,
    ]
}

// --- LDK types --- //

pub fn any_onchain_fallback() -> impl Strategy<Value = Fallback> {
    any_mainnet_addr().prop_filter_map(
        "Invalid bitcoin::address::Address",
        |address| {
            if let Some(pkh) = address.pubkey_hash() {
                return Some(Fallback::PubKeyHash(pkh));
            }
            if let Some(sh) = address.script_hash() {
                return Some(Fallback::ScriptHash(sh));
            }
            if let Some(wp) = address.witness_program() {
                let version = wp.version();
                // TODO(max): Ideally can just get owned PushBytesBuf to
                // avoid allocation here, can contribute this upstream
                let program_bytes_buf = wp.program().to_owned();
                let program = Vec::<u8>::from(program_bytes_buf);
                return Some(Fallback::SegWitProgram { version, program });
            }
            None
        },
    )
}

/// Invoice [`RouteHint`]s don't include HTLC min/max msat amounts.
pub fn any_invoice_route_hint() -> impl Strategy<Value = RouteHint> {
    vec(any_invoice_route_hint_hop(), 0..=2).prop_map(RouteHint)
}

/// Invoice [`RouteHintHop`]s don't include HTLC min/max msat amounts.
pub fn any_invoice_route_hint_hop() -> impl Strategy<Value = RouteHintHop> {
    let src_node_id = any::<NodePk>();
    let scid = any::<u64>();
    let base_msat = any::<u32>();
    let proportional_millionths = any::<u32>();
    let cltv_expiry_delta = any::<u16>();
    // NOTE: BOLT11 invoice route hint hops don't include the HTLC min/max sat
    // amounts.
    // See: <https://github.com/lightningdevkit/rust-lightning/blob/806b7f0e312c59c87fd628fb71e7c4a77a39645a/lightning-invoice/src/de.rs#L615-L616>

    (
        src_node_id,
        scid,
        base_msat,
        proportional_millionths,
        cltv_expiry_delta,
    )
        .prop_map(
            |(
                src_node_id,
                scid,
                base_msat,
                proportional_millionths,
                cltv_expiry_delta,
            )| RouteHintHop {
                src_node_id: src_node_id.0,
                short_channel_id: scid,
                fees: RoutingFees {
                    base_msat,
                    proportional_millionths,
                },
                cltv_expiry_delta,
                htlc_minimum_msat: None,
                htlc_maximum_msat: None,
            },
        )
}

// --- Generate values directly from a [`proptest`] [`Strategy`] --- //

/// Generate a single value from a [`proptest`] [`Strategy`]. Avoid all the
/// proptest macro junk. Useful for generating sample data.
pub fn gen_value<T, S: Strategy<Value = T>>(
    rng: &mut FastRng,
    strategy: S,
) -> T {
    GenValueIter::new(rng, strategy).next().unwrap()
}

/// Generate a list of values from a [`proptest`] [`Strategy`]. Avoid all the
/// proptest macro junk. Useful for generating sample data.
pub fn gen_values<T, S: Strategy<Value = T>>(
    rng: &mut FastRng,
    strategy: S,
    n: usize,
) -> Vec<T> {
    GenValueIter::new(rng, strategy).take(n).collect()
}

/// Generate an unlimited values from a [`proptest`] [`Strategy`]. Avoid all the
/// proptest macro junk. Useful for generating sample data. Produces more varied
/// data than just running [`gen_value`] in a loop.
pub fn gen_value_iter<T, S: Strategy<Value = T>>(
    rng: &mut FastRng,
    strategy: S,
) -> GenValueIter<T, S> {
    GenValueIter::new(rng, strategy)
}

/// An [`Iterator`] that generates values of type `T`, according to a
/// [`proptest`] [`Strategy`].
pub struct GenValueIter<T, S: Strategy<Value = T>> {
    rng: FastRng,
    strategy: S,
    proptest_runner: TestRunner,
}

impl<T, S: Strategy<Value = T>> GenValueIter<T, S> {
    fn new(rng: &mut FastRng, strategy: S) -> Self {
        // Extract this to save on some code bloat.
        fn make_proptest_runner(rng: &mut FastRng) -> TestRunner {
            let seed = rng.gen_bytes::<32>();
            let test_rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed);
            TestRunner::new_with_rng(Config::default(), test_rng)
        }
        let proptest_runner = make_proptest_runner(rng);
        Self {
            rng: rng.clone(),
            strategy,
            proptest_runner,
        }
    }
}

impl<T, S: Strategy<Value = T>> Iterator for GenValueIter<T, S> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut value_tree = self
            .strategy
            .new_tree(&mut self.proptest_runner)
            .expect("Failed to build ValueTree from Strategy");

        // Call `simplify` a bit to get some more interesting data.
        // NOTE: `complicate` doesn't do what you think it does -- it's more
        // like "undo" for the previous, successful `simplify` call.
        let simplify_iters = self.rng.gen_range(0..4);
        for _ in 0..simplify_iters {
            // `simplify` returns `false` if there's no more simplification to
            // do.
            if !value_tree.simplify() {
                break;
            }
        }
        Some(value_tree.current())
    }
}

#[cfg(test)]
mod test {
    use bitcoin::consensus::{Decodable, Encodable};
    use proptest::{prop_assert_eq, proptest};

    use super::*;
    use crate::test_utils::roundtrip;

    /// Test that the flowinfo workaround used by [`any_socket_addr`] works
    #[test]
    fn socket_addr_roundtrip() {
        let config = Config::with_cases(16);
        roundtrip::fromstr_display_custom(any_socket_addr(), config);
    }

    /// Test [`any_chrono_datetime`] doesn't reject too much.
    #[test]
    fn chrono_datetime_roundtrip() {
        let config = Config::with_cases(1024);
        roundtrip::fromstr_display_custom(any_chrono_datetime(), config);
    }

    /// Test that the [`bitcoin::Transaction`] consensus encoding roundtrips.
    #[test]
    fn bitcoin_consensus_encode_roundtrip() {
        proptest!(|(tx1 in any_raw_tx())| {
            let mut data = Vec::new();
            tx1.consensus_encode(&mut data).unwrap();
            let tx2 =
                bitcoin::Transaction::consensus_decode(&mut data.as_slice())
                    .unwrap();
            prop_assert_eq!(tx1, tx2)
        });
    }
}
