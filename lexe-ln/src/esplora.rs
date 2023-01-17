use bitcoin::blockdata::transaction::Transaction;
use common::task::LxTask;
use esplora_client::AsyncClient;
use lightning::chain::chaininterface::BroadcasterInterface;
use tracing::{debug, error};

pub struct LexeEsplora(AsyncClient);

impl LexeEsplora {
    pub fn new(inner: AsyncClient) -> Self {
        Self(inner)
    }
}

impl BroadcasterInterface for LexeEsplora {
    fn broadcast_transaction(&self, tx: &Transaction) {
        let esplora_client = self.0.clone();
        let tx = tx.clone();
        let txid = tx.txid();
        let _ = LxTask::spawn(async move {
            match esplora_client.broadcast(&tx).await {
                Ok(_) => debug!("Successfully broadcasted tx {txid}"),
                Err(e) => error!("Could not broadcast tx {txid}: {e:#}"),
            };
        });
    }
}
