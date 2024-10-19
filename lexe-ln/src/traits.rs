use std::{ops::Deref, sync::Mutex};

use async_trait::async_trait;
use common::{
    api::vfs::VfsFile,
    ln::{
        payments::{LxPaymentId, PaymentIndex},
        peer::ChannelPeer,
    },
};
use lightning::{
    chain::chainmonitor::Persist, events::EventHandler, util::ser::Writeable,
};
use serde::Serialize;

use crate::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
        NetworkGraphType, ProbabilisticScorerType, SignerType,
    },
    payments::{
        manager::{CheckedPayment, PersistedPayment},
        Payment,
    },
};

/// Defines all the persister methods needed in shared Lexe LN logic.
#[async_trait]
pub trait LexeInnerPersister: Persist<SignerType> {
    fn encrypt_json(
        &self,
        dirname: impl Into<String>,
        filename: impl Into<String>,
        value: &impl Serialize,
    ) -> VfsFile;

    async fn persist_file(
        &self,
        file: VfsFile,
        retries: usize,
    ) -> anyhow::Result<()>;

    async fn persist_manager<W: Writeable + Send + Sync>(
        &self,
        channel_manager: &W,
    ) -> anyhow::Result<()>;

    async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()>;

    async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()>;

    async fn persist_external_peer(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()>;

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
}

impl<PM, CM, PS> LexePeerManager<CM, PS> for PM
where
    PM: Clone + Send + Sync + 'static + Deref<Target = LexePeerManagerType<CM>>,
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe event handler.
pub trait LexeEventHandler: EventHandler + Send + Sync + 'static {}

impl<EH> LexeEventHandler for EH where EH: EventHandler + Send + Sync + 'static {}
