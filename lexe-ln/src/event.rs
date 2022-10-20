use lightning::util::events::Event;
use tokio::sync::mpsc;

const TEST_EVENT_CHANNEL_SIZE: usize = 16; // Increase if needed

// TODO(max): Perhaps we should upstream this as a Display impl?
pub fn get_event_name(event: &Event) -> &'static str {
    match event {
        Event::FundingGenerationReady { .. } => "funding generation ready",
        Event::PaymentReceived { .. } => "payment received",
        Event::PaymentClaimed { .. } => "payment claimed",
        Event::PaymentSent { .. } => "payment sent",
        Event::PaymentFailed { .. } => "payment failed",
        Event::PaymentPathSuccessful { .. } => "payment path successful",
        Event::PaymentPathFailed { .. } => "payment path failed",
        Event::ProbeSuccessful { .. } => "probe successful",
        Event::ProbeFailed { .. } => "probe failed",
        Event::PendingHTLCsForwardable { .. } => "pending HTLCs forwardable",
        Event::SpendableOutputs { .. } => "spendable outputs",
        Event::PaymentForwarded { .. } => "payment forwarded",
        Event::ChannelClosed { .. } => "channel closed",
        Event::DiscardFunding { .. } => "discard funding",
        Event::OpenChannelRequest { .. } => "open channel request",
        Event::HTLCHandlingFailed { .. } => "HTLC handling failed",
    }
}

/// Test events emitted throughout the node that allow a white box test to know
/// when something has happened, obviating the need for sleeps (which introduce
/// flakiness) while keeping tests reasonably fast.
// This is named `TestEvent` (not `LxEvent`) in case we need a `LxEvent` later.
#[derive(Debug, Eq, PartialEq)]
pub enum TestEvent {
    /// A [`Event::FundingGenerationReady`] event was handled; i.e. a funding
    /// tx was successfully generated, broadcasted, and fed back into LDK.
    FundingTxHandled,
}

/// Creates a [`TestEvent`] channel, returning a `(tx, rx)` tuple.
pub fn test_event_channel() -> (TestEventSender, TestEventReceiver) {
    let (tx, rx) = mpsc::channel(TEST_EVENT_CHANNEL_SIZE);
    (TestEventSender { tx }, TestEventReceiver { rx })
}

/// Wraps an [`mpsc::Sender<TestEvent>`] to allow actually sending the event to
/// be cfg'd out in prod.
#[derive(Clone)]
pub struct TestEventSender {
    #[cfg_attr(target_env = "sgx", allow(dead_code))]
    tx: mpsc::Sender<TestEvent>,
}

impl TestEventSender {
    #[cfg_attr(target_env = "sgx", allow(unused_variables))]
    pub fn send(&self, event: TestEvent) {
        #[cfg(any(test, not(target_env = "sgx")))]
        self.tx.try_send(event).expect("Channel was full")
    }
}

/// Wraps a [`mpsc::Receiver<TestEvent>`] to provide convenience helpers for
/// waiting for certain events to occur.
pub struct TestEventReceiver {
    rx: mpsc::Receiver<TestEvent>,
}

impl TestEventReceiver {
    /// Clears the channel of all pending messages.
    pub fn clear(&mut self) {
        while self.rx.try_recv().is_ok() {}
    }

    /// Waits to receive the given [`TestEvent`] on the channel, ignoring and
    /// discarding all other events. Panics if the sender was dropped.
    pub async fn wait_on(&mut self, given: TestEvent) {
        while let Some(recvd) = self.rx.recv().await {
            if recvd == given {
                return;
            }
        }
        panic!("Sender dropped");
    }
}

// TODO(max): Implement waiting on all of a Vec of events to occur
// TODO(max): Implement waiting on a single event occurring n times
// TODO(max): Implement waiting on all of a Vec of (event, n) tuples to occur
