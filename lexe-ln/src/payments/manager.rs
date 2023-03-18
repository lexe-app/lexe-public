use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use crate::payments::{LxPaymentId, Payment};
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
}
