use common::ln::hashes::LxTxid;
use serde::{Deserialize, Serialize};

// --- Onchain deposits --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainDeposit {
    pub txid: LxTxid,
}

// --- Onchain withdrawals --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainWithdrawal {
    pub txid: LxTxid,
}
