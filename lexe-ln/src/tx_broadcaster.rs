use std::{collections::HashSet, ops::Deref, sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use lexe_common::api::test_event::TestEvent;
use lexe_std::{const_assert, fmt::DisplayIter};
use lexe_tokio::{DEFAULT_CHANNEL_SIZE, notify_once::NotifyOnce, task::LxTask};
use lightning::chain::chaininterface::{BroadcasterInterface, TransactionType};
use thiserror::Error;
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    oneshot,
};
use tracing::{Instrument, error, info, info_span, warn};

use crate::{
    BoxedAnyhowFuture, TxDisplay,
    esplora::{self, LexeEsplora},
    test_event::TestEventSender,
    wallet::OnchainWallet,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Broadcast error: {0:#}")]
    Broadcast(esplora_client::Error),
    #[error("Other error: {0:#}")]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// Whether this error indicates that the transaction had bad inputs;
    /// i.e. the inputs were missing or already spent.
    /// In such a case we should persist the tx and not broadcast it again.
    pub fn is_spent_or_missing_inputs(&self) -> bool {
        match self {
            Error::Broadcast(esplora_client::Error::HttpResponse {
                message,
                ..
            }) => message.contains("bad-txns-inputs-missingorspent"),
            _ => false,
        }
    }
}

/// Maximum time we'll wait for a response from the broadcaster task.
const BROADCAST_RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);
const_assert!(
    BROADCAST_RESPONSE_TIMEOUT.as_secs()
        > esplora::ESPLORA_REQUEST_TIMEOUT.as_secs()
);

/// The type of the hook to be called just before broadcasting a tx.
type PreBroadcastHook =
    Arc<dyn Fn(&bitcoin::Transaction) -> BoxedAnyhowFuture + Send + Sync>;

struct BroadcastRequest {
    txs: Vec<bitcoin::Transaction>,
    /// The span from which the broadcast was initiated.
    span: tracing::Span,
    responder: oneshot::Sender<Result<(), Error>>,
}

/// A handle to a task responsible for broadcasting transactions.
/// We do this in a task because LDK's [`BroadcasterInterface`] isn't async.
///
/// `TxBroadcaster` is cheaply cloneable and impls
/// `Deref<Target = TxBroadcasterInner>`, allowing it to be passed directly to
/// LDK without an additional `Arc` wrapper.
#[derive(Clone)]
pub struct TxBroadcaster {
    inner: TxBroadcasterInner,
}

/// The inner struct that actually impls [`BroadcasterInterface`].
#[derive(Clone)]
pub struct TxBroadcasterInner {
    sender: mpsc::Sender<BroadcastRequest>,
}

impl TxBroadcaster {
    pub fn start(
        esplora: Arc<LexeEsplora>,
        wallet: OnchainWallet,
        broadcast_hook: Option<PreBroadcastHook>,
        test_event_sender: TestEventSender,
        mut shutdown: NotifyOnce,
    ) -> (Self, LxTask<()>) {
        // Avoid tx/rx idiom here since "transaction" also abbreviates to "tx"
        let (sender, mut receiver) = mpsc::channel(DEFAULT_CHANNEL_SIZE);

        let myself = Self {
            inner: TxBroadcasterInner { sender },
        };

        const SPAN_NAME: &str = "(tx-broadcaster)";
        let task = LxTask::spawn_with_span(
            SPAN_NAME,
            info_span!(SPAN_NAME),
            async move {
                loop {
                    let request = tokio::select! {
                        Some(req) = receiver.recv() => req,
                        () = shutdown.recv() => return,
                    };

                    let do_broadcast_fut = {
                        let span = request.span.clone();
                        // Instrument this call with the caller's span.
                        Self::do_broadcast(
                            &esplora,
                            &wallet,
                            broadcast_hook.clone(),
                            request,
                            &test_event_sender,
                        )
                        .instrument(span)
                    };

                    tokio::select! {
                        () = do_broadcast_fut => (),
                        () = shutdown.recv() => return,
                    }
                }
            },
        );

        (myself, task)
    }

    /// Queues a single transaction for broadcast and waits on the result.
    pub async fn broadcast_transaction(
        &self,
        tx: bitcoin::Transaction,
    ) -> Result<(), Error> {
        let (responder, receiver) = oneshot::channel();
        let span = tracing::Span::current();
        let request = BroadcastRequest {
            txs: vec![tx],
            span,
            responder,
        };
        self.inner
            .sender
            .try_send(request)
            .context("Couldn't queue tx for broadcast")?;

        match tokio::time::timeout(BROADCAST_RESPONSE_TIMEOUT, receiver).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(e))) => Err(e),
            Ok(Err(_)) => Err(Error::Other(anyhow!("Sender dropped"))),
            Err(_) => Err(Error::Other(anyhow!(
                "Timed out waiting for broadcast result"
            ))),
        }
    }

    #[tracing::instrument(skip_all, name = "(broadcast)")]
    async fn do_broadcast(
        esplora: &LexeEsplora,
        wallet: &OnchainWallet,
        broadcast_hook: Option<PreBroadcastHook>,
        req: BroadcastRequest,
        test_event_sender: &TestEventSender,
    ) {
        // Package relay requires transactions to be topologically sorted, with
        // parents before children.
        let txs = helpers::toposort_tx_package(req.txs);

        let tx_infos = DisplayIter(txs.iter().map(TxDisplay));
        info!("Broadcasting tx(s): {tx_infos}");

        // Notify the broadcast hook and actually broadcast the tx(s)
        let result =
            Self::do_broadcast_inner(esplora, broadcast_hook, &txs).await;

        match &result {
            Ok(()) => {
                info!("Successfully broadcasted tx(s): {tx_infos}");
                // TODO(phlip9): apply whole package at once, only 1 test event
                for tx in txs {
                    // Apply each transaction to BDK so we don't double spend
                    // its inputs.
                    wallet.transaction_broadcasted(tx);
                    test_event_sender.send(TestEvent::TxBroadcasted);
                }
            }
            Err(err) => warn!("Error broadcasting tx(s): {err:#}, {tx_infos}"),
        }

        // Send the result back to the caller.
        let _ = req.responder.send(result);
    }

    async fn do_broadcast_inner(
        esplora: &LexeEsplora,
        broadcast_hook: Option<PreBroadcastHook>,
        txs: &[bitcoin::Transaction],
    ) -> Result<(), Error> {
        // Run the pre-broadcast hook if one exists.
        // TODO(phlip9): refactor broadcast hook so it gets tx broadcast package
        if let Some(hook) = broadcast_hook {
            for tx in txs {
                hook(tx).await.context("Pre-broadcast hook failed")?;
            }
        }

        match txs {
            [] => Err(Error::Other(anyhow!(
                "Cannot broadcast an empty transaction list"
            ))),

            // Simple 1-tx broadcast
            [tx] => esplora
                .client()
                .broadcast(tx)
                .await
                .map(|_txid| ())
                .map_err(Error::Broadcast),

            // Package-relay broadcast
            package => {
                let maxfeerate = None;
                let maxburnamount = None;
                let result = esplora
                    .client()
                    .submit_package(package, maxfeerate, maxburnamount)
                    .await
                    .map_err(Error::Broadcast)?;

                let any_tx_errors = result
                    .tx_results
                    .iter()
                    .any(|(_wtxid, tx_result)| tx_result.error.is_some());

                // Check package broadcast errors
                let package_msg = &result.package_msg;
                if package_msg != "success" || any_tx_errors {
                    let tx_errors =
                        result.tx_results.iter().filter_map(|(wtxid, tx)| {
                            // TODO(phlip9): should we use `txid` here?
                            tx.error.as_ref().map(|error| {
                                format!("{{wtxid={wtxid}: {error}}}")
                            })
                        });
                    return Err(Error::Other(anyhow!(
                        "Package rejected: {package_msg}; errors: {}",
                        DisplayIter(tx_errors)
                    )));
                }

                Ok(())
            }
        }
    }
}

impl Deref for TxBroadcaster {
    type Target = TxBroadcasterInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl BroadcasterInterface for TxBroadcasterInner {
    fn broadcast_transactions(
        &self,
        txs: &[(&bitcoin::Transaction, TransactionType)],
    ) {
        // TODO(phlip9): log `TransactionType`s alongside txs

        if txs.is_empty() {
            error!("LDK requested an empty transaction broadcast");
            return;
        }

        let span = tracing::Span::current();
        let txs = txs.iter().map(|(tx, _)| (*tx).clone()).collect();
        let (responder, _) = oneshot::channel();
        let req = BroadcastRequest {
            txs,
            span,
            responder,
        };
        if let Err(error) = self.sender.try_send(req) {
            let txids = match &error {
                TrySendError::Full(req) | TrySendError::Closed(req) =>
                    DisplayIter(
                        req.txs.iter().map(bitcoin::Transaction::compute_txid),
                    ),
            };
            error!(
                "Failed to queue tx(s) for broadcast: {error}, txids={txids}"
            );
        }
    }
}

mod helpers {
    use super::*;

    /// Topological-sort a list of transactions so that parent txs come before
    /// child txs. This is required for package relay.
    ///
    /// See: <https://bitcoincore.org/en/doc/31.0.0/rpc/rawtransactions/submitpackage/>
    pub fn toposort_tx_package(
        mut transactions: Vec<bitcoin::Transaction>,
    ) -> Vec<bitcoin::Transaction> {
        let mut sorted = Vec::with_capacity(transactions.len());
        while !transactions.is_empty() {
            let remaining_txids = transactions
                .iter()
                .map(bitcoin::Transaction::compute_txid)
                .collect::<HashSet<_>>();
            let parent_idx = transactions
                .iter()
                .position(|tx| {
                    tx.input.iter().all(|input| {
                        !remaining_txids.contains(&input.previous_output.txid)
                    })
                })
                .expect("Transaction package cannot contain a cycle");
            sorted.push(transactions.remove(parent_idx));
        }
        sorted
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::{
        Amount, OutPoint, ScriptBuf, Transaction, TxIn, TxOut,
        absolute::LockTime, transaction::Version,
    };

    use super::*;

    #[test]
    fn test_sort_tx_package() {
        let grandparent = transaction_with_marker(1);
        let parent = transaction_spending(&grandparent, 2);
        let child = transaction_spending(&parent, 3);

        let sorted = helpers::toposort_tx_package(vec![
            child.clone(),
            grandparent.clone(),
            parent.clone(),
        ]);
        let sorted_txids = sorted
            .iter()
            .map(Transaction::compute_txid)
            .collect::<Vec<_>>();

        assert_eq!(
            sorted_txids,
            [grandparent, parent, child].map(|tx| tx.compute_txid())
        );
    }

    fn transaction_with_marker(marker: u64) -> Transaction {
        Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: Vec::new(),
            output: vec![TxOut {
                value: Amount::from_sat(marker),
                script_pubkey: ScriptBuf::new(),
            }],
        }
    }

    fn transaction_spending(parent: &Transaction, marker: u64) -> Transaction {
        Transaction {
            input: vec![TxIn {
                previous_output: OutPoint::new(parent.compute_txid(), 0),
                ..Default::default()
            }],
            ..transaction_with_marker(marker)
        }
    }
}
