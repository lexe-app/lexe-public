use tokio::sync::broadcast;

use crate::DEFAULT_CHANNEL_SIZE;

/// The [`EventsBus`] makes it easy to listen on events from some producer
/// (or possibly many producers).
///
/// - Simply clone the [`EventsBus`] to get another handle to it.
/// - Call [`send`] to send an event onto the bus. If no waiters are registered,
///   this is a noop.
/// - Call [`subscribe`] to get a receiver. Events emitted prior to
///   [`subscribe`] will not be received.
///
/// We use a [`tokio::sync::broadcast`] channel here because
/// (1) event notification is a noop if there are no waiters, which is common,
/// (2) we don't need to garbage collect waiters that timeout.
///
/// [`send`]: Self::send
/// [`subscribe`]: Self::subscribe
#[derive(Clone)]
pub struct EventsBus<T> {
    event_tx: broadcast::Sender<T>,
}

impl<T: Clone> EventsBus<T> {
    /// Create a new [`EventsBus`] with the default channel size.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::new_with_size(DEFAULT_CHANNEL_SIZE)
    }

    /// Create a new [`EventsBus`] with a custom channel size.
    pub fn new_with_size(size: usize) -> Self {
        Self {
            event_tx: broadcast::channel(size).0,
        }
    }

    /// Notify all waiters (if any) that an event occurred.
    pub fn send(&self, event: T) {
        // `broadcast::Sender::send` returns an error if there are no active
        // receivers. That's fine for us.
        let _ = self.event_tx.send(event);
    }

    /// Get a subscriber which will get notified after this point.
    ///
    /// Be sure to start tailing events quickly so they don't queue up and you
    /// don't lose events.
    pub fn subscribe(&self) -> EventsRx<'_, T> {
        EventsRx::subscribe(&self.event_tx)
    }
}

pub struct EventsRx<'a, T> {
    // Hold on to this sender handle so the channel can't shutdown while we're
    // waiting.
    _event_tx: &'a broadcast::Sender<T>,
    event_rx: broadcast::Receiver<T>,
}

impl<'a, T: Clone> EventsRx<'a, T> {
    fn subscribe(event_tx: &'a broadcast::Sender<T>) -> Self {
        Self {
            _event_tx: event_tx,
            event_rx: event_tx.subscribe(),
        }
    }

    /// Wait for the next event.
    ///
    /// Will wait indefinitely, so ensure there's a timeout around this.
    pub async fn recv(&mut self) -> T {
        self.recv_filtered(|_| true).await
    }

    /// Wait for the next event that makes `filter` return true.
    ///
    /// Will wait indefinitely, so ensure there's a timeout around this.
    pub async fn recv_filtered(&mut self, filter: impl Fn(&T) -> bool) -> T {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match self.event_rx.recv().await {
                Ok(event) =>
                    if filter(&event) {
                        return event;
                    },
                Err(RecvError::Closed) => unreachable!(
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
