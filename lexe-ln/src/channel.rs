use common::ln::channel::{LxChannelId, LxUserChannelId};
use lightning::events::ClosureReason;

/// Channel lifecycle events emitted from the node event handler.
///
/// Tail these events using the [`EventsBus`].
///
/// [`EventsBus`]: lexe_tokio::events_bus::EventsBus
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
