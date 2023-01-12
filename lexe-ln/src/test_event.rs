use std::collections::HashMap;
use std::mem::{self, Discriminant};
use std::time::Duration;

use cfg_if::cfg_if;
use tokio::sync::mpsc;
use tracing::debug;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15); // Increase if needed
const TEST_EVENT_CHANNEL_SIZE: usize = 16; // Increase if needed

/// Creates a [`TestEvent`] channel, returning a `(tx, rx)` tuple.
pub fn test_event_channel() -> (TestEventSender, TestEventReceiver) {
    let (tx, rx) = mpsc::channel(TEST_EVENT_CHANNEL_SIZE);
    let sender = TestEventSender::new(tx);
    let receiver = TestEventReceiver::new(rx);
    (sender, receiver)
}

/// Test events emitted throughout the node that allow a white box test to know
/// when something has happened, obviating the need for sleeps (which introduce
/// flakiness) while keeping tests reasonably fast.
// This is named `TestEvent` (not `LxEvent`) in case we need a `LxEvent` later.
// NOTE: Perhaps we could allow the host (Lexe) to subscribe to a TestEvent
// stream so that black box tests can get notifications as well, even in SGX...
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TestEvent {
    /// A [`FundingGenerationReady`] event was handled; i.e. a funding
    /// tx was successfully generated, broadcasted, and fed back into LDK.
    ///
    /// [`FundingGenerationReady`]: lightning::util::events::Event::FundingGenerationReady
    FundingTxHandled,
    /// A [`ChannelReady`] event was handled.
    ///
    /// [`ChannelReady`]: lightning::util::events::Event::ChannelReady
    ChannelReady,
    /// A channel monitor update was successfully persisted.
    ChannelMonitorPersisted,
    /// A [`PaymentClaimable`] event was handled.
    ///
    /// [`PaymentClaimable`]: lightning::util::events::Event::PaymentClaimable
    PaymentClaimable,
    /// A [`PaymentClaimed`] event was handled.
    ///
    /// [`PaymentClaimed`]: lightning::util::events::Event::PaymentClaimed
    PaymentClaimed,
    /// A [`PaymentSent`] event was handled.
    ///
    /// [`PaymentSent`]: lightning::util::events::Event::PaymentSent
    PaymentSent,
}

/// Wraps an [`mpsc::Sender<TestEvent>`] to allow actually sending the event to
/// be cfg'd out in prod.
#[derive(Clone)]
pub struct TestEventSender {
    #[cfg(any(test, not(target_env = "sgx")))]
    tx: mpsc::Sender<TestEvent>,
}

impl TestEventSender {
    fn new(tx: mpsc::Sender<TestEvent>) -> Self {
        cfg_if! {
            if #[cfg(any(test, not(target_env = "sgx")))] {
                Self { tx }
            } else {
                let _ = tx;
                Self {}
            }
        }
    }

    pub fn send(&self, event: TestEvent) {
        cfg_if! {
            if #[cfg(any(test, not(target_env = "sgx")))] {
                self.tx.try_send(event).expect("Channel was full")
            } else {
                let _ = event;
            }
        }
    }
}

/// Wraps a [`mpsc::Receiver<TestEvent>`] to provide convenience helpers for
/// waiting for certain events to occur.
pub struct TestEventReceiver {
    rx: mpsc::Receiver<TestEvent>,
}

impl TestEventReceiver {
    fn new(rx: mpsc::Receiver<TestEvent>) -> Self {
        Self { rx }
    }

    /// Clears the channel of all pending messages.
    pub fn clear(&mut self) {
        while self.rx.try_recv().is_ok() {}
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// test_event_rx
    ///     .wait(TestEvent::ChannelMonitorPersisted)
    ///     .await
    ///     .expect("Timed out waiting on channel monitor persist");
    /// # }
    /// ```
    pub async fn wait(&mut self, event: TestEvent) -> Result<(), &'static str> {
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_n() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// test_event_rx
    ///     .wait_n(TestEvent::ChannelMonitorPersisted, 3)
    ///     .await
    ///     .expect("Timed out waiting on channel monitor persist");
    /// # }
    /// ```
    pub async fn wait_n(
        &mut self,
        event: TestEvent,
        n: usize,
    ) -> Result<(), &'static str> {
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_all() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::FundingTxHandled);
    /// test_event_rx
    ///     .wait_all(vec![
    ///         TestEvent::ChannelMonitorPersisted, TestEvent::FundingTxHandled,
    ///     ])
    ///     .await
    ///     .expect("Timed out waiting on persist and funding tx");
    /// # }
    /// ```
    pub async fn wait_all(
        &mut self,
        all_events: Vec<TestEvent>,
    ) -> Result<(), &'static str> {
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_all_n() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::FundingTxHandled);
    /// test_event_rx
    ///     .wait_all_n(vec![
    ///         (TestEvent::ChannelMonitorPersisted, 3),
    ///         (TestEvent::FundingTxHandled, 1),
    ///     ])
    ///     .await
    ///     .expect("Timed out waiting on persist and funding tx");
    /// # }
    /// ```
    pub async fn wait_all_n(
        &mut self,
        all_n_events: Vec<(TestEvent, usize)>,
    ) -> Result<(), &'static str> {
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// test_event_rx
    ///     .wait_timeout(
    ///         TestEvent::ChannelMonitorPersisted,
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out waiting on channel monitor persist");
    /// # }
    /// ```
    pub async fn wait_timeout(
        &mut self,
        event: TestEvent,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        tokio::select! {
            () = self.wait_inner(event) => Ok(()),
            () = tokio::time::sleep(timeout) => Err("Timed out"),
        }
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_n_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// test_event_rx
    ///     .wait_n_timeout(TestEvent::ChannelMonitorPersisted, 3)
    ///     .await
    ///     .expect("Timed out waiting on channel monitor persist");
    /// # }
    /// ```
    pub async fn wait_n_timeout(
        &mut self,
        event: TestEvent,
        n: usize,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        tokio::select! {
            () = self.wait_n_inner(event, n) => Ok(()),
            () = tokio::time::sleep(timeout) => Err("Timed out"),
        }
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_all_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::FundingTxHandled);
    /// test_event_rx
    ///     .wait_all_timeout(
    ///         vec![
    ///             TestEvent::ChannelMonitorPersisted,
    ///             TestEvent::FundingTxHandled,
    ///         ],
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out waiting on persist and funding tx");
    /// # }
    /// ```
    pub async fn wait_all_timeout(
        &mut self,
        all_events: Vec<TestEvent>,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        tokio::select! {
            () = self.wait_all_inner(all_events) => Ok(()),
            () = tokio::time::sleep(timeout) => Err("Timed out"),
        }
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
    /// # use lexe_ln::test_event::{test_event_channel, TestEvent};
    /// # #[tokio::test]
    /// # async fn wait_all_n_timeout() {
    /// # let (test_event_tx, test_event_rx) = test_event_channel();
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::ChannelMonitorPersisted);
    /// # test_event_tx.send(TestEvent::FundingTxHandled);
    /// test_event_rx
    ///     .wait_all_n_timeout(
    ///         vec![
    ///             (TestEvent::ChannelMonitorPersisted, 3),
    ///             (TestEvent::FundingTxHandled, 1),
    ///         ],
    ///         Duration::from_secs(15),
    ///     )
    ///     .await
    ///     .expect("Timed out waiting on persist and funding tx");
    /// # }
    /// ```
    pub async fn wait_all_n_timeout(
        &mut self,
        all_n_events: Vec<(TestEvent, usize)>,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        tokio::select! {
            () = self.wait_all_n_inner(all_n_events) => Ok(()),
            () = tokio::time::sleep(timeout) => Err("Timed out"),
        }
    }

    // --- Inner 'wait' methods --- //

    async fn wait_inner(&mut self, event: TestEvent) {
        self.wait_all_n_inner(vec![(event, 1)]).await
    }

    async fn wait_n_inner(&mut self, event: TestEvent, n: usize) {
        self.wait_all_n_inner(vec![(event, n)]).await
    }

    async fn wait_all_inner(&mut self, all_events: Vec<TestEvent>) {
        let all_n_events = all_events
            .into_iter()
            // Default to requiring each event be seen once
            .map(|e| (e, 1))
            .collect::<Vec<(TestEvent, usize)>>();
        self.wait_all_n_inner(all_n_events).await
    }

    async fn wait_all_n_inner(
        &mut self,
        all_n_events: Vec<(TestEvent, usize)>,
    ) {
        struct Quota {
            seen: usize,
            needed: usize,
        }

        // Initialize quotas for all the test events we're looking for
        let mut quotas = HashMap::<Discriminant<TestEvent>, Quota>::new();
        for (event, needed) in all_n_events {
            let k = mem::discriminant(&event);
            let v = Quota {
                seen: 0, // We haven't seen anything yet
                needed,
            };
            quotas.insert(k, v);
        }

        // Return early if all quotas have already been met,
        // i.e. no events were supplied
        if quotas.values().all(|q| q.seen >= q.needed) {
            return;
        }

        // Wait on the channel
        while let Some(recvd) = self.rx.recv().await {
            debug!("Received test event: {recvd:?}");

            // Increment the quota for the recvd event if it exists
            let discriminant = mem::discriminant(&recvd);
            if let Some(quota) = quotas.get_mut(&discriminant) {
                quota.seen += 1;
            }

            // Check to see if all quotas have been met
            if quotas.values().all(|q| q.seen >= q.needed) {
                return;
            }
        }

        // Received None on the channel, panic.
        panic!("Sender dropped");
    }
}

#[cfg(test)]
mod test {
    use tokio_test::{assert_pending, assert_ready};

    use super::*;

    #[tokio::test]
    async fn pending_before_ready_after() {
        let event1 = TestEvent::ChannelMonitorPersisted;
        let event2 = TestEvent::FundingTxHandled;

        // wait_inner()
        let (tx, mut rx) = test_event_channel();
        let mut task = tokio_test::task::spawn(rx.wait_inner(event1));
        assert_pending!(task.poll());
        tx.send(event1);
        assert_ready!(task.poll());

        // wait_n_inner()
        let (tx, mut rx) = test_event_channel();
        let mut task = tokio_test::task::spawn(rx.wait_n_inner(event1, 3));
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event1);
        tx.send(event1);
        assert_ready!(task.poll());

        // wait_all_inner()
        let (tx, mut rx) = test_event_channel();
        let mut task =
            tokio_test::task::spawn(rx.wait_all_inner(vec![event1, event2]));
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event2);
        assert_ready!(task.poll());

        // wait_all_n_inner()
        let (tx, mut rx) = test_event_channel();
        let mut task = tokio_test::task::spawn(
            rx.wait_all_n_inner(vec![(event1, 3), (event2, 1)]),
        );
        assert_pending!(task.poll());
        tx.send(event1);
        tx.send(event1);
        tx.send(event1);
        tx.send(event2);
        assert_ready!(task.poll());

        // wait_all_inner(), 0 events
        let (_tx, mut rx) = test_event_channel();
        let mut task = tokio_test::task::spawn(rx.wait_all_inner(vec![]));
        assert_ready!(task.poll());

        // wait_all_n_inner(), events with 0 quota
        let (_tx, mut rx) = test_event_channel();
        let mut task = tokio_test::task::spawn(
            rx.wait_all_n_inner(vec![(event1, 0), (event2, 0)]),
        );
        assert_ready!(task.poll());
    }
}
