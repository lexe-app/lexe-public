use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{bail, ensure};
use lightning::util::events::PaymentPurpose;

use crate::payments::offchain::inbound::{
    InboundLightningPayment, InboundSpontaneousPayment,
};
use crate::payments::payment_trait::PaymentTrait;
use crate::payments::{
    LxPaymentHash, LxPaymentId, LxPaymentPreimage, Payment, PaymentStatus,
};
use crate::traits::LexePersister;

#[allow(dead_code)] // TODO(max): Remove
#[derive(Clone)]
pub struct PaymentsManager<PS: LexePersister> {
    pending: Arc<Mutex<HashMap<LxPaymentId, Payment>>>,
    finalized: Arc<Mutex<HashSet<LxPaymentId>>>,
    persister: PS,
}

impl<PS: LexePersister> PaymentsManager<PS> {
    pub fn new(persister: PS) -> Self {
        // TODO(max): Take these as params
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let finalized = Arc::new(Mutex::new(HashSet::new()));

        Self {
            pending,
            finalized,
            persister,
        }
    }

    /// Register a new, globally-unique payment.
    pub fn new_payment<P: Into<Payment>>(
        &self,
        payment: P,
    ) -> anyhow::Result<()> {
        let payment = payment.into();
        let mut locked_pending = self.pending.lock().unwrap();
        let locked_finalized = self.finalized.lock().unwrap();

        // Check that this payment is indeed unique.
        let id = payment.id();
        ensure!(
            !locked_pending.contains_key(&id),
            "Payment already exists: pending"
        );
        ensure!(
            !locked_finalized.contains(&id),
            "Payment already exists: finalized"
        );

        // Newly created payments should *always* be pending.
        debug_assert!(matches!(payment.status(), PaymentStatus::Pending));

        // Insert into the map
        locked_pending.insert(id, payment);

        // TODO(max): Persist the payment

        Ok(())
    }

    /// Register that we are about to claim an inbound Lightning payment.
    pub fn payment_claimable(
        &self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let mut locked_pending = self.pending.lock().unwrap();
        let locked_finalized = self.finalized.lock().unwrap();
        let id = LxPaymentId::from(hash);

        ensure!(
            !locked_finalized.contains(&id),
            "Payment was a duplicate, or was already finalized"
        );

        let maybe_pending_payment = locked_pending.get_mut(&id);

        match (maybe_pending_payment, purpose) {
            (Some(pending_payment), purpose) => {
                // Pending payment exists; update it
                pending_payment.payment_claimable(hash, amt_msat, purpose)?
            }
            (None, PaymentPurpose::SpontaneousPayment(preimage)) => {
                // We just got a new spontaneous payment!
                // Create the new payment and insert it into our hashmap.
                let preimage = LxPaymentPreimage::from(preimage);
                let isp =
                    InboundSpontaneousPayment::new(hash, preimage, amt_msat);
                let payment = Payment::from(isp);
                // TODO(max): Should we be calling into Self::new_payment here?
                // How best to design the API to prevent deadlocks?
                // Maybe an inner struct with methods that take &mut self?
                locked_pending.insert(id, payment);
            }
            (None, PaymentPurpose::InvoicePayment { .. }) => {
                bail!("Tried to claim non-existent invoice payment")
            }
        }

        // TODO(max): Persist

        Ok(())
    }
}
