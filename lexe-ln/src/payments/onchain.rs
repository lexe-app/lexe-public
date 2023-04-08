use common::ln::hashes::LxTxid;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

// --- Onchain deposits --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainDeposit {
    pub txid: LxTxid,
}

// --- Onchain withdrawals --- //

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct OnchainWithdrawal {
    pub txid: LxTxid,
}
