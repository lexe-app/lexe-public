use std::{
    future::Future,
    ops::Deref,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use common::{
    api::{
        vfs::{Vfs, VfsDirectory, VfsFileId},
        NodePk,
    },
    constants,
    ln::{
        network::LxNetwork,
        payments::{LxPaymentId, PaymentIndex},
    },
};
use lightning::{
    chain::chainmonitor::Persist,
    events::Event,
    routing::{
        gossip::NetworkGraph, scoring::ProbabilisticScoringDecayParameters,
    },
    util::ser::Writeable,
};

use crate::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
        NetworkGraphType, ProbabilisticScorerType, SignerType,
    },
    event::EventExt,
    logger::LexeTracingLogger,
    payments::{
        manager::{CheckedPayment, PersistedPayment},
        Payment,
    },
};

/// Defines all the persister methods needed in shared Lexe LN logic.
#[async_trait]
pub trait LexeInnerPersister: Vfs + Persist<SignerType> {
    // --- Required methods --- //

    async fn read_pending_payments(&self) -> anyhow::Result<Vec<Payment>>;

    async fn read_finalized_payment_ids(
        &self,
    ) -> anyhow::Result<Vec<LxPaymentId>>;

    async fn create_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment>;

    async fn persist_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment>;

    async fn persist_payment_batch(
        &self,
        checked_batch: Vec<CheckedPayment>,
    ) -> anyhow::Result<Vec<PersistedPayment>>;

    async fn get_payment(
        &self,
        index: PaymentIndex,
    ) -> anyhow::Result<Option<Payment>>;

    // --- Provided methods --- //

    async fn read_graph(
        &self,
        network: LxNetwork,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<NetworkGraphType> {
        let file_id = VfsFileId::new(
            constants::SINGLETON_DIRECTORY,
            constants::NETWORK_GRAPH_FILENAME,
        );
        let read_args = logger.clone();
        let network_graph = self
            .read_readableargs(&file_id, read_args)
            .await?
            .unwrap_or_else(|| NetworkGraph::new(network.to_bitcoin(), logger));
        Ok(network_graph)
    }

    async fn read_scorer(
        &self,
        graph: Arc<NetworkGraphType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<ProbabilisticScorerType> {
        let file_id = VfsFileId::new(
            constants::SINGLETON_DIRECTORY,
            constants::SCORER_FILENAME,
        );
        let params = ProbabilisticScoringDecayParameters::default();
        let read_args = (params, graph.clone(), logger.clone());
        let scorer = self
            .read_readableargs(&file_id, read_args)
            .await?
            .unwrap_or_else(|| {
                ProbabilisticScorerType::new(params, graph, logger)
            });
        Ok(scorer)
    }

    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(
            constants::SINGLETON_DIRECTORY,
            constants::CHANNEL_MANAGER_FILENAME,
        );
        let file = self.encrypt_ldk_writeable(file_id, channel_manager);
        self.persist_file(&file, constants::IMPORTANT_PERSIST_RETRIES)
            .await
    }

    async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(
            constants::SINGLETON_DIRECTORY,
            constants::NETWORK_GRAPH_FILENAME,
        );
        let file = self.encrypt_ldk_writeable(file_id, network_graph);
        let retries = 0;
        self.persist_file(&file, retries).await
    }

    async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(
            constants::SINGLETON_DIRECTORY,
            constants::SCORER_FILENAME,
        );
        let file = self.encrypt_ldk_writeable(
            file_id,
            scorer_mutex.lock().unwrap().deref(),
        );
        let retries = 0;
        self.persist_file(&file, retries).await
    }

    async fn read_events(&self) -> anyhow::Result<Vec<Event>> {
        let dir = VfsDirectory::new(constants::EVENTS_DIR);
        let events = self.read_dir_maybereadable(&dir).await?;
        Ok(events)
    }

    async fn persist_event(&self, event: &Event) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(constants::EVENTS_DIR, event.id());
        // Failed event persistence can result in the node shutting down, so try
        // a few extra times. TODO(max): Change back to 1 once we switch to
        // LDK's fallible event handling.
        let retries = 3;
        self.persist_ldk_writeable(file_id, &event, retries).await
    }

    async fn remove_event(&self, event_id: String) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(constants::EVENTS_DIR, event_id);
        self.remove_file(&file_id).await
    }
}

/// A 'trait alias' defining all the requirements of a Lexe persister.
pub trait LexePersister:
    Clone + Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

impl<PS> LexePersister for PS where
    PS: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

/// A 'trait alias' defining all the requirements of a Lexe channel manager.
pub trait LexeChannelManager<PS: LexePersister>:
    Clone + Send + Sync + 'static + Deref<Target = LexeChannelManagerType<PS>>
{
}

impl<CM, PS> LexeChannelManager<PS> for CM
where
    CM: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target = LexeChannelManagerType<PS>>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe chain monitor.
pub trait LexeChainMonitor<PS: LexePersister>:
    Send + Sync + 'static + Deref<Target = LexeChainMonitorType<PS>>
{
}

impl<CM, PS> LexeChainMonitor<PS> for CM
where
    CM: Send + Sync + 'static + Deref<Target = LexeChainMonitorType<PS>>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe peer manager.
pub trait LexePeerManager<CM, PS>:
    Clone + Send + Sync + 'static + Deref<Target = LexePeerManagerType<CM>>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    /// Returns `true` if we're connected to a peer with `node_pk`.
    fn is_connected(&self, node_pk: &NodePk) -> bool {
        self.peer_by_node_id(&node_pk.0).is_some()
    }
}

impl<PM, CM, PS> LexePeerManager<CM, PS> for PM
where
    PM: Clone + Send + Sync + 'static + Deref<Target = LexePeerManagerType<CM>>,
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe event handler.
pub trait LexeEventHandler: Send + Sync + 'static {
    /// Given a LDK [`Event`], get a future which handles it.
    fn get_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = ()> + Send;
}
