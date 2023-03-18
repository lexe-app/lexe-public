use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, ensure, Context};
use lightning::util::events::PaymentPurpose;

use crate::payments::inbound::{
    InboundLightningPayment, InboundSpontaneousPayment,
};
use crate::payments::{
    LxPaymentHash, LxPaymentId, LxPaymentPreimage, Payment, PaymentStatus,
};
use crate::traits::LexePersister;

#[allow(dead_code)] // TODO(max): Remove
#[derive(Clone)]
pub struct PaymentsManager<PS: LexePersister> {
    data: Arc<Mutex<PaymentsData>>,
    persister: PS,
}

/// Methods on [`PaymentsData`] take `&mut self`, which allows reentrancy
/// without deadlocking. [`PaymentsData`] also reduces code bloat from
/// monomorphization by taking only concrete types as parameters.
struct PaymentsData {
    pending: HashMap<LxPaymentId, Payment>,
    finalized: HashSet<LxPaymentId>,
}

impl<PS: LexePersister> PaymentsManager<PS> {
    pub fn new(persister: PS) -> Self {
        // TODO(max): Take initial data in parameters
        let data = Arc::new(Mutex::new(PaymentsData {
            pending: HashMap::new(),
            finalized: HashSet::new(),
        }));

        Self { data, persister }
    }

    /// Register a new, globally-unique payment.
    pub fn new_payment(
        &self,
        payment: impl Into<Payment>,
    ) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .new_payment(payment.into())
            .context("Error handling new payment")?;

        // TODO(max): Persist the payment

        Ok(())
    }

    /// Register that we are about to claim an inbound Lightning payment.
    pub fn payment_claimable(
        &self,
        hash: impl Into<LxPaymentHash>,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        self.data
            .lock()
            .unwrap()
            .payment_claimable(hash.into(), amt_msat, purpose)
            .context("Error handling PaymentClaimable")?;

        // TODO(max): Persist

        Ok(())
    }
}

impl PaymentsData {
    pub fn new_payment(&mut self, payment: Payment) -> anyhow::Result<()> {
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

        // Insert into the map
        self.pending.insert(id, payment);

        Ok(())
    }

    pub fn payment_claimable(
        &mut self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let id = LxPaymentId::from(hash);

        ensure!(
            !self.finalized.contains(&id),
            "Payment was a duplicate, or was already finalized"
        );

        let maybe_pending_payment = self.pending.get_mut(&id);

        match (maybe_pending_payment, purpose) {
            (Some(pending_payment), purpose) => {
                // Pending payment exists; update it
                pending_payment.payment_claimable(hash, amt_msat, purpose)?
            }
            (None, PaymentPurpose::SpontaneousPayment(preimage)) => {
                // We just got a new spontaneous payment!
                // Create the new payment.
                let preimage = LxPaymentPreimage::from(preimage);
                let isp =
                    InboundSpontaneousPayment::new(hash, preimage, amt_msat);
                let payment = Payment::from(isp);
                self.new_payment(payment)
                    .context("Error creating new spontaneous payment")?;
            }
            (None, PaymentPurpose::InvoicePayment { .. }) => {
                bail!("Tried to claim non-existent invoice payment")
            }
        }

        Ok(())
    }
}
