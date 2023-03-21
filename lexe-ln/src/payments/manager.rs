use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, ensure, Context};
use lightning::util::events::PaymentPurpose;
use tracing::info;

use crate::payments::inbound::InboundSpontaneousPayment;
use crate::payments::{
    LxPaymentHash, LxPaymentId, LxPaymentPreimage, Payment, PaymentStatus,
};
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};

/// A simple type which annotates that a given [`Payment`] was returned by a
/// `check_` method which successfully validated a proposed state transition.
#[must_use]
pub struct CheckedPayment(pub Payment);

#[allow(dead_code)] // TODO(max): Remove
#[derive(Clone)]
pub struct PaymentsManager<CM: LexeChannelManager<PS>, PS: LexePersister> {
    data: Arc<Mutex<PaymentsData>>,
    persister: PS,
    channel_manager: CM,
    test_event_tx: TestEventSender,
}

/// Methods on [`PaymentsData`] take `&mut self`, which allows reentrancy
/// without deadlocking. [`PaymentsData`] also reduces code bloat from
/// monomorphization by taking only concrete types as parameters.
struct PaymentsData {
    pending: HashMap<LxPaymentId, Payment>,
    finalized: HashSet<LxPaymentId>,
}

impl<CM: LexeChannelManager<PS>, PS: LexePersister> PaymentsManager<CM, PS> {
    pub fn new(
        persister: PS,
        channel_manager: CM,
        test_event_tx: TestEventSender,
    ) -> Self {
        // TODO(max): Take initial data in parameters
        let data = Arc::new(Mutex::new(PaymentsData {
            pending: HashMap::new(),
            finalized: HashSet::new(),
        }));

        Self {
            data,
            persister,
            channel_manager,
            test_event_tx,
        }
    }

    /// Register a new, globally-unique payment.
    pub fn new_payment(
        &self,
        payment: impl Into<Payment>,
    ) -> anyhow::Result<()> {
        let mut locked_data = self.data.lock().unwrap();
        let checked = locked_data
            .check_new_payment(payment.into())
            .context("Error handling new payment")?;

        // TODO(max): Persist the payment

        locked_data.apply_checked(checked);

        Ok(())
    }

    /// Handles a [`PaymentClaimable`] event.
    ///
    /// [`PaymentClaimable`]: lightning::util::events::Event::PaymentClaimable
    pub fn payment_claimable(
        &self,
        hash: impl Into<LxPaymentHash>,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let hash = hash.into();
        info!(%amt_msat, %hash, "Handling PaymentClaimable");

        // Extract the preimage required to claim the payment later
        let preimage = match purpose {
            PaymentPurpose::InvoicePayment {
                payment_preimage, ..
            } => payment_preimage.context(
                "We previously generated this invoice using a method \
                other than `ChannelManager::create_inbound_payment`, \
                OR LDK failed to provide the preimage back to us.",
            )?,
            PaymentPurpose::SpontaneousPayment(preimage) => preimage,
        };

        // Validate the state transition
        let mut locked_data = self.data.lock().unwrap();
        let checked = locked_data
            .check_payment_claimable(hash, amt_msat, purpose)
            .context("Error handling PaymentClaimable")?;

        // TODO(max): Persist

        // TODO(max): Persist successful; apply the state transition
        locked_data.apply_checked(checked);

        // Everything ok; claim the payment
        // TODO(max): `claim_funds` docs state that we must check that the
        // amt_msat we received matches our expectation, relevant if
        // we're receiving payment for e.g. an order of some sort.
        // Otherwise, we will have given the sender a proof-of-payment
        // when they did not fulfill the full expected payment.
        // Implement this once it becomes relevant.
        self.channel_manager.claim_funds(preimage);

        self.test_event_tx.send(TestEvent::PaymentClaimable);

        info!("Handled PaymentClaimable");
        Ok(())
    }
}

impl PaymentsData {
    /// Applies the data contained a [`CheckedPayment`] to the local state.
    // TODO(max): Change this to persisted Payment?
    fn apply_checked(&mut self, checked: CheckedPayment) {
        let payment = checked.0;
        let id = payment.id();
        match payment.status() {
            PaymentStatus::Pending => {
                self.pending.insert(id, payment);
            }
            PaymentStatus::Completed | PaymentStatus::Failed => {
                self.finalized.insert(id);
            }
        }
    }

    // We intentially take and return an owned `Payment` so that this method
    // resembles the other `check_` methods.
    fn check_new_payment(
        &self,
        payment: Payment,
    ) -> anyhow::Result<CheckedPayment> {
        // Check that this payment is indeed unique.
        let id = payment.id();
        ensure!(
            !self.pending.contains_key(&id),
            "Payment already exists: pending"
        );
        ensure!(
            !self.finalized.contains(&id),
            "Payment already exists: finalized"
        );

        // Newly created payments should *always* be pending.
        debug_assert!(matches!(payment.status(), PaymentStatus::Pending));

        // Everything ok.
        Ok(CheckedPayment(payment))
    }

    /// Checks the proposed state transition, returning the [`Payment`] to
    /// update our local state to if everything is ok.
    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<CheckedPayment> {
        let id = LxPaymentId::from(hash);

        // The PaymentClaimable docs have a note that LDK will not stop an
        // inbound payment from being paid multiple times. We should fail the
        // payment in this case because:
        // - This messes up (or significantly complicates) our accounting
        // - This likely reflects an error on the receiver's part (reusing the
        //   same invoice for multiple payments, which would allow any nodes
        //   along the first payment path to steal subsequent payments)
        // - We should not allow payments to go through, in order to teach users
        //   that this is not an acceptable way to use lightning, because it is
        //   not safe. It is not hard to imagine users developing the
        //   misconception that it is safe to reuse invoices if duplicate
        //   payments actually do succeed.
        //
        // TODO(max): If LDK implements the regeneration of PaymentClaimable
        // events upon restart, we'll need a way to differentiate between these
        // regenerated events and duplicate payments to the same invoice.
        // https://discord.com/channels/915026692102316113/978829624635195422/1085427966986690570
        ensure!(
            !self.finalized.contains(&id),
            "Payment was a duplicate, or was already finalized"
        );

        let maybe_pending_payment = self.pending.get(&id);

        let updated_payment = match (maybe_pending_payment, purpose) {
            (Some(pending_payment), purpose) => {
                // Pending payment exists; update it
                pending_payment
                    .check_payment_claimable(hash, amt_msat, purpose)?
            }
            (None, PaymentPurpose::SpontaneousPayment(preimage)) => {
                // We just got a new spontaneous payment!
                // Create the new payment.
                let preimage = LxPaymentPreimage::from(preimage);
                let isp =
                    InboundSpontaneousPayment::new(hash, preimage, amt_msat);
                let payment = Payment::from(isp);

                // Validate the new payment.
                self.check_new_payment(payment)
                    .context("Error creating new spontaneous payment")?
            }
            (None, PaymentPurpose::InvoicePayment { .. }) => {
                bail!("Tried to claim non-existent invoice payment")
            }
        };

        Ok(updated_payment)
    }
}
