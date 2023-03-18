use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::ensure;

use crate::payments::{LxPaymentId, Payment, PaymentStatus, PaymentTrait};
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
}
