//! App local payments db

use std::{
    collections::{btree_map::Entry, BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::PathBuf,
    str::FromStr,
    sync::Mutex,
};

use anyhow::Context;
use common::{
    api::{
        def::AppNodeRunApi,
        qs::{GetNewPayments, GetPaymentsByIds},
    },
    client::NodeClient,
    iter::IteratorExt,
    ln::payments::{BasicPayment, PaymentIndex},
};
use tracing::warn;

pub trait Vfs {
    fn read(&self, filename: &str) -> io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.read_into(filename, &mut buf)?;
        Ok(buf)
    }
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()>;

    fn read_dir(&self) -> io::Result<Vec<String>> {
        let mut filenames = Vec::new();
        self.read_dir_visitor(|filename| {
            filenames.push(filename.to_owned());
            Ok(())
        })?;
        Ok(filenames)
    }
    fn read_dir_visitor(
        &self,
        dir_visitor: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()>;

    fn write(&self, filename: &str, data: &[u8]) -> io::Result<()>;
}

pub struct FlatFileFs {
    base_dir: PathBuf,
}

impl Vfs for FlatFileFs {
    fn read_into(&self, filename: &str, buf: &mut Vec<u8>) -> io::Result<()> {
        let path = self.base_dir.join(filename);
        let mut file = fs::File::open(path)?;
        file.read_to_end(buf)?;
        Ok(())
    }

    fn read_dir_visitor(
        &self,
        mut dir_visitor: impl FnMut(&str) -> io::Result<()>,
    ) -> io::Result<()> {
        for maybe_file_entry in self.base_dir.read_dir()? {
            let file_entry = maybe_file_entry?;

            // Only visit files.
            if file_entry.file_type()?.is_file() {
                // Just skip non-UTF-8 filenames.
                if let Some(filename) = file_entry.file_name().to_str() {
                    dir_visitor(filename)?;
                }
            }
        }
        Ok(())
    }

    fn write(&self, filename: &str, data: &[u8]) -> io::Result<()> {
        // NOTE: could use `atomicwrites` crate to make this a little safer
        // against random crashes. definitely not free though; costs at
        // least 5 ms per write on Linux (while macOS just ignores fsyncs lol).
        fs::write(self.base_dir.join(filename), data)?;
        Ok(())
    }
}

pub struct PaymentDb<V> {
    vfs: V,
    state: PaymentDbState,
}

#[derive(Debug, PartialEq, Eq)]
struct PaymentDbState {
    payments: BTreeMap<PaymentIndex, BasicPayment>,
    pending: BTreeSet<PaymentIndex>,
}

// fn io_err_other<E>(err: E) -> io::Error
// where
//     E: Into<Box<dyn std::error::Error + Send + Sync>>,
// {
//     io::Error::new(io::ErrorKind::Other, err)
// }

fn io_err_invalid_data<E>(err: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::InvalidData, err)
}

impl<V: Vfs> PaymentDb<V> {
    /// Create a new empty `PaymentDb`. Does not touch disk/storage.
    pub fn empty(vfs: V) -> Self {
        Self {
            vfs,
            state: PaymentDbState::empty(),
        }
    }

    /// Read all the payments on-disk into a new `PaymentDb`.
    pub fn read(vfs: V) -> anyhow::Result<Self> {
        let state = PaymentDbState::read(&vfs)
            .context("Failed to read on-disk PaymentDb state")?;

        state.debug_assert_invariants();

        Ok(Self { vfs, state })
    }

    /// Check the integrity of the whole PaymentDb.
    ///
    /// (1.) The in-memory state should not be corrupted.
    /// (2.) The current on-disk state should match the in-memory state.
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // (1.)
        self.state.debug_assert_invariants();

        // (2.)
        let on_disk_state = PaymentDbState::read(&self.vfs)
            .expect("Failed to re-read on-disk state");
        assert_eq!(on_disk_state, self.state);
    }

    /// The most latest/newest payment that the `PaymentDb` has synced from the
    /// user node.
    fn latest_payment_index(&self) -> Option<PaymentIndex> {
        self.state
            .payments
            .last_key_value()
            .map(|(idx, _payment)| *idx)
    }

    /// Write a payment to on-disk storage. Does not update `PaymentDb` indexes
    /// or in-memory state though.
    // Making this an associated fn avoids some borrow checker issues.
    fn write_payment(vfs: &V, payment: &BasicPayment) -> io::Result<()> {
        let idx = payment.index();
        let filename = idx.to_string();
        let data =
            serde_json::to_vec(&payment).expect("Failed to serialize payment");
        vfs.write(&filename, &data)
    }

    /// Insert a batch of new payments synced from the user node.
    fn insert_new_payments(
        &mut self,
        new_payments: Vec<BasicPayment>,
    ) -> io::Result<()> {
        for new_payment in new_payments {
            self.insert_new_payment(new_payment)?;
        }

        self.debug_assert_invariants();

        Ok(())
    }

    fn insert_new_payment(
        &mut self,
        new_payment: BasicPayment,
    ) -> io::Result<()> {
        let vacant_payment_entry =
            match self.state.payments.entry(new_payment.index()) {
                Entry::Vacant(e) => e,
                Entry::Occupied(_) => panic!(
                    "PaymentDb is corrupted! A new payment from the node \
                     already in our in-memory state somehow!"
                ),
            };

        // Try to write the payment first.
        Self::write_payment(&self.vfs, &new_payment)?;

        // Update the in-memory state.
        if new_payment.is_pending() {
            let already_in_pending =
                self.state.pending.insert(new_payment.index());
            assert!(
                !already_in_pending,
                "PaymentDb is corrupted! A new payment from the noew was \
                 already in our pending payments index!"
            );
        }
        vacant_payment_entry.insert(new_payment);

        Ok(())
    }

    /// Update a batch of currently pending payments w/ updated values from the
    /// node.
    fn update_pending_payments(
        &mut self,
        pending_payments_updates: Vec<BasicPayment>,
    ) -> io::Result<()> {
        for updated_payment in pending_payments_updates {
            self.update_pending_payment(updated_payment)?;
        }

        self.debug_assert_invariants();

        Ok(())
    }

    fn update_pending_payment(
        &mut self,
        updated_payment: BasicPayment,
    ) -> io::Result<()> {
        // Get the current, pending payment.
        let mut existing_payment_entry =
            match self.state.payments.entry(updated_payment.index()) {
                Entry::Vacant(_) => panic!(
                    "PaymentDb is corrupted! We are missing a pending payment \
                     that should exist!"
                ),
                Entry::Occupied(e) => e,
            };

        // No change to payment; skip.
        if &updated_payment == existing_payment_entry.get() {
            return Ok(());
        }

        // Payment is changed; persist the updated payment to storage.
        Self::write_payment(&self.vfs, &updated_payment)?;

        // If the payment is also finalized, remove from pending payments index.
        if !updated_payment.is_pending() {
            let was_pending =
                self.state.pending.remove(&updated_payment.index());
            assert!(
                was_pending,
                "PaymentDb is corrupted! Pending payment not in index!"
            );
        }

        // Update in-memory state.
        existing_payment_entry.insert(updated_payment);

        Ok(())
    }
}

impl PaymentDbState {
    fn empty() -> Self {
        Self {
            payments: BTreeMap::new(),
            pending: BTreeSet::new(),
        }
    }

    fn read<V: Vfs>(vfs: &V) -> anyhow::Result<Self> {
        let mut buf = Vec::new();
        let mut payments: BTreeMap<PaymentIndex, BasicPayment> =
            BTreeMap::new();

        vfs.read_dir_visitor(|filename| {
            let payment_index = match PaymentIndex::from_str(filename) {
                Ok(idx) => idx,
                Err(err) => {
                    warn!(
                        "Error: unrecognized filename ('{filename}') in \
                         payments dir: {err:#}"
                    );
                    // Just skip random files in this directory.
                    return Ok(());
                }
            };

            buf.clear();
            vfs.read_into(filename, &mut buf)?;

            let payment: BasicPayment = serde_json::from_slice(&buf)
                .with_context(|| {
                    format!(
                        "Failed to deserialize payment file ('{filename}')"
                    )
                })
                .map_err(io_err_invalid_data)?;

            // Sanity check.
            if payment_index != payment.index() {
                return Err(io_err_invalid_data(
                    format!("Payment DB corruption: payment file ('{filename}') contents don't match filename??")
                ));
            }

            // Sanity check.
            assert!(
                payments.insert(payment_index, payment).is_none(),
                "VFS somehow gaves us duplicate file names??",
            );

            Ok(())
        })
        .context("Failed to read payments db, possibly corrupted?")?;

        let pending = Self::build_pending_index(&payments);

        Ok(Self { payments, pending })
    }

    fn build_pending_index(
        payments: &BTreeMap<PaymentIndex, BasicPayment>,
    ) -> BTreeSet<PaymentIndex> {
        payments
            .iter()
            .filter_map(|(idx, payment)| {
                if payment.is_pending() {
                    Some(*idx)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check the integrity of the in-memory state.
    ///
    /// (1.) The computed index of each payment value should match its BTreeMap
    ///      key.
    /// (2.) Re-computing the pending payments index should exactly match the
    ///      its current value.
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // (1.)
        for (payment_index, payment) in &self.payments {
            assert_eq!(payment_index, &payment.index());
        }

        // (2.)
        let rebuilt_pending_index = Self::build_pending_index(&self.payments);
        assert_eq!(rebuilt_pending_index, self.pending);
    }
}

/// Only fetch at most this many payments per requests.
const PAYMENT_BATCH_LIMIT: u16 = 50;

pub async fn sync_payments<V: Vfs>(
    db: &Mutex<PaymentDb<V>>,
    node: &NodeClient,
) -> anyhow::Result<()> {
    // Fetch any updates to our pending payments to see if any are finalized.
    sync_pending_payments(db, node)
        .await
        .context("Failed to sync pending payments")?;

    // Fetch any new payments made since we last synced.
    sync_new_payments(db, node)
        .await
        .context("Failed to sync new payments")?;

    Ok(())
}

/// Fetch any updates to our pending payments to see if any are finalized.
async fn sync_pending_payments<V: Vfs>(
    db: &Mutex<PaymentDb<V>>,
    node: &NodeClient,
) -> anyhow::Result<()> {
    let pending = {
        let lock = db.lock().unwrap();

        // No pending payments; nothing to do : )
        if lock.state.pending.is_empty() {
            return Ok(());
        }

        lock.state.pending.iter().cloned().collect::<Vec<_>>()
    };

    for pending_idx_batch in pending.chunks(usize::from(PAYMENT_BATCH_LIMIT)) {
        // Request the current state of all payments we believe are pending.
        let req = GetPaymentsByIds {
            ids: pending_idx_batch
                .iter()
                .map(|idx| idx.to_string())
                .collect(),
        };
        let resp_payments = node
            .get_payments_by_ids(req)
            .await
            .context("Failed to request updated pending payments from node")?;

        // Sanity check response.
        assert_eq!(
            pending_idx_batch.len(),
            resp_payments.len(),
            "Node returned less payments than we expected!"
        );
        for (pending_idx, resp_payment) in
            pending_idx_batch.iter().zip(resp_payments.iter())
        {
            assert_eq!(
                pending_idx,
                &resp_payment.index(),
                "Node returned payment with different index!"
            );
        }

        // Update the db. Changed payments are updated on-disk. Finalized
        // payments are removed from the `pending` index.
        db.lock()
            .unwrap()
            .update_pending_payments(resp_payments)
            .context(
                "PaymentDb: Failed to persist updated pending payments batch",
            )?;
    }

    Ok(())
}

/// Fetch any new payments made since we last synced.
async fn sync_new_payments<V: Vfs>(
    db: &Mutex<PaymentDb<V>>,
    node: &NodeClient,
) -> anyhow::Result<()> {
    let mut latest_payment_index = db.lock().unwrap().latest_payment_index();

    loop {
        // Fetch the next batch of new payments.
        let req = GetNewPayments {
            // Remember, this start index is _exclusive_. The payment w/ this
            // index will _NOT_ be included in the response.
            start_index: latest_payment_index,
            limit: Some(PAYMENT_BATCH_LIMIT),
        };
        let resp_payments = node
            .get_new_payments(req)
            .await
            .context("Failed to fetch new payments")?;

        // Sanity check response.
        assert!(
            resp_payments.len() <= usize::from(PAYMENT_BATCH_LIMIT),
            "Node returned too many payments"
        );
        assert!(
            resp_payments
                .iter()
                .is_strict_total_order_by_key(BasicPayment::index),
            "Node's response is not sorted or contains duplicates"
        );

        // Update `latest_payment_index`.
        match resp_payments.last() {
            Some(p) => {
                let idx = p.index();
                assert!(
                    latest_payment_index < Some(idx),
                    "Node response gave us older payments?"
                );
                latest_payment_index = Some(idx);
            }
            // No more payments, nothing to do. We are also done syncing.
            None => break,
        }
        // Appease the all mighty borrow checker.
        let resp_payments_len = resp_payments.len();

        // Update the db. Persist new payments on-disk. Add pending payments to
        // index.
        db.lock()
            .unwrap()
            .insert_new_payments(resp_payments)
            .context("Failed to insert new payments")?;

        // If the node returns less payments than our requested batch size, then
        // we are done (there are no more new payments after this batch).
        if resp_payments_len < usize::from(PAYMENT_BATCH_LIMIT) {
            break;
        }
    }

    Ok(())
}
