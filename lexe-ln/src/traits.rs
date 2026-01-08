use std::{future::Future, ops::Deref};

use common::api::user::NodePk;
use lexe_api::vfs::Vfs;
use lexe_tokio::notify_once::NotifyOnce;
use lightning::{
    chain::chainmonitor::Persist,
    events::{Event, ReplayEvent},
    ln::msgs::RoutingMessageHandler,
};

use crate::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
        SignerType,
    },
    event::{EventHandleError, EventId},
    persister::LexePersisterMethods,
};

/// A 'trait alias' defining all the requirements of a Lexe persister.
pub trait LexePersister:
    Clone
    + Send
    + Sync
    + 'static
    + Deref<
        Target: LexePersisterMethods + Vfs + Persist<SignerType> + Send + Sync,
    >
{
}

impl<PS> LexePersister for PS where
    PS: Clone
        + Send
        + Sync
        + 'static
        + Deref<
            Target: LexePersisterMethods
                        + Vfs
                        + Persist<SignerType>
                        + Send
                        + Sync,
        >
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
pub trait LexePeerManager<CM, PS, RMH>:
    Clone + Send + Sync + 'static + Deref<Target = LexePeerManagerType<CM, RMH>>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    // TODO(max): Tried to create a `LexeRoutingMessageHandler` alias for these
    // bounds so the don't propagate everywhere, but couldn't get it to work.
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
{
    /// Returns `true` if we're connected to a peer with `node_pk`.
    fn is_connected(&self, node_pk: &NodePk) -> bool {
        // TODO(max): This LDK fn is O(n) in the # of peers...
        self.peer_by_node_id(&node_pk.0).is_some()
    }
}

impl<PM, CM, PS, RMH> LexePeerManager<CM, PS, RMH> for PM
where
    PM: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target = LexePeerManagerType<CM, RMH>>,
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
{
}

/// A 'trait alias' defining all the requirements of a Lexe event handler.
pub trait LexeEventHandler: Clone + Send + Sync + 'static {
    /// Given a LDK [`Event`], get a future which handles it.
    /// The BGP passes this future to LDK for async event handling.
    fn get_ldk_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> + Send;

    /// Handle an event.
    fn handle_event(
        &self,
        event_id: &EventId,
        event: Event,
    ) -> impl Future<Output = Result<(), EventHandleError>> + Send;

    fn persister(&self) -> &impl LexePersister;
    fn shutdown(&self) -> &NotifyOnce;
}
