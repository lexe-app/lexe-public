use std::time::Duration;

use serde::{Deserialize, Serialize};

/// An enum for calling various `TestEventReceiver` methods.
#[derive(Debug, Serialize, Deserialize)]
pub enum TestEventOp {
    Clear,
    Wait(TestEvent),
    WaitN(TestEvent, usize),
    WaitAll(Vec<TestEvent>),
    WaitAllN(Vec<(TestEvent, usize)>),
    WaitTimeout(TestEvent, Duration),
    WaitNTimeout(TestEvent, usize, Duration),
    WaitAllTimeout(Vec<TestEvent>, Duration),
    WaitAllNTimeout(Vec<(TestEvent, usize)>, Duration),
}

/// Test events emitted throughout the node that allow test to know when
/// something has happened, obviating the need for sleeps (which introduce
/// flakiness) while keeping tests reasonably fast.
// This is named `TestEvent` (not `LxEvent`) in case we need a `LxEvent` later.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TestEvent {
    /// A [`FundingGenerationReady`] event was handled.
    ///
    /// [`FundingGenerationReady`]: lightning::events::Event::FundingGenerationReady
    FundingGenerationHandled,
    /// An on-chain transaction was successfully broadcasted by `LexeEsplora`.
    TxBroadcasted,
    /// A [`ChannelPending`] event was handled.
    ///
    /// [`ChannelPending`]: lightning::events::Event::ChannelPending
    ChannelPending,
    /// A [`ChannelReady`] event was handled.
    ///
    /// [`ChannelReady`]: lightning::events::Event::ChannelReady
    ChannelReady,
    /// A [`PaymentClaimable`] event was handled.
    ///
    /// [`PaymentClaimable`]: lightning::events::Event::PaymentClaimable
    PaymentClaimable,
    /// A [`PaymentClaimed`] event was handled.
    ///
    /// [`PaymentClaimed`]: lightning::events::Event::PaymentClaimed
    PaymentClaimed,
    /// A [`PaymentSent`] event was handled.
    ///
    /// [`PaymentSent`]: lightning::events::Event::PaymentSent
    PaymentSent,
    /// A [`PaymentFailed`] event was handled.
    ///
    /// [`PaymentFailed`]: lightning::events::Event::PaymentFailed
    PaymentFailed,
    /// A [`ChannelClosed`] event was handled.
    ///
    /// [`ChannelClosed`]: lightning::events::Event::ChannelClosed
    ChannelClosed,
    /// A [`SpendableOutputs`] event was handled.
    ///
    /// [`SpendableOutputs`]: lightning::events::Event::SpendableOutputs
    SpendableOutputs,
}

impl From<TestEvent> for Vec<TestEvent> {
    fn from(event: TestEvent) -> Self {
        vec![event]
    }
}
