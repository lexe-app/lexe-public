use common::ln::channel::{LxChannelId, LxUserChannelId};
use lexe_tokio::DEFAULT_CHANNEL_SIZE;
use lightning::events::ClosureReason;
use tokio::sync::broadcast;

/// Channel lifecycle events emitted from the node event handler.
///
/// Tail these events using the [`ChannelEventsBus`].
#[derive(Clone)]
pub enum ChannelEvent {
    Pending {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
        funding_txo: bitcoin::OutPoint,
    },
    Ready {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
    },
    Closed {
        user_channel_id: LxUserChannelId,
        channel_id: LxChannelId,
        reason: ClosureReason,
    },
}

impl ChannelEvent {
    pub fn channel_id(&self) -> &LxChannelId {
        match self {
            Self::Pending { channel_id, .. } => channel_id,
            Self::Ready { channel_id, .. } => channel_id,
            Self::Closed { channel_id, .. } => channel_id,
        }
    }

    pub fn user_channel_id(&self) -> &LxUserChannelId {
        match self {
            Self::Pending {
                user_channel_id, ..
            } => user_channel_id,
            Self::Ready {
                user_channel_id, ..
            } => user_channel_id,
            Self::Closed {
                user_channel_id, ..
            } => user_channel_id,
        }
    }
}

/// The `ChannelEventsBus` lets API handlers like `open_channel` and
/// `close_channel` wait on channel lifecycle events (pending, ready, closed)
/// for specific channels.
///
/// We use a [`tokio::sync::broadcast`] channel here because (1) event
/// notification is a noop if there are no waiters, which is common, and (2) we
/// don't need to garbage collect waiters that timeout.
#[derive(Clone)]
pub struct ChannelEventsBus {
    event_tx: broadcast::Sender<ChannelEvent>,
}

impl ChannelEventsBus {
    pub fn new() -> Self {
        Self {
            event_tx: broadcast::channel(DEFAULT_CHANNEL_SIZE).0,
        }
    }

    /// Called from the event handler, when it observes a channel event.
    pub fn notify(&self, event: ChannelEvent) {
        // `broadcast::Sender::send` returns an error if there are no active
        // receivers. That's fine in this case.
        let _ = self.event_tx.send(event);
    }

    /// Start listening to all new [`ChannelEvent`]s that get [`Self::notify`]'d
    /// after this point.
    ///
    /// Be sure to start tailing events quickly so they don't queue up and you
    /// don't lose events.
    pub fn subscribe(&self) -> ChannelEventsRx<'_> {
        ChannelEventsRx::subscribe(&self.event_tx)
    }
}

pub struct ChannelEventsRx<'a> {
    // Hold on to this sender handle so the channel can't shutdown while we're
    // waiting.
    _event_tx: &'a broadcast::Sender<ChannelEvent>,
    event_rx: broadcast::Receiver<ChannelEvent>,
}

impl<'a> ChannelEventsRx<'a> {
    fn subscribe(event_tx: &'a broadcast::Sender<ChannelEvent>) -> Self {
        Self {
            _event_tx: event_tx,
            event_rx: event_tx.subscribe(),
        }
    }

    /// Wait for the next [`ChannelEvent`] that makes `filter` return true.
    ///
    /// Will wait indefinitely, so make sure there's a timeout somewhere around
    /// this.
    pub async fn next_filtered(
        &mut self,
        filter: impl Fn(&ChannelEvent) -> bool,
    ) -> ChannelEvent {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match self.event_rx.recv().await {
                Ok(event) =>
                    if filter(&event) {
                        return event;
                    },
                Err(RecvError::Closed) => panic!(
                    "This cannot happen. We currently have a handle to the \
                     `event_tx` sender, so the channel cannot be closed."
                ),
                // We missed some notifications somehow (too slow). Nothing
                // much we can do other than keep going
                // until timeout.
                Err(RecvError::Lagged(_)) => (),
            }
        }
    }
}
