use std::{future::Future, ops::Deref, str::FromStr};

use anyhow::Context;
use async_trait::async_trait;
use common::{api::user::NodePk, ln::channel::LxOutPoint};
use lexe_api::{
    types::payments::{LxPaymentId, PaymentIndex},
    vfs::{self, Vfs, VfsDirectory, VfsFileId},
};
use lexe_tokio::notify_once::NotifyOnce;
use lightning::{
    chain::chainmonitor::Persist,
    events::{Event, ReplayEvent},
    ln::msgs::RoutingMessageHandler,
    util::ser::Writeable,
};

use crate::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
        SignerType,
    },
    event::{EventHandleError, EventId},
    payments::{
        Payment,
        manager::{CheckedPayment, PersistedPayment},
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

    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()>;

    async fn persist_channel_monitor<PS: LexePersister>(
        &self,
        chain_monitor: &LexeChainMonitorType<PS>,
        funding_txo: &LxOutPoint,
    ) -> anyhow::Result<()>;

    // --- Provided methods --- //

    /// Reads all persisted events, along with their event IDs.
    async fn read_events(&self) -> anyhow::Result<Vec<(EventId, Event)>> {
        let dir = VfsDirectory::new(vfs::EVENTS_DIR);
        let ids_and_events = self
            .read_dir_maybereadable(&dir)
            .await?
            .into_iter()
            .map(|(file_id, event)| {
                let event_id = EventId::from_str(&file_id.filename)
                    .with_context(|| file_id.filename.clone())
                    .context("Couldn't parse event ID from filename")?;
                Ok((event_id, event))
            })
            .collect::<anyhow::Result<_>>()
            .context("Error while reading events")?;
        Ok(ids_and_events)
    }

    async fn persist_event(
        &self,
        event: &Event,
        event_id: &EventId,
    ) -> anyhow::Result<()> {
        let filename = event_id.to_string();
        let file_id = VfsFileId::new(vfs::EVENTS_DIR, filename);
        // Failed event persistence can result in the node shutting down, so try
        // a few extra times. TODO(max): Change back to 1 once we switch to
        // LDK's fallible event handling.
        let retries = 3;
        self.persist_ldk_writeable(file_id, &event, retries).await
    }

    async fn remove_event(&self, event_id: &EventId) -> anyhow::Result<()> {
        let filename = event_id.to_string();
        let file_id = VfsFileId::new(vfs::EVENTS_DIR, filename);
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
