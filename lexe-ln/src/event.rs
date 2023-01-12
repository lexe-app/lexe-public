use lightning::util::events::Event;

// TODO(max): Perhaps we should upstream this as a Display impl?
pub fn get_event_name(event: &Event) -> &'static str {
    match event {
        Event::OpenChannelRequest { .. } => "open channel request",
        Event::FundingGenerationReady { .. } => "funding generation ready",
        Event::ChannelReady { .. } => "channel ready",
        Event::PaymentClaimable { .. } => "payment claimable",
        Event::HTLCIntercepted { .. } => "HTLC intercepted",
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
        Event::HTLCHandlingFailed { .. } => "HTLC handling failed",
    }
}
