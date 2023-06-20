//! App-local payments db and payment sync
//!
//! ### [`PaymentDb`]
//!
//! The app's [`PaymentDb`] maintains a local copy of all [`BasicPayment`]s,
//! synced from the user node. The user nodes are the source-of-truth for
//! payment state; consequently, this payment db is effectively a projection of
//! the user node's payment state.
//!
//! Currently the [`BasicPayment`]s in the [`PaymentDb`] are just dumped into
//! a subdirectory of the app's data directory as unencrypted json blobs. On
//! startup, we just load all on-disk [`BasicPayment`]s into memory.
//!
//! In the future, this could be a SQLite DB or something.
//!
//! ### Payment Syncing
//!
//! Syncing payments from the user node is done in two steps:
//!
//! 1. For every pending payment in our db, we request an update from the user
//!    node to see if that pending payment has finalized (either successfully or
//!    unsuccessfully).
//! 2. We then request, in order, any new payments made since our last sync.
//!
//! [`BasicPayment`]: common::ln::payments::BasicPayment
//! [`PaymentDb`]: crate::payments::PaymentDb

use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use anyhow::{format_err, Context};
use common::{
    api::{
        def::AppNodeRunApi,
        qs::{GetNewPayments, GetPaymentsByIds},
    },
    iter::IteratorExt,
    ln::payments::{BasicPayment, PaymentIndex},
};
use tracing::warn;

/// The app's local [`BasicPayment`] database, synced from the user node.
pub struct PaymentDb<V> {
    vfs: V,
    state: PaymentDbState,
}

/// Pure in-memory state of the [`PaymentDb`].
#[derive(Debug, PartialEq, Eq)]
struct PaymentDbState {
    // All locally synced payments.
    //
    // Sorted from oldest to newest (reverse of the UI scroll order).
    //
    // * The `BasicPayment`s are wrapped in `Arc<_>` so we can freely send them
    //   to the flutter UI.
    payments: Vec<Arc<BasicPayment>>,

    // An index of currently pending payments.
    pending: BTreeSet<PaymentIndex>,
}

#[derive(Debug)]
pub struct PaymentSyncSummary {
    num_updated: usize,
    num_new: usize,
}

/// Abstraction over a flat file system, suitable for mocking.
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

/// File system impl for [`Vfs`] that does real IO.
pub struct FlatFileFs {
    base_dir: PathBuf,
}

// -- impl FlatFileFs -- //

impl FlatFileFs {
    fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Create a new `FlatFileFs` ready for use.
    ///
    /// Normally, it's expected that this directory already exists. In case that
    /// directory doesn't exist, this fn will create `base_dir` and any parent
    /// directories.
    pub fn create_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        fs::create_dir_all(&base_dir).with_context(|| {
            format!("Failed to create directory ({})", base_dir.display())
        })?;
        Ok(Self::new(base_dir))
    }

    /// Create a new `FlatFileFs` at `base_dir`, but clean any existing files
    /// first.
    pub fn create_clean_dir_all(base_dir: PathBuf) -> anyhow::Result<Self> {
        // Clean up any existing directory, if it exists.
        if let Err(err) = fs::remove_dir_all(&base_dir) {
            match err.kind() {
                io::ErrorKind::NotFound => (),
                _ => return Err(anyhow::Error::new(err))
                    .with_context(|| {
                        format!(
                            "Something went wrong while trying to clean the directory ({})",
                            base_dir.display(),
                        )
                    }),
            }
        }

        Self::create_dir_all(base_dir)
    }
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

// -- impl PaymentDb -- //

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

    pub fn num_payments(&self) -> usize {
        self.state.payments.len()
    }

    pub fn num_pending(&self) -> usize {
        self.state.pending.len()
    }

    /// The latest/newest payment that the `PaymentDb` has synced from the user
    /// node.
    pub fn latest_payment_index(&self) -> Option<PaymentIndex> {
        self.state.payments.last().map(|payment| payment.index())
    }

    /// Get a payment by integer index in reverse order (newest to oldest). This
    /// is the same order we display payments in the UI.
    pub fn get_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<Arc<BasicPayment>> {
        // vec_idx | scroll_idx | payment timestamp
        // 0       | 2          | 23
        // 1       | 1          | 50
        // 2       | 0          | 75
        //
        // vec_idx := num_payments - scroll_idx - 1

        let num_payments = self.num_payments();
        if scroll_idx >= num_payments {
            return None;
        }

        let vec_idx = num_payments - scroll_idx - 1;
        Some(self.state.payments[vec_idx].clone())
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
    ///
    /// A new payment batch should satisfy:
    ///
    /// (1) all payments are sorted and contain no duplicates.
    /// (2) all payments are newer than any current payment in the DB.
    fn insert_new_payments(
        &mut self,
        mut new_payments: Vec<Arc<BasicPayment>>,
    ) -> io::Result<()> {
        let oldest_new_payment = match new_payments.first() {
            Some(p) => p,
            // No new payments; nothing to do.
            None => return Ok(()),
        };

        // (1)
        let not_sorted_or_unique = new_payments
            .iter()
            .is_strict_total_order_by_key(arc_payment_index);
        if !not_sorted_or_unique {
            return Err(io_err_invalid_data(
                "new payments batch is not sorted or contains duplicates",
            ));
        }

        // (2)
        let all_new_payments_are_newer =
            self.latest_payment_index() < Some(oldest_new_payment.index());
        if !all_new_payments_are_newer {
            return Err(io_err_invalid_data(
                "new payments contains older payments than db",
            ));
        }

        for new_payment in &new_payments {
            if new_payment.is_pending() {
                let not_already_in_pending =
                    self.state.pending.insert(new_payment.index());
                let already_in_pending = !not_already_in_pending;
                if already_in_pending {
                    return Err(io_err_invalid_data(
                        "new payment is somehow already in our pending index",
                    ));
                }
            }

            Self::write_payment(&self.vfs, new_payment)?;
        }

        self.state.payments.append(&mut new_payments);
        self.debug_assert_invariants();

        Ok(())
    }

    /// Update a batch of currently pending payments w/ updated values from the
    /// node.
    fn update_pending_payments(
        &mut self,
        pending_payments_updates: Vec<Arc<BasicPayment>>,
    ) -> io::Result<usize> {
        let mut num_updated = 0;

        // this could be done more efficiently. assuming the update is sorted,
        // after updating a payment, only search the _rest_ for the next
        // payment.

        for updated_payment in pending_payments_updates {
            num_updated += self.update_pending_payment(updated_payment)?;
        }

        self.debug_assert_invariants();

        Ok(num_updated)
    }

    fn update_pending_payment(
        &mut self,
        updated_payment: Arc<BasicPayment>,
    ) -> io::Result<usize> {
        let updated_payment_index = updated_payment.index();

        // Get the current, pending payment.
        let search_result = self
            .state
            .payments
            .binary_search_by_key(&updated_payment_index, arc_payment_index);
        let existing_payment = match search_result {
            Err(_) => panic!(
                "PaymentDb is corrupted! We are missing a pending payment \
                 that should exist!"
            ),
            Ok(idx) => self.state.payments.get_mut(idx).unwrap(),
        };

        // No change to payment; skip.
        if &updated_payment == existing_payment {
            return Ok(0);
        }

        // Payment is changed; persist the updated payment to storage.
        Self::write_payment(&self.vfs, &updated_payment)?;

        // If the payment is now finalized, remove from pending payments index.
        if !updated_payment.is_pending() {
            let was_pending =
                self.state.pending.remove(&updated_payment.index());
            assert!(
                was_pending,
                "PaymentDb is corrupted! Pending payment not in index!"
            );
        }

        // Update in-memory state.
        *existing_payment = updated_payment;

        Ok(1)
    }
}

// -- impl PaymentDbState -- //

fn arc_payment_index(payment: &Arc<BasicPayment>) -> PaymentIndex {
    payment.index()
}

impl PaymentDbState {
    fn empty() -> Self {
        Self {
            payments: Vec::new(),
            pending: BTreeSet::new(),
        }
    }

    fn read<V: Vfs>(vfs: &V) -> anyhow::Result<Self> {
        let mut buf: Vec<u8> = Vec::new();
        let mut payments: Vec<Arc<BasicPayment>> = Vec::new();

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

            let payment: Arc<BasicPayment> = Arc::new(serde_json::from_slice(&buf)
                .with_context(|| {
                    format!(
                        "Failed to deserialize payment file ('{filename}')"
                    )
                })
                .map_err(io_err_invalid_data)?);

            // Sanity check.
            if payment_index != payment.index() {
                return Err(io_err_invalid_data(
                    format!("Payment DB corruption: payment file ('{filename}') contents don't match filename??")
                ));
            }

            payments.push(payment);
            Ok(())
        })
        .context("Failed to read payments db, possibly corrupted?")?;

        payments.sort_unstable_by_key(arc_payment_index);

        let pending = Self::build_pending_index(&payments);

        let state = Self { payments, pending };
        state.debug_assert_invariants();
        Ok(state)
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.payments.is_empty()
    }

    fn build_pending_index(
        payments: &[Arc<BasicPayment>],
    ) -> BTreeSet<PaymentIndex> {
        payments
            .iter()
            .filter_map(|payment| {
                if payment.is_pending() {
                    Some(payment.index())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check the integrity of the in-memory state.
    ///
    /// (1.) The payments are currently sorted by `PaymentIndex` from oldest to
    ///      newest.
    /// (2.) Re-computing the pending payments index should exactly match the
    ///      its current value.
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // (1.)
        assert!(self
            .payments
            .iter()
            .is_strict_total_order_by_key(arc_payment_index));

        // (2.)
        let rebuilt_pending_index = Self::build_pending_index(&self.payments);
        assert_eq!(rebuilt_pending_index, self.pending);
    }
}

// -- PaymentDb sync -- //

impl PaymentSyncSummary {
    /// Did any payments in the DB change in this sync? (i.e., do we need to
    /// update part of the UI?)
    pub fn any_changes(&self) -> bool {
        self.num_new > 0 || self.num_updated > 0
    }
}

/// Sync the app's local payment state from the user node. Sync happens in two
/// steps:
///
/// (1.) Fetch any updates to our currently pending payments.
/// (2.) Fetch any new payments made since our last sync.
pub async fn sync_payments<V: Vfs, N: AppNodeRunApi>(
    db: &Mutex<PaymentDb<V>>,
    node: &N,
    batch_size: u16,
) -> anyhow::Result<PaymentSyncSummary> {
    assert!(batch_size > 0);

    // Fetch any updates to our pending payments to see if any are finalized.
    let num_updated = sync_pending_payments(db, node, batch_size)
        .await
        .context("Failed to sync pending payments")?;

    // Fetch any new payments made since we last synced.
    let num_new = sync_new_payments(db, node, batch_size)
        .await
        .context("Failed to sync new payments")?;

    let summary = PaymentSyncSummary {
        num_updated,
        num_new,
    };

    Ok(summary)
}

/// Fetch any updates to our pending payments to see if any are finalized.
///
/// Returns the number of payments that had were finalized or otherwise had
/// updates. Returns 0 if nothing changed with the pending payments since our
/// last sync.
async fn sync_pending_payments<V: Vfs, N: AppNodeRunApi>(
    db: &Mutex<PaymentDb<V>>,
    node: &N,
    batch_size: u16,
) -> anyhow::Result<usize> {
    let pending = {
        let lock = db.lock().unwrap();

        // No pending payments; nothing to do : )
        if lock.state.pending.is_empty() {
            return Ok(0);
        }

        lock.state.pending.iter().cloned().collect::<Vec<_>>()
    };

    let mut num_updated = 0;

    for pending_idx_batch in pending.chunks(usize::from(batch_size)) {
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

        let resp_payments =
            resp_payments.into_iter().map(Arc::new).collect::<Vec<_>>();

        // Sanity check response.
        if resp_payments.len() > pending_idx_batch.len() {
            return Err(format_err!(
                "Node returned more payments than we expected!"
            ));
        }
        // for (pending_idx, resp_payment) in
        //     pending_idx_batch.iter().zip(resp_payments.iter())
        // {
        //     assert_eq!(
        //         pending_idx,
        //         &resp_payment.index(),
        //         "Node returned payment with different index!"
        //     );
        // }

        // Update the db. Changed payments are updated on-disk. Finalized
        // payments are removed from the `pending` index.
        num_updated += db
            .lock()
            .unwrap()
            .update_pending_payments(resp_payments)
            .context(
                "PaymentDb: Failed to persist updated pending payments batch",
            )?;
    }

    Ok(num_updated)
}

/// Fetch any new payments made since we last synced.
///
/// Returns the number of new payments.
async fn sync_new_payments<V: Vfs, N: AppNodeRunApi>(
    db: &Mutex<PaymentDb<V>>,
    node: &N,
    batch_size: u16,
) -> anyhow::Result<usize> {
    let mut num_new = 0;
    let mut latest_payment_index = db.lock().unwrap().latest_payment_index();

    loop {
        // Fetch the next batch of new payments.
        let req = GetNewPayments {
            // Remember, this start index is _exclusive_. The payment w/ this
            // index will _NOT_ be included in the response.
            start_index: latest_payment_index,
            limit: Some(batch_size),
        };
        let resp_payments = node
            .get_new_payments(req)
            .await
            .context("Failed to fetch new payments")?;
        let resp_payments =
            resp_payments.into_iter().map(Arc::new).collect::<Vec<_>>();

        let resp_payments_len = resp_payments.len();
        num_new += resp_payments_len;

        // Update the db. Persist new payments on-disk. Add pending payments to
        // index.
        {
            let mut lock = db.lock().unwrap();
            lock.insert_new_payments(resp_payments)
                .context("Failed to insert new payments")?;
            latest_payment_index = lock.latest_payment_index();
        }

        // If the node returns fewer payments than our requested batch size,
        // then we are done (there are no more new payments after this batch).
        if resp_payments_len < usize::from(batch_size) {
            break;
        }
    }

    Ok(num_new)
}

// -- Tests -- //

#[cfg(test)]
mod test {
    use std::{cell::RefCell, collections::BTreeMap, ops::Bound};

    use async_trait::async_trait;
    use bitcoin::Address;
    use common::{
        api::{
            command::{
                CreateInvoiceRequest, NodeInfo, PayInvoiceRequest,
                SendOnchainRequest,
            },
            error::NodeApiError,
            qs::UpdatePaymentNote,
        },
        ln::{
            hashes::LxTxid,
            invoice::LxInvoice,
            payments::{BasicPayment, PaymentStatus},
        },
        rng::{shuffle, RngCore, WeakRng},
    };
    use proptest::{
        arbitrary::any,
        collection::vec,
        proptest,
        sample::{Index, SizeRange},
        strategy::Strategy,
    };
    use tempfile::tempdir;

    use super::*;

    fn io_err_not_found(filename: &str) -> io::Error {
        io::Error::new(io::ErrorKind::NotFound, filename)
    }

    #[derive(Debug)]
    struct MockVfs {
        inner: RefCell<MockVfsInner>,
    }

    #[derive(Debug)]
    struct MockVfsInner {
        rng: WeakRng,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl MockVfs {
        fn new() -> Self {
            Self {
                inner: RefCell::new(MockVfsInner {
                    rng: WeakRng::new(),
                    files: BTreeMap::new(),
                }),
            }
        }

        fn from_rng(rng: WeakRng) -> Self {
            Self {
                inner: RefCell::new(MockVfsInner {
                    rng,
                    files: BTreeMap::new(),
                }),
            }
        }
    }

    impl Vfs for MockVfs {
        fn read_into(
            &self,
            filename: &str,
            buf: &mut Vec<u8>,
        ) -> io::Result<()> {
            match self.inner.borrow().files.get(filename) {
                Some(data) => buf.extend_from_slice(data),
                None => return Err(io_err_not_found(filename)),
            }
            Ok(())
        }

        fn read_dir_visitor(
            &self,
            mut dir_visitor: impl FnMut(&str) -> io::Result<()>,
        ) -> io::Result<()> {
            // shuffle the file order to ensure we don't rely on it.
            let mut filenames = self
                .inner
                .borrow()
                .files
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            {
                let rng = &mut self.inner.borrow_mut().rng;
                shuffle(rng, &mut filenames);
            }

            for filename in &filenames {
                dir_visitor(filename)?;
            }
            Ok(())
        }

        fn write(&self, filename: &str, data: &[u8]) -> io::Result<()> {
            self.inner
                .borrow_mut()
                .files
                .insert(filename.to_owned(), data.to_owned());
            Ok(())
        }
    }

    struct MockNode {
        payments: BTreeMap<PaymentIndex, BasicPayment>,
    }

    impl MockNode {
        fn new(payments: BTreeMap<PaymentIndex, BasicPayment>) -> Self {
            Self { payments }
        }
    }

    #[async_trait]
    impl AppNodeRunApi for MockNode {
        // these methods are not relevant

        async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
            unimplemented!()
        }
        async fn create_invoice(
            &self,
            _req: CreateInvoiceRequest,
        ) -> Result<LxInvoice, NodeApiError> {
            unimplemented!()
        }
        async fn pay_invoice(
            &self,
            _req: PayInvoiceRequest,
        ) -> Result<(), NodeApiError> {
            unimplemented!()
        }
        async fn send_onchain(
            &self,
            _req: SendOnchainRequest,
        ) -> Result<LxTxid, NodeApiError> {
            unimplemented!()
        }
        async fn get_new_address(&self) -> Result<Address, NodeApiError> {
            unimplemented!()
        }

        // payment sync methods

        /// POST /v1/payments/ids [`GetPaymentsByIds`] -> [`Vec<DbPayment>`]
        async fn get_payments_by_ids(
            &self,
            req: GetPaymentsByIds,
        ) -> Result<Vec<BasicPayment>, NodeApiError> {
            Ok(req
                .ids
                .iter()
                .filter_map(|idx_str| {
                    let idx = PaymentIndex::from_str(idx_str).unwrap();
                    self.payments.get(&idx).cloned()
                })
                .collect())
        }

        /// GET /app/payments/new [`GetNewPayments`] -> [`Vec<BasicPayment>`]
        async fn get_new_payments(
            &self,
            req: GetNewPayments,
        ) -> Result<Vec<BasicPayment>, NodeApiError> {
            let lower_bound = match &req.start_index {
                Some(idx) => Bound::Excluded(idx),
                None => Bound::Unbounded,
            };
            let mut limit = req.limit.unwrap_or(u16::MAX);
            let mut cursor = self.payments.lower_bound(lower_bound);

            let mut out = Vec::new();
            loop {
                if limit == 0 {
                    break;
                }

                match cursor.value() {
                    Some(payment) => out.push(payment.clone()),
                    None => break,
                }

                cursor.move_next();
                limit -= 1;
            }
            Ok(out)
        }

        /// PUT /app/payments/note [`UpdatePaymentNote`] -> [`()`]
        async fn update_payment_note(
            &self,
            _req: UpdatePaymentNote,
        ) -> Result<(), NodeApiError> {
            unimplemented!()
        }
    }

    #[test]
    fn read_from_empty() {
        let mock_vfs = MockVfs::new();
        let mock_vfs_db = PaymentDb::read(mock_vfs).unwrap();
        assert!(mock_vfs_db.state.is_empty());

        let tempdir = tempdir().unwrap();
        let temp_fs = FlatFileFs::new(tempdir.path().to_path_buf());
        let temp_fs_db = PaymentDb::read(temp_fs).unwrap();
        assert!(temp_fs_db.state.is_empty());

        assert_eq!(mock_vfs_db.state, temp_fs_db.state);
    }

    fn arb_payments(
        approx_size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = BTreeMap<PaymentIndex, BasicPayment>> {
        vec(any::<BasicPayment>(), approx_size).prop_map(|payments| {
            payments
                .into_iter()
                .map(|payment| (payment.index(), payment))
                .collect::<BTreeMap<_, _>>()
        })
    }

    fn take_n<T>(iter: &mut impl Iterator<Item = T>, n: usize) -> Vec<T> {
        let mut out = Vec::with_capacity(n);

        while out.len() < n {
            match iter.next() {
                Some(value) => out.push(value),
                None => break,
            }
        }

        out
    }

    fn visit_batches<T>(
        iter: &mut impl Iterator<Item = T>,
        batch_sizes: Vec<usize>,
        mut f: impl FnMut(Vec<T>),
    ) {
        let batch_sizes = batch_sizes.into_iter();

        for batch_size in batch_sizes {
            let batch = take_n(iter, batch_size);
            let batch_len = batch.len();

            if batch_len == 0 {
                return;
            }

            f(batch);

            if batch_len < batch_size {
                return;
            }
        }

        let batch = iter.collect::<Vec<_>>();

        if !batch.is_empty() {
            f(batch);
        }
    }

    #[test]
    fn test_insert_new() {
        let config = proptest::test_runner::Config::with_cases(10);

        proptest!(config, |(
            rng: WeakRng,
            payments in arb_payments(0..20),
            batch_sizes in vec(1_usize..20, 0..5),
        )| {
            let tempdir = tempdir().unwrap();
            let temp_fs = FlatFileFs::new(tempdir.path().to_path_buf());
            let mut temp_fs_db = PaymentDb::empty(temp_fs);

            let mock_vfs = MockVfs::from_rng(rng);
            let mut mock_vfs_db = PaymentDb::empty(mock_vfs);

            let payments_arc = payments.values()
                .map(|payment| Arc::new(payment.clone()))
                .collect::<Vec<_>>();
            let mut payments_arc_iter = payments_arc.into_iter();

            visit_batches(&mut payments_arc_iter, batch_sizes, |new_payment_batch| {
                mock_vfs_db.insert_new_payments(new_payment_batch.clone()).unwrap();
                temp_fs_db.insert_new_payments(new_payment_batch).unwrap();
            });

            assert_eq!(
                mock_vfs_db.latest_payment_index(),
                payments.last_key_value().map(|(k, _v)| *k),
            );
            assert_eq!(
                temp_fs_db.latest_payment_index(),
                payments.last_key_value().map(|(k, _v)| *k),
            );

            assert_eq!(mock_vfs_db.state, temp_fs_db.state);
        });
    }

    #[tokio::test]
    async fn test_sync_empty() {
        let mock_node = MockNode::new(BTreeMap::new());
        let mock_vfs = MockVfs::new();
        let db = Mutex::new(PaymentDb::empty(mock_vfs));

        sync_payments(&db, &mock_node, 5).await.unwrap();

        assert!(db.lock().unwrap().state.is_empty());
    }

    fn assert_db_payments_eq(
        db_payments: &[Arc<BasicPayment>],
        node_payments: &BTreeMap<PaymentIndex, BasicPayment>,
    ) {
        assert_eq!(db_payments.len(), node_payments.len());

        assert!(db_payments
            .iter()
            .eq_by(node_payments.values(), |p1, p2| p1.as_ref() == p2));
    }

    #[test]
    fn test_sync() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let config = proptest::test_runner::Config::with_cases(1);

        proptest!(config, |(
            mut rng: WeakRng,
            payments in arb_payments(1..20),
            req_batch_size in 1_u16..5,
            finalize_idxs in vec(any::<Index>(), 1..5),
        )| {
            let mut mock_node = MockNode::new(payments);

            let mut rng2 = WeakRng::from_u64(rng.next_u64());
            let mock_vfs = MockVfs::from_rng(rng);
            let db = Mutex::new(PaymentDb::empty(mock_vfs));

            rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                .unwrap();

            assert_db_payments_eq(&db.lock().unwrap().state.payments, &mock_node.payments);

            // reread and resync db from vfs -- should not change

            let mock_vfs = db.into_inner().unwrap().vfs;
            let db = Mutex::new(PaymentDb::read(mock_vfs).unwrap());

            rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                .unwrap();

            assert_db_payments_eq(&db.lock().unwrap().state.payments, &mock_node.payments);

            // finalize some payments

            let pending_payments = mock_node
                .payments
                .values()
                .filter(|p| p.is_pending())
                .cloned()
                .collect::<Vec<_>>();
            let mut finalized_payments = Vec::new();

            if !pending_payments.is_empty() {
                for finalize_idx in finalize_idxs {
                    let finalize_idx = finalize_idx.index(pending_payments.len());
                    let payment = &pending_payments[finalize_idx];
                    finalized_payments.push(payment.index());
                    let new_status = if rng2.next_u32() % 2 == 0 {
                        PaymentStatus::Completed
                    } else {
                        PaymentStatus::Failed
                    };
                    mock_node
                        .payments
                        .get_mut(&payment.index())
                        .unwrap()
                        .status = new_status;
                }
            }

            // resync -- should pick up the finalized payments

            rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                .unwrap();

            let db_lock = db.lock().unwrap();
            assert_db_payments_eq(&db_lock.state.payments, &mock_node.payments);

            for finalized_payment in finalized_payments {
                assert!(!db_lock.state.pending.contains(&finalized_payment));
            }
        });
    }
}
