use anyhow::{anyhow, Context};
use bitcoin::blockdata::script::Script;
use bitcoin::secp256k1;
use common::hex;
use lightning::chain::chaininterface::ConfirmationTarget;
use lightning::ln::PaymentHash;
use lightning::util::events::{Event, PaymentPurpose};
use tracing::info;

use crate::payments::manager::PaymentsManager;
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};
use crate::wallet::LexeWallet;

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

/// Handles a [`Event::FundingGenerationReady`].
pub async fn handle_funding_generation_ready<CM, PS>(
    wallet: &LexeWallet,
    channel_manager: CM,
    test_event_tx: &TestEventSender,

    temporary_channel_id: [u8; 32],
    counterparty_node_id: secp256k1::PublicKey,
    channel_value_satoshis: u64,
    output_script: Script,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let conf_target = ConfirmationTarget::Normal;
    let signed_raw_funding_tx = wallet
        .create_and_sign_funding_tx(
            output_script,
            channel_value_satoshis,
            conf_target,
        )
        .await
        .context("Could not create and sign funding tx")
        // Force close the pending channel if funding tx generation failed.
        .inspect_err(|_| {
            channel_manager
                .force_close_without_broadcasting_txn(
                    &temporary_channel_id,
                    &counterparty_node_id,
                )
                .expect(
                    "Failed to force close after funding generation failed",
                );
        })?;

    channel_manager
        .funding_transaction_generated(
            &temporary_channel_id,
            &counterparty_node_id,
            signed_raw_funding_tx,
        )
        .inspect(|()| test_event_tx.send(TestEvent::FundingGenerationHandled))
        .map_err(|e| anyhow!("{e:?}"))
        .context("LDK rejected the signed funding tx")?;

    Ok(())
}

/// Handles a [`Event::PaymentClaimable`].
pub fn handle_payment_claimable<CM, PS>(
    channel_manager: CM,
    payments_manager: PaymentsManager<PS>,
    test_event_tx: &TestEventSender,

    payment_hash: PaymentHash,
    amt_msat: u64,
    purpose: PaymentPurpose,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let hash_str = hex::encode(&payment_hash.0);
    info!("Received payment of {amt_msat} msats with hash {hash_str}");

    payments_manager
        .payment_claimable(payment_hash, amt_msat, purpose.clone())
        .context("Error claiming payment")?;

    let payment_preimage = match purpose {
        PaymentPurpose::InvoicePayment {
            payment_preimage, ..
        } => payment_preimage.expect(
            "We previously generated this invoice using a method other than \
            `ChannelManager::create_inbound_payment`, resulting in the channel \
            manager not being aware of the payment preimage, OR LDK failed to \
            provide the preimage back to us.",
        ),
        PaymentPurpose::SpontaneousPayment(preimage) => preimage,
    };

    // The PaymentClaimable docs have a note that LDK will not stop an inbound
    // payment from being paid multiple times. We should fail the payment in
    // this case because:
    // - This messes up (or significantly complicates) our accounting
    // - This likely reflects an error on the receiver's part (reusing the same
    //   invoice for multiple payments, which would allow any nodes along the
    //   first payment path to steal subsequent payments)
    // - We should not allow payments to go through, in order to teach users
    //   that this is not an acceptable way to use lightning, because it is not
    //   safe. It is not hard to imagine users developing the misconception that
    //   it is safe to reuse invoices if duplicate payments actually do succeed.
    // TODO(max): If LDK implements the regeneration of PaymentClaimable events
    // upon restart, we'll need a way to differentiate between these regenerated
    // events and duplicate payments to the same invoice.
    // https://discord.com/channels/915026692102316113/978829624635195422/1085427966986690570

    // TODO(max): `claim_funds` docs state that we must check that the amt_msat
    // we received matches our expectation, relevant if we're receiving payment
    // for e.g. an order of some sort. Otherwise, we will have given the sender
    // a proof-of-payment when they did not fulfill the full expected payment.
    // Implement this once it becomes relevant.
    channel_manager.claim_funds(payment_preimage);

    test_event_tx.send(TestEvent::PaymentClaimable);

    Ok(())
}
