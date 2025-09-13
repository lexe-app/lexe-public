use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Context};
use common::api::test_event::TestEvent;
use lexe_std::const_assert;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask, DEFAULT_CHANNEL_SIZE};
use lightning::chain::chaininterface::BroadcasterInterface;
use thiserror::Error;
use tokio::sync::{
    mpsc::{self, error::TrySendError},
    oneshot,
};
use tracing::{error, info, info_span, warn, Instrument};

use crate::{
    esplora::{self, LexeEsplora},
    test_event::TestEventSender,
    wallet::LexeWallet,
    BoxedAnyhowFuture, TxDisplay,
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
    tx: bitcoin::Transaction,
    /// The span from which the broadcast was initiated.
    span: tracing::Span,
    responder: oneshot::Sender<Result<(), Error>>,
}

/// A handle to a task responsible for broadcasting transactions.
/// We do this in a task because LDK's [`BroadcasterInterface`] isn't async.
pub struct TxBroadcaster {
    sender: mpsc::Sender<BroadcastRequest>,
}

impl TxBroadcaster {
    pub fn start(
        esplora: Arc<LexeEsplora>,
        wallet: LexeWallet,
        broadcast_hook: Option<PreBroadcastHook>,
        test_event_sender: TestEventSender,
        mut shutdown: NotifyOnce,
    ) -> (Arc<Self>, LxTask<()>) {
        // Avoid tx/rx idiom here since "transaction" also abbreviates to "tx"
        let (sender, mut receiver) = mpsc::channel(DEFAULT_CHANNEL_SIZE);

        let myself = Arc::new(Self { sender });

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

    /// Queues a transaction for broadcast and waits on the result.
    pub async fn broadcast_transaction(
        &self,
        tx: bitcoin::Transaction,
    ) -> Result<(), Error> {
        let (responder, receiver) = oneshot::channel();
        let span = tracing::Span::current();
        let request = BroadcastRequest {
            tx,
            span,
            responder,
        };
        self.sender
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
        wallet: &LexeWallet,
        broadcast_hook: Option<PreBroadcastHook>,
        request: BroadcastRequest,
        test_event_sender: &TestEventSender,
    ) {
        // Log some useful information about the transaction.
        let tx = &request.tx;
        let tx_info = TxDisplay(tx);
        info!("Broadcasting transaction: {tx_info}");

        let result =
            Self::do_broadcast_inner(esplora, broadcast_hook, tx).await;

        match &result {
            Ok(()) => {
                info!("Successfully broadcasted tx: {tx_info}");
                // Apply the transaction to BDK so we don't double spend the
                // outputs spent by this tx
                wallet.transaction_broadcasted(request.tx);
                test_event_sender.send(TestEvent::TxBroadcasted);
            }
            Err(e) => warn!("Error broadcasting tx: {e:#}: {tx_info}"),
        }

        // Send the result back to the caller.
        let _ = request.responder.send(result);
    }

    async fn do_broadcast_inner(
        esplora: &LexeEsplora,
        broadcast_hook: Option<PreBroadcastHook>,
        tx: &bitcoin::Transaction,
    ) -> Result<(), Error> {
        // Run the pre-broadcast hook if one exists.
        if let Some(hook) = broadcast_hook {
            let try_future = hook(tx);
            try_future.await.context("Pre-broadcast hook failed")?;
        }

        // Broadcast the transaction.
        esplora
            .client()
            .broadcast(tx)
            .await
            .map_err(Error::Broadcast)
    }
}

impl BroadcasterInterface for TxBroadcaster {
    fn broadcast_transactions(&self, txs: &[&bitcoin::Transaction]) {
        let span = tracing::Span::current();
        for &tx in txs {
            let (responder, _) = oneshot::channel();
            let req = BroadcastRequest {
                tx: tx.clone(),
                span: span.clone(),
                responder,
            };
            let result = self.sender.try_send(req);
            if let Err(error) = result {
                let txid = match &error {
                    TrySendError::Full(req) => req.tx.compute_txid(),
                    TrySendError::Closed(req) => req.tx.compute_txid(),
                };
                error!(%txid, "Failed to queue tx for broadcast: {error}");
            }
        }
    }
}
