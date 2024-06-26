use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    ops::RangeInclusive,
    time::Duration,
};

use bitcoin::{
    blockdata::{opcodes, script},
    hashes::Hash,
    secp256k1,
    util::address::{self, Payload},
    Address, Network, OutPoint, PackedLockTime, Script, ScriptHash, Sequence,
    Transaction, TxIn, TxOut, Txid, Witness,
};
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
    prop_oneof,
    strategy::{Just, Strategy, ValueTree},
    test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
};
use rand::Rng;
use semver::{BuildMetadata, Prerelease};

use crate::{
    api::NodePk,
    rng::{RngExt, WeakRng},
};

// --- Rust types --- ///

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
    vec(any::<char>(), 0..256).prop_map(String::from_iter)
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
    proptest::option::of(any_string())
}

/// A strategy for simple (i.e. alphanumeric) strings, useful when the contents
/// of a [`String`] aren't the interesting thing to test.
pub fn any_simple_string() -> impl Strategy<Value = String> {
    static RANGES: &[RangeInclusive<char>] = &['0'..='9', 'A'..='Z', 'a'..='z'];

    let any_alphanum_char = proptest::char::ranges(RANGES.into());

    proptest::collection::vec(any_alphanum_char, 0..256)
        .prop_map(String::from_iter)
}

/// An [`Option`] version of [`any_simple_string`].
///
/// The option has a 50% probability of being [`Some`].
pub fn any_option_simple_string() -> impl Strategy<Value = Option<String>> {
    proptest::option::of(any_simple_string())
}

/// An `Arbitrary`-like [`Strategy`] for LDK's [`Hostname`] type. `Hostname` is
/// just a DNS-like string with 1..=255 alphanumeric + '-' + '.' chars.
pub fn any_hostname() -> impl Strategy<Value = Hostname> {
    static RANGES: &[RangeInclusive<char>; 4] = &[
        '0'..='9',
        'A'..='Z',
        'a'..='z',
        // This range conveniently contains only these two chars.
        '-'..='.',
    ];

    let any_valid_char = proptest::char::ranges(RANGES.into());

    proptest::collection::vec(any_valid_char, 1..256)
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
    proptest::option::of(any_duration())
}

// --- Bitcoin types --- //

/// An `Arbitrary`-like [`Strategy`] for [`bitcoin::PublicKey`]s.
pub fn any_bitcoin_pubkey() -> impl Strategy<Value = bitcoin::PublicKey> {
    any::<NodePk>()
        .prop_map(secp256k1::PublicKey::from)
        .prop_map(|inner| bitcoin::PublicKey {
            compressed: true,
            inner,
        })
}

/// An `Arbitrary`-like [`Strategy`] for [`bitcoin::XOnlyPublicKey`]s.
pub fn any_x_only_pubkey() -> impl Strategy<Value = bitcoin::XOnlyPublicKey> {
    any::<NodePk>()
        .prop_map(secp256k1::PublicKey::from)
        .prop_map(bitcoin::XOnlyPublicKey::from)
}

/// An `Arbitrary`-like [`Strategy`] for bitcoin [opcode]s.
///
/// [opcode]: opcodes::All
pub fn any_opcode() -> impl Strategy<Value = opcodes::All> {
    any::<u8>().prop_map(opcodes::All::from)
}

/// An `Arbitrary`-like [`Strategy`] for Bitcoin [`Script`]s.
pub fn any_script() -> impl Strategy<Value = Script> {
    #[derive(Clone, Debug)]
    enum PushOp {
        Int(i64),
        ScriptInt(i64),
        Slice(Vec<u8>),
        Key(bitcoin::PublicKey),
        XOnlyPublicKey(bitcoin::XOnlyPublicKey),
        Opcode(opcodes::All),
        OpVerify,
    }

    impl PushOp {
        fn do_push(&self, builder: script::Builder) -> script::Builder {
            match self {
                Self::Int(i) => builder.push_int(*i),
                Self::ScriptInt(i) => builder.push_scriptint(*i),
                Self::Slice(data) => builder.push_slice(data.as_slice()),
                Self::Key(pubkey) => builder.push_key(pubkey),
                Self::XOnlyPublicKey(x_only_pubkey) =>
                    builder.push_x_only_key(x_only_pubkey),
                Self::Opcode(opcode) => builder.push_opcode(*opcode),
                Self::OpVerify => builder.push_verify(),
            }
        }
    }

    // Limit Vec<u8>s to 8 bytes
    let any_vec_u8 = vec(any::<u8>(), 0..=8);

    let any_push_op = prop_oneof![
        any::<i64>().prop_map(PushOp::Int),
        any::<i64>().prop_map(PushOp::ScriptInt),
        any_vec_u8.prop_map(PushOp::Slice),
        any_bitcoin_pubkey().prop_map(PushOp::Key),
        any_x_only_pubkey().prop_map(PushOp::XOnlyPublicKey),
        any_opcode().prop_map(PushOp::Opcode),
        Just(PushOp::OpVerify),
    ];

    // Include anywhere from 0 to 8 instructions in the script
    vec(any_push_op, 0..=8).prop_map(|vec_of_push_ops| {
        let mut builder = script::Builder::new();
        for push_op in vec_of_push_ops {
            builder = push_op.do_push(builder);
        }
        builder.into_script()
    })
}

/// An `Arbitrary`-like [`Strategy`] for a [`Witness`].
pub fn any_witness() -> impl Strategy<Value = Witness> {
    // The `Vec<Vec<u8>>`s from any::<Vec<u8>>() are too big,
    // so we limit to 8x8 = 64 bytes.
    let any_vec_u8 = vec(any::<u8>(), 0..=8);
    let any_vec_vec_u8 = vec(any_vec_u8, 0..=8);
    any_vec_vec_u8.prop_map(Witness::from_vec)
}

/// An `Arbitrary`-like [`Strategy`] for a [`Sequence`].
pub fn any_sequence() -> impl Strategy<Value = Sequence> {
    any::<u32>().prop_map(Sequence)
}

/// An `Arbitrary`-like [`Strategy`] for a [`TxIn`].
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

/// An `Arbitrary`-like [`Strategy`] for a [`TxOut`].
pub fn any_txout() -> impl Strategy<Value = TxOut> {
    (any::<u64>(), any_script()).prop_map(|(value, script_pubkey)| TxOut {
        value,
        script_pubkey,
    })
}

/// An `Arbitrary`-like [`Strategy`] for a raw [`Transaction`].
pub fn any_raw_tx() -> impl Strategy<Value = Transaction> {
    let any_lock_time = any::<u32>().prop_map(PackedLockTime);
    // Txns include anywhere from 1 to 2 inputs / outputs
    let any_vec_of_txins = vec(any_txin(), 1..=2);
    let any_vec_of_txouts = vec(any_txout(), 1..=2);
    (any_lock_time, any_vec_of_txins, any_vec_of_txouts).prop_map(
        |(lock_time, input, output)| Transaction {
            version: 1,
            lock_time,
            input,
            output,
        },
    )
}

/// An `Arbitrary`-like [`Strategy`] for a [`Txid`].
///
/// NOTE that it is often preferred to generate a [`Transaction`] first, and
/// then get the [`Txid`] via [`Transaction::txid`].
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
        .prop_map(bitcoin::hashes::sha256d::Hash::from_inner)
        .prop_map(Txid::from_hash)
        .no_shrink()
    // */
}

/// An `Arbitrary`-like [`Strategy`] for a [`OutPoint`].
pub fn any_outpoint() -> impl Strategy<Value = OutPoint> {
    (any_txid(), any::<u32>()).prop_map(|(txid, vout)| OutPoint { txid, vout })
}

pub fn any_script_hash() -> impl Strategy<Value = ScriptHash> {
    any::<[u8; 20]>()
        .prop_map(|hash| ScriptHash::from_slice(&hash).unwrap())
        .no_shrink()
}

pub fn any_mainnet_address() -> impl Strategy<Value = Address> {
    const NET: Network = Network::Bitcoin;

    prop_oneof![
        // P2PKH
        any_bitcoin_pubkey().prop_map(|pk| Address::p2pkh(&pk, NET)),
        // P2SH / P2WSH / P2SHWSH / P2SHWPKH
        any_script_hash().prop_map(|sh| Address {
            payload: address::Payload::ScriptHash(sh),
            network: NET,
        }),
        // P2WPKH
        any_bitcoin_pubkey().prop_map(|pk| Address::p2wpkh(&pk, NET).unwrap()),
        // P2WSH
        any_script().prop_map(|script| Address::p2wsh(&script, NET)),
        // TODO(phlip9): taproot
    ]
}

/// An `Arbitrary`-like [`Strategy`] for [`semver::Version`]s.
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

/// An `Arbitrary`-like [`Strategy`] for [`chrono::DateTime<Utc>`].
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

/// An `Arbitrary`-like [`Strategy`] for a lightning invoice on-chain
/// [`Fallback`] address.
pub fn any_onchain_fallback() -> impl Strategy<Value = Fallback> {
    any_mainnet_address().prop_map(|address| match address.payload {
        Payload::WitnessProgram { version, program } =>
            Fallback::SegWitProgram { version, program },
        Payload::PubkeyHash(pkh) => Fallback::PubKeyHash(pkh),
        Payload::ScriptHash(sh) => Fallback::ScriptHash(sh),
    })
}

/// An `Arbitrary`-like [`Strategy`] for a lightning invoice [`RouteHint`].
/// Invoice [`RouteHint`]s don't include HTLC min/max msat amounts.
pub fn any_invoice_route_hint() -> impl Strategy<Value = RouteHint> {
    vec(any_invoice_route_hint_hop(), 0..=2).prop_map(RouteHint)
}

/// An `Arbitrary`-like [`Strategy`] for a lightning invoice [`RouteHintHop`].
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
    rng: &mut WeakRng,
    strategy: S,
) -> T {
    GenValueIter::new(rng, strategy).next().unwrap()
}

/// Generate an unlimited values from a [`proptest`] [`Strategy`]. Avoid all the
/// proptest macro junk. Useful for generating sample data. Produces more varied
/// data than just running [`gen_value`] in a loop.
pub fn gen_value_iter<T, S: Strategy<Value = T>>(
    rng: &mut WeakRng,
    strategy: S,
) -> GenValueIter<T, S> {
    GenValueIter::new(rng, strategy)
}

/// An [`Iterator`] that generates values of type `T`, according to a
/// [`proptest`] [`Strategy`].
pub struct GenValueIter<T, S: Strategy<Value = T>> {
    rng: WeakRng,
    strategy: S,
    proptest_runner: TestRunner,
}

impl<T, S: Strategy<Value = T>> GenValueIter<T, S> {
    fn new(rng: &mut WeakRng, strategy: S) -> Self {
        // Extract this to save on some code bloat.
        fn make_proptest_runner(rng: &mut WeakRng) -> TestRunner {
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
        let simplify_iters = self.rng.gen_range(0..128);
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
}
