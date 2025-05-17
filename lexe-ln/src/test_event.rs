use std::{
    collections::HashMap,
    fmt::Write,
    mem::{self, Discriminant},
    time::Duration,
};

use anyhow::bail;
use cfg_if::cfg_if;
use common::test_event::{TestEvent, TestEventOp};
use lexe_api::{rest, server};
use tokio::sync::mpsc;
use tracing::debug;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
lexe_std::const_assert!(
    rest::API_REQUEST_TIMEOUT.as_secs() > DEFAULT_TIMEOUT.as_secs()
);
lexe_std::const_assert!(
    server::SERVER_HANDLER_TIMEOUT.as_secs() > DEFAULT_TIMEOUT.as_secs()
);

const TEST_EVENT_CHANNEL_SIZE: usize = 16;

/// Creates a [`TestEvent`] channel, returning a `(tx, rx)` tuple.
pub fn channel(label: &'static str) -> (TestEventSender, TestEventReceiver) {
    let (tx, rx) = mpsc::channel(TEST_EVENT_CHANNEL_SIZE);
    let sender = TestEventSender::new(label, tx);
    let receiver = TestEventReceiver::new(label, rx);
    (sender, receiver)
}

/// A handler for calling any of the [`TestEventReceiver`] methods.
pub async fn do_op(
    op: TestEventOp,
    rx: &tokio::sync::Mutex<TestEventReceiver>,
) -> anyhow::Result<()> {
    cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            use anyhow::Context;
            use TestEventOp::*;
            let mut rx = rx
                .try_lock()
                .context("Can only call one /test_event endpoint at once!")?;

            match op {
                #[allow(clippy::unit_arg)] // Dumb lint
                Clear => Ok(rx.clear()),
                Wait(event) => rx.wait(event).await,
                WaitN(event, n) => rx.wait_n(event, n).await,
                WaitAll(all_events) => rx.wait_all(all_events).await,
                WaitAllN(all_n_events) => rx.wait_all_n(all_n_events).await,
                WaitTimeout(event, timeout) =>
                    rx.wait_timeout(event, timeout).await,
                WaitNTimeout(event, n, timeout) =>
                    rx.wait_n_timeout(event, n, timeout).await,
                WaitAllTimeout(all_events, timeout) =>
                    rx.wait_all_timeout(all_events, timeout).await,
                WaitAllNTimeout(all_n_events, timeout) =>
                    rx.wait_all_n_timeout(all_n_events, timeout).await,
            }
        } else {
            let _ = op;
            let _ = rx;
            bail!("This endpoint is disabled in staging/prod");
        }
    }
}

/// Wraps an [`mpsc::Sender<TestEvent>`] to allow actually sending the event to
/// be cfg'd out in staging/prod.
#[derive(Clone)]
pub struct TestEventSender {
    /// A label (e.g. "(user)", "(lsp)") which allows "received test event" log
    /// outputs emitted by this receiver to be differentiated from similar log
    /// outputs emitted by other receivers.
    #[cfg_attr(all(not(test), not(feature = "test-utils")), allow(dead_code))]
    label: &'static str,
    #[cfg(any(test, feature = "test-utils"))]
    tx: mpsc::Sender<TestEvent>,
}

impl TestEventSender {
    fn new(label: &'static str, tx: mpsc::Sender<TestEvent>) -> Self {
        cfg_if! {
            if #[cfg(any(test, feature = "test-utils"))] {
                Self { label, tx }
            } else {
                let _ = tx;
                Self { label }
            }
        }
    }

    pub fn send(&self, event: TestEvent) {
        cfg_if! {
            if #[cfg(any(test, feature = "test-utils"))] {
                let label = &self.label;
                debug!("{label} sending test event: {event:?}");
                let _ = self.tx.try_send(event);
            } else {
                let _ = event;
            }
        }
    }
}

/// Wraps a [`mpsc::Receiver<TestEvent>`] to provide convenience helpers for
/// waiting for certain events to occur.
pub struct TestEventReceiver {
    /// A label (e.g. "(user)", "(lsp)") which allows "received test event" log
    /// outputs emitted by this receiver to be differentiated from similar log
    /// outputs emitted by other receivers.
    label: &'static str,
    rx: mpsc::Receiver<TestEvent>,
}

impl TestEventReceiver {
    fn new(label: &'static str, rx: mpsc::Receiver<TestEvent>) -> Self {
        Self { label, rx }
    }

    /// Clears the channel of all pending messages.
    pub fn clear(&mut self) {
        let label = &self.label;
        while let Ok(event) = self.rx.try_recv() {
            debug!("{label} Clearing event: {event:?}");
        }
    }

    // --- default timeout --- //

    /// Waits to receive the given [`TestEvent`] on the channel, ignoring and
    /// discarding all other events.
    ///
    /// - Returns [`Err`] if the default timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// test_event_rx
    ///     .wait(TestEvent::TxBroadcasted)
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait(&mut self, event: TestEvent) -> anyhow::Result<()> {
        self.wait_timeout(event, DEFAULT_TIMEOUT).await
    }

    /// Waits on the channel until the given [`TestEvent`] has been seen `n`
    /// times, ignoring and discarding all other events.
    ///
    /// - Returns [`Err`] if the default timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_n() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// test_event_rx
    ///     .wait_n(TestEvent::TxBroadcasted, 3)
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_n(
        &mut self,
        event: TestEvent,
        n: usize,
    ) -> anyhow::Result<()> {
        self.wait_n_timeout(event, n, DEFAULT_TIMEOUT).await
    }

    /// Waits on the channel until all given [`TestEvent`]s have been observed,
    /// ignoring and discarding all other events.
    ///
    /// - Returns [`Err`] if the default timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_all() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::FundingGenerationHandled);
    /// test_event_rx
    ///     .wait_all(vec![
    ///         TestEvent::TxBroadcasted,
    ///         TestEvent::FundingGenerationHandled,
    ///     ])
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_all(
        &mut self,
        all_events: Vec<TestEvent>,
    ) -> anyhow::Result<()> {
        self.wait_all_timeout(all_events, DEFAULT_TIMEOUT).await
    }

    /// Waits on the channel until all given [`TestEvent`]s have been observed
    /// `n_i` times for all `i` in `[0..all_n_events.len()]`, ignoring and
    /// discarding all other events.
    ///
    /// - Returns [`Err`] if the default timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_all_n() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::FundingGenerationHandled);
    /// test_event_rx
    ///     .wait_all_n(vec![
    ///         (TestEvent::TxBroadcasted, 3),
    ///         (TestEvent::FundingGenerationHandled, 1),
    ///     ])
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_all_n(
        &mut self,
        all_n_events: Vec<(TestEvent, usize)>,
    ) -> anyhow::Result<()> {
        self.wait_all_n_timeout(all_n_events, DEFAULT_TIMEOUT).await
    }

    // --- custom timeouts --- //

    /// Waits to receive the given [`TestEvent`] on the channel, ignoring and
    /// discarding all other events.
    ///
    /// - Returns [`Err`] if the timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// test_event_rx
    ///     .wait_timeout(
    ///         TestEvent::TxBroadcasted,
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_timeout(
        &mut self,
        event: TestEvent,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        self.wait_all_n_timeout(vec![(event, 1)], timeout).await
    }

    /// Waits on the channel until the given [`TestEvent`] has been seen `n`
    /// times, ignoring and discarding all other events.
    ///
    /// - Returns [`Err`] if the timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_n_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// test_event_rx
    ///     .wait_n_timeout(TestEvent::TxBroadcasted, 3)
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_n_timeout(
        &mut self,
        event: TestEvent,
        n: usize,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        self.wait_all_n_timeout(vec![(event, n)], timeout).await
    }

    /// Waits on the channel until all given [`TestEvent`]s have been observed,
    /// ignoring and discarding all other events.
    ///
    /// - Returns [`Err`] if the timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use common::test_event::TestEvent;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_all_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::FundingGenerationHandled);
    /// test_event_rx
    ///     .wait_all_timeout(
    ///         vec![
    ///             TestEvent::TxBroadcasted,
    ///             TestEvent::FundingGenerationHandled,
    ///         ],
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_all_timeout(
        &mut self,
        all_events: Vec<TestEvent>,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let all_n_events = all_events
            .into_iter()
            // Default to requiring each event be seen once
            .map(|e| (e, 1))
            .collect::<Vec<(TestEvent, usize)>>();
        self.wait_all_n_timeout(all_n_events, timeout).await
    }

    /// Waits on the channel until all given [`TestEvent`]s have been observed
    /// `n_i` times for all `i` in `[0..all_n_events.len()]`, ignoring and
    /// discarding all other events.
    ///
    /// - Returns [`Err`] if the timeout was reached.
    /// - Panics if the sender was dropped.
    ///
    /// # Example
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use lexe_ln::test_event;
    /// # #[tokio::test]
    /// # async fn wait_all_n_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event::channel();
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::TxBroadcasted);
    /// # test_event_tx.send(TestEvent::FundingGenerationHandled);
    /// test_event_rx
    ///     .wait_all_n_timeout(
    ///         vec![
    ///             (TestEvent::TxBroadcasted, 3),
    ///             (TestEvent::FundingGenerationHandled, 1),
    ///         ],
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out");
    /// # }
    /// ```
    pub async fn wait_all_n_timeout(
        &mut self,
        all_n_events: Vec<(TestEvent, usize)>,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        debug!("Waiting on {all_n_events:?}");

        struct Quota {
            name: String,
            seen: usize,
            needed: usize,
        }

        // Initialize quotas for all the test events we're looking for
        let mut quotas = HashMap::<Discriminant<TestEvent>, Quota>::new();
        for (event, needed) in all_n_events {
            let k = mem::discriminant(&event);
            let v = Quota {
                name: format!("{event:?}"),
                seen: 0, // We haven't seen anything yet
                needed,
            };
            quotas.insert(k, v);
        }

        // Return early if all quotas have already been met,
        // i.e. no events were supplied
        if quotas.values().all(|q| q.seen >= q.needed) {
            return Ok(());
        }

        // Create a sleep future which can be polled without being consumed
        let timeout_fut = tokio::time::sleep(timeout);
        tokio::pin!(timeout_fut);

        let label = &self.label;
        loop {
            tokio::select! {
                maybe_recvd = self.rx.recv() => match maybe_recvd {
                    Some(recvd) => {
                        debug!("{label} received test event: {recvd:?}");

                        // Increment the quota for the recvd event if it exists
                        let discriminant = mem::discriminant(&recvd);
                        if let Some(quota) = quotas.get_mut(&discriminant) {
                            quota.seen += 1;
                        }

                        // Check to see if all quotas have been met
                        if quotas.values().all(|q| q.seen >= q.needed) {
                            return Ok(());
                        }
                    }
                    None => bail!("Sender dropped"),
                },
                () = &mut timeout_fut => {
                    // Construct an error msg showing events with unmet quotas
                    let mut err_msg =
                        format!("{label} timed out waiting for test events: ");
                    for Quota { name, seen, needed } in quotas.into_values() {
                        if seen < needed {
                            write!(&mut err_msg, "{seen}/{needed} {name}, ")
                                .expect("Could not write to string??");
                        }
                    }

                    // Remove the trailing ", ". Can't use str::strip_suffix
                    // because `anyhow::Error` needs a `'static` (owned) string
                    err_msg.pop();
                    err_msg.pop();

                    bail!(err_msg);
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use tokio_test::{assert_pending, assert_ready};

    use super::*;

    #[tokio::test]
    async fn pending_before_ready_after() {
        let event1 = TestEvent::TxBroadcasted;
        let event2 = TestEvent::FundingGenerationHandled;
        let label = "(node)";

        // wait()
        let (tx, mut rx) = channel(label);
        let mut task = tokio_test::task::spawn(rx.wait(event1));
        assert_pending!(task.poll());
        tx.send(event1);
        assert_ready!(task.poll()).unwrap();

        // wait_n()
        let (tx, mut rx) = channel(label);
        let mut task = tokio_test::task::spawn(rx.wait_n(event1, 3));
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event1);
        tx.send(event1);
        assert_ready!(task.poll()).unwrap();

        // wait_all()
        let (tx, mut rx) = channel(label);
        let mut task =
            tokio_test::task::spawn(rx.wait_all(vec![event1, event2]));
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event2);
        assert_ready!(task.poll()).unwrap();

        // wait_all_n()
        let (tx, mut rx) = channel(label);
        let mut task = tokio_test::task::spawn(
            rx.wait_all_n(vec![(event1, 3), (event2, 1)]),
        );
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event1);
        tx.send(event1);
        tx.send(event2);
        assert_ready!(task.poll()).unwrap();

        // wait_all(), 0 events
        let (_tx, mut rx) = channel(label);
        let mut task = tokio_test::task::spawn(rx.wait_all(vec![]));
        assert_ready!(task.poll()).unwrap();

        // wait_all_n(), events with 0 quota
        let (_tx, mut rx) = channel(label);
        let mut task = tokio_test::task::spawn(
            rx.wait_all_n(vec![(event1, 0), (event2, 0)]),
        );
        assert_ready!(task.poll()).unwrap();
    }
}
