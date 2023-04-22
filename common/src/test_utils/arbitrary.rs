#[cfg(not(target_env = "sgx"))]
use std::net::SocketAddr;

use bitcoin::{
    blockdata::{opcodes, script},
    hashes::Hash,
    secp256k1, OutPoint, PackedLockTime, Script, Sequence, Transaction, TxIn,
    TxOut, Txid, Witness,
};
use proptest::{
    arbitrary::any,
    collection, prop_oneof,
    strategy::{Just, Strategy},
};

use crate::api::NodePk;

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
    proptest::collection::vec(any::<char>(), 0..256)
        .prop_map(|chars| String::from_iter(chars.into_iter()))
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
    proptest::option::weighted(0.5, any_string())
}

/// An `Arbitrary`-like [`Strategy`] for [`SocketAddr`]s which are guaranteed to
/// roundtrip via the `FromStr` / `Display` impls. Useful when implementing
/// `Arbitrary` for structs that contain a [`SocketAddr`] field and whose
/// `FromStr` / `Display` impls must roundtrip.
// [`SocketAddr`]'s `FromStr` / `Display` impls fail to roundtrip due to the
// IPv6 flowinfo field (which we don't care about) not being represented in
// serialized form. To fix this, we simply set the flowinfo field to 0 if we
// detect that the socket address is an IPv6n address.
// TODO(max): Make this available inside SGX too
#[cfg(not(target_env = "sgx"))]
pub fn any_socket_addr() -> impl Strategy<Value = SocketAddr> {
    any::<SocketAddr>().prop_map(|mut addr| {
        if let SocketAddr::V6(inner) = &mut addr {
            inner.set_flowinfo(0);
        }
        addr
    })
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
    let any_vec_u8 = collection::vec(any::<u8>(), 0..=8);

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
    collection::vec(any_push_op, 0..=8).prop_map(|vec_of_push_ops| {
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
    let any_vec_u8 = collection::vec(any::<u8>(), 0..=8);
    let any_vec_vec_u8 = collection::vec(any_vec_u8, 0..=8);
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
    let any_vec_of_txins = collection::vec(any_txin(), 1..=2);
    let any_vec_of_txouts = collection::vec(any_txout(), 1..=2);
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
    // */
}

/// An `Arbitrary`-like [`Strategy`] for a [`OutPoint`].
pub fn any_outpoint() -> impl Strategy<Value = OutPoint> {
    (any_txid(), any::<u32>()).prop_map(|(txid, vout)| OutPoint { txid, vout })
}
