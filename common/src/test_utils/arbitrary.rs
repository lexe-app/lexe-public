use std::str::FromStr;

use bitcoin::blockdata::{opcodes, script};
use bitcoin::hashes::sha256d;
use bitcoin::{
    secp256k1, OutPoint, PackedLockTime, Script, Sequence, Transaction, TxIn,
    TxOut, Txid, Witness,
};
use proptest::arbitrary::any;
use proptest::strategy::{BoxedStrategy, Just, Strategy};
use proptest::{collection, prop_oneof};

use crate::api::NodePk;
use crate::hex;

/// An `Arbitrary`-like [`Strategy`] for [`bitcoin::PublicKey`]s.
pub fn any_bitcoin_pubkey() -> BoxedStrategy<bitcoin::PublicKey> {
    any::<NodePk>()
        .prop_map(secp256k1::PublicKey::from)
        .prop_map(|inner| bitcoin::PublicKey {
            compressed: true,
            inner,
        })
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for [`bitcoin::XOnlyPublicKey`]s.
pub fn any_x_only_pubkey() -> BoxedStrategy<bitcoin::XOnlyPublicKey> {
    any::<NodePk>()
        .prop_map(secp256k1::PublicKey::from)
        .prop_map(bitcoin::XOnlyPublicKey::from)
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for bitcoin [opcode]s.
///
/// [opcode]: opcodes::All
pub fn any_opcode() -> BoxedStrategy<opcodes::All> {
    any::<u8>().prop_map(opcodes::All::from).boxed()
}

/// An `Arbitrary`-like [`Strategy`] for Bitcoin [`Script`]s.
pub fn any_script() -> BoxedStrategy<Script> {
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
                Self::XOnlyPublicKey(x_only_pubkey) => {
                    builder.push_x_only_key(x_only_pubkey)
                }
                Self::Opcode(opcode) => builder.push_opcode(*opcode),
                Self::OpVerify => builder.push_verify(),
            }
        }
    }

    let any_push_op = prop_oneof![
        any::<i64>().prop_map(PushOp::Int),
        any::<i64>().prop_map(PushOp::ScriptInt),
        any::<Vec<u8>>().prop_map(PushOp::Slice),
        any_bitcoin_pubkey().prop_map(PushOp::Key),
        any_x_only_pubkey().prop_map(PushOp::XOnlyPublicKey),
        any_opcode().prop_map(PushOp::Opcode),
        Just(PushOp::OpVerify),
    ];

    // Include anywhere from 0 to 32 instructions in the script
    collection::vec(any_push_op, 0..=32)
        .prop_map(|vec_of_push_ops| {
            let mut builder = script::Builder::new();
            for push_op in vec_of_push_ops {
                builder = push_op.do_push(builder);
            }
            builder.into_script()
        })
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`Witness`].
pub fn any_witness() -> BoxedStrategy<Witness> {
    any::<Vec<Vec<u8>>>().prop_map(Witness::from_vec).boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`Sequence`].
pub fn any_sequence() -> BoxedStrategy<Sequence> {
    any::<u32>().prop_map(Sequence).boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`TxIn`].
pub fn any_txin() -> BoxedStrategy<TxIn> {
    (any_outpoint(), any_script(), any_sequence(), any_witness())
        .prop_map(|(previous_output, script_sig, sequence, witness)| TxIn {
            previous_output,
            script_sig,
            sequence,
            witness,
        })
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`TxOut`].
pub fn any_txout() -> BoxedStrategy<TxOut> {
    (any::<u64>(), any_script())
        .prop_map(|(value, script_pubkey)| TxOut {
            value,
            script_pubkey,
        })
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a raw [`Transaction`].
pub fn any_raw_tx() -> BoxedStrategy<Transaction> {
    let any_lock_time = any::<u32>().prop_map(PackedLockTime);
    // Txns include anywhere from 0 to 10 inputs / outputs
    let any_vec_of_txins = collection::vec(any_txin(), 0..=10);
    let any_vec_of_txouts = collection::vec(any_txout(), 0..=10);
    (any_lock_time, any_vec_of_txins, any_vec_of_txouts)
        .prop_map(|(lock_time, input, output)| Transaction {
            version: 1,
            lock_time,
            input,
            output,
        })
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`Txid`].
///
/// NOTE that it is often preferred to generate a [`Transaction`] first, and
/// then get the [`Txid`] via [`Transaction::txid`].
pub fn any_txid() -> BoxedStrategy<Txid> {
    any::<[u8; 32]>()
        .prop_map(|array| hex::encode(&array))
        .prop_map(|hex_str| sha256d::Hash::from_str(hex_str.as_str()).unwrap())
        .prop_map(Txid::from_hash)
        .boxed()
}

/// An `Arbitrary`-like [`Strategy`] for a [`OutPoint`].
pub fn any_outpoint() -> BoxedStrategy<OutPoint> {
    (any_txid(), any::<u32>())
        .prop_map(|(txid, vout)| OutPoint { txid, vout })
        .boxed()
}
