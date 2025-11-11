#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// The number of confirmations a tx needs to before we consider it final.
pub(crate) const ONCHAIN_CONFIRMATION_THRESHOLD: u32 = 6;

// --- Onchain send --- //

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
pub enum OnchainSendStatus {
    /// (Pending, not broadcasted) The tx has been created and signed but
    /// hasn't been broadcasted yet.
    //
    // TODO(phlip9): handle the case where we create a new `OnchainSend` but
    // crash before we broadcast the tx.
    Created,

    /// (Pending, zeroconf) The tx has been broadcasted and is awaiting its
    /// first confirmation.
    Broadcasted,
    /// (Pending, zeroconf) We broadcasted a RBF replacement or some other
    /// transaction that spends at least one of this transaction's inputs, with
    /// the intention of confirming the newer transaction.
    ReplacementBroadcasted,

    /// (Pending, 1-5 confs) The tx has at least 1 conf, but no greater than 5.
    PartiallyConfirmed,
    /// (Pending, 1-5 confs) At least one of this tx's inputs has been included
    /// in a different tx which has at least 1 conf, but no greater than 5.
    PartiallyReplaced,

    /// (Finalized-Completed, 6+ confs) The tx has 6 or more confirmations.
    FullyConfirmed,
    /// (Finalized-Failed, 6+ confs) At least one of this tx's inputs has been
    /// spent by a different tx which has between 6 or greater confirmations.
    FullyReplaced,
    /// (Finalized-Failed, zeroconf) All of the following are true:
    ///
    /// - This tx has not received a single confirmation.
    /// - We have not detected a replacement tx spending at least one of this
    ///   tx's inputs with 1 or more confirmations.
    /// - It has been at least 14 days since we first created this transaction.
    ///
    /// 14 days is the default `-mempoolexpiry` value in Bitcoin Core. It is
    /// likely that most nodes will have evicted our transaction from their
    /// mempool by now. There is a small chance that this transaction ends up
    /// getting confirmed, but we'll mark it as failed in our payments manager
    /// and move on, since this isn't security-critical; the user will still
    /// see the successful send reflected in their wallet balance.
    Dropped,
}

// --- Onchain receive --- //

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(Arbitrary, strum::VariantArray))]
pub enum OnchainReceiveStatus {
    /// (Pending, zeroconf) We detected the inbound tx, but it is still
    /// awaiting its first confirmation.
    Zeroconf,

    /// (Pending, 1-5 confs) The tx has at least 1 conf, but no greater than 5.
    PartiallyConfirmed,
    /// (Pending, 1-5 confs) At least one of this tx's inputs has been included
    /// in a different tx which has at least 1 conf, but no greater than 5.
    PartiallyReplaced,

    /// (Finalized-Completed, 6+ confs) The tx has 6 or more confirmations.
    FullyConfirmed,
    /// (Finalized-Failed, 6+ confs) At least one of this tx's inputs has been
    /// spent by a different tx which has between 6 or greater confirmations.
    FullyReplaced,
    /// (Finalized-Failed, zeroconf) All of the following are true:
    ///
    /// - This tx has not received a single confirmation.
    /// - We have not detected a replacement tx spending at least one of this
    ///   tx's inputs with 1 or more confirmations.
    /// - It has been at least 14 days since we first detected this transaction.
    ///
    /// 14 days is the default `-mempoolexpiry` value in Bitcoin Core. It is
    /// likely that most nodes will have evicted our transaction from their
    /// mempool by now. There is a small chance that this transaction ends up
    /// getting confirmed, but we'll mark it as failed in our payments manager
    /// and move on, since this isn't security-critical; the user will still
    /// see the successful receive reflected in their wallet balance.
    Dropped,
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip::json_unit_enum_backwards_compat;

    use super::*;

    #[test]
    fn status_json_backwards_compat() {
        let expected_ser = r#"["created","broadcasted","replacement_broadcasted","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainSendStatus>(expected_ser);

        let expected_ser = r#"["zeroconf","partially_confirmed","partially_replaced","fully_confirmed","fully_replaced","dropped"]"#;
        json_unit_enum_backwards_compat::<OnchainReceiveStatus>(expected_ser);
    }
}
