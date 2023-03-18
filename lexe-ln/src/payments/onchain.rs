use bitcoin::Txid;
use serde::{Deserialize, Serialize};

// --- Onchain deposits --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainDeposit {
    pub txid: Txid,
}

// --- Onchain withdrawals --- //

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnchainWithdrawal {
    pub txid: Txid,
}
