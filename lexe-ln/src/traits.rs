use std::ops::Deref;
use std::sync::Mutex;

use async_trait::async_trait;
use common::api::vfs::BasicFile;
use common::ln::peer::ChannelPeer;
use lightning::chain::chainmonitor::Persist;
use lightning::util::events::EventHandler;
use lightning::util::ser::Writeable;
use serde::Serialize;

use crate::alias::{
    LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
    NetworkGraphType, ProbabilisticScorerType, SignerType,
};
use crate::payments::Payment;

/// Defines all the persister methods needed in shared Lexe LN logic.
#[async_trait]
pub trait LexeInnerPersister: Persist<SignerType> {
    /// Serialize an impl [`Serialize`] to JSON bytes, encrypt the bytes, and
    /// return the [`BasicFile`] which is (almost) ready to be persisted.
    fn encrypt_json<S: Serialize>(
        &self,
        directory: String,
        filename: String,
        value: &S,
    ) -> BasicFile;

    async fn persist_basic_file(
        &self,
        basic_file: BasicFile,
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

    async fn persist_channel_peer(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()>;

    async fn persist_payments(
        &self,
        _payments: Vec<Payment>,
    ) -> anyhow::Result<()> {
        use std::time::Duration;
        // TODO(max): Remove this default impl and replace with actual persists
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(())
    }
}

/// A 'trait alias' defining all the requirements of a Lexe persister.
pub trait LexePersister:
    Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

impl<PS> LexePersister for PS where
    PS: Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
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
