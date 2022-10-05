use std::ops::Deref;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use common::ln::peer::ChannelPeer;
use lightning::chain::chainmonitor::Persist;
use lightning::util::events::EventHandler;
use lightning::util::ser::Writeable;

use crate::alias::{
    LexeChannelManagerType, LexePeerManagerType, NetworkGraphType,
    ProbabilisticScorerType, SignerType,
};

/// A trait for converting from a generic `Deref<Target = T>` to `Arc<T>`.
///
/// Requiring `ArcInner<T>` (instead of `Deref<Target = T>`) is required if
/// something downstream of the function requires a conversion to [`Arc`].
// TODO: It should be possible to remove this trait by patching LDK's
// `setup_outbound`, `connect_outbound` to not require Arc<T>
pub trait ArcInner<T>: Deref<Target = T> {
    fn arc_inner(&self) -> Arc<T>;
}

/// Defines all the persister methods needed in shared Lexe LN logic.
#[async_trait]
pub trait LexeInnerPersister: Persist<SignerType> {
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
        _channel_peer: ChannelPeer,
    ) -> anyhow::Result<()>;
}

/// A 'trait alias' defining all the requirements of a Lexe persister.
pub trait LexePersister:
    Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

impl<PERSISTER> LexePersister for PERSISTER where
    PERSISTER:
        Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

/// A 'trait alias' defining all the requirements a Lexe channel manager.
pub trait LexeChannelManager<PERSISTER>:
    Send + Sync + 'static + Deref<Target = LexeChannelManagerType<PERSISTER>>
where
    PERSISTER: LexePersister,
{
}

impl<CHANNEL_MANAGER, PERSISTER> LexeChannelManager<PERSISTER>
    for CHANNEL_MANAGER
where
    CHANNEL_MANAGER: Send
        + Sync
        + 'static
        + Deref<Target = LexeChannelManagerType<PERSISTER>>,
    PERSISTER: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe peer manager.
pub trait LexePeerManager<CHANNEL_MANAGER, PERSISTER>:
    Clone + Send + Sync + 'static + ArcInner<LexePeerManagerType<CHANNEL_MANAGER>>
where
    CHANNEL_MANAGER: LexeChannelManager<PERSISTER>,
    PERSISTER: LexePersister,
{
}

impl<PEER_MANAGER, CHANNEL_MANAGER, PERSISTER>
    LexePeerManager<CHANNEL_MANAGER, PERSISTER> for PEER_MANAGER
where
    PEER_MANAGER: Clone
        + Send
        + Sync
        + 'static
        + ArcInner<LexePeerManagerType<CHANNEL_MANAGER>>,
    CHANNEL_MANAGER: LexeChannelManager<PERSISTER>,
    PERSISTER: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe event handler.
pub trait LexeEventHandler: EventHandler + Send + Sync + 'static {}

impl<EVENT_HANDLER> LexeEventHandler for EVENT_HANDLER where
    EVENT_HANDLER: EventHandler + Send + Sync + 'static
{
}
