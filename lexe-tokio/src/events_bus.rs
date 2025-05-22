use tokio::sync::broadcast;

/// The [`EventsBus`] makes it easy to listen on events from some producer
/// (or possibly many producers).
///
/// - Simply clone the [`EventsBus`] to get another handle to it.
/// - Call [`notify`] to send an event onto the bus.
/// - Call [`subscribe`] to _. Events emitted prior to [`subscribe`] will not be
///   received.
///
/// API handlers like `open_channel` and
/// `close_channel` wait on channel lifecycle events (pending, ready, closed)
/// for specific channels.
///
/// We use a [`tokio::sync::broadcast`] channel here because (1) event
/// notification is a noop if there are no waiters, which is common, and (2) we
/// don't need to garbage collect waiters that timeout.
///
/// [`notify`]: Self::notify
/// [`subscribe`]: Self::subscribe
#[derive(Clone)]
pub struct EventsBus<T> {
    event_tx: broadcast::Sender<T>,
}

impl<T: Clone> EventsBus<T> {
    pub fn new() -> Self {
        Self {
            event_tx: broadcast::channel(crate::DEFAULT_CHANNEL_SIZE).0,
        }
    }

    /// Called from the event handler, when it observes a channel event.
    pub fn notify(&self, event: T) {
        // `broadcast::Sender::send` returns an error if there are no active
        // receivers. That's fine in this case.
        let _ = self.event_tx.send(event);
    }

    /// Start listening to all new events that get [`Self::notify`]'d
    /// after this point.
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

    /// Wait for the next event that makes `filter` return true.
    ///
    /// Will wait indefinitely, so make sure there's a timeout somewhere around
    /// this.
    pub async fn next_filtered(&mut self, filter: impl Fn(&T) -> bool) -> T {
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
