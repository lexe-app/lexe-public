//! App-local payments db and payment sync
//!
//! ### [`PaymentsDb`]
//!
//! The app's [`PaymentsDb`] maintains a local copy of all [`BasicPaymentV2`]s,
//! synced from the user node. The user nodes are the source-of-truth for
//! payment state; consequently, this payment db is effectively a projection of
//! the user node's payment state.
//!
//! Currently the [`BasicPaymentV2`]s in the [`PaymentsDb`] are just dumped into
//! a subdirectory of the app's data directory as unencrypted json blobs. On
//! startup, we just load all on-disk [`BasicPaymentV2`]s into memory.
//!
//! In the future, this could be a SQLite DB or something.
//!
//! ### Payment Syncing
//!
//! Syncing payments is done by tailing payments from the user node by
//! [`PaymentUpdatedIndex`]. We simply keep track of the latest updated at index
//! in our DB, and fetch batches of updated payments, merging them into our DB,
//! until no payments are returned.
//!
//! ## Design goals
//!
//! We want efficient random access for pending payments, which are always
//! displayed, and efficient fetching for the last N finalized payments, which
//! are the first payments displayed in the finalized payments list.
//!
//! Cursors don't work well with flutter's lazy lists, since they want random
//! access from scroll index -> content.
//!
//! Performance when syncing payments is secondary, since that's done
//! asynchronously and off the main UI thread, which is highly latency
//! sensitive.
//!
//! In the future, we could be even more clever and serialize+store the pending
//! index on-disk. If we also added a finalized index, we could avoid the need
//! to load all the payments on startup and could instead lazy load them.
//!
//! [`BasicPaymentV2`]: lexe_api::types::payments::BasicPaymentV2
//! [`PaymentsDb`]: crate::payments_db::PaymentsDb
//! [`PaymentUpdatedIndex`]: lexe_api::types::payments::PaymentUpdatedIndex

use std::{
    cmp,
    collections::{BTreeMap, BTreeSet},
    io,
    str::FromStr,
    sync::Mutex,
};

use anyhow::Context;
#[cfg(doc)]
use lexe_api::types::payments::VecDbPaymentV2;
use lexe_api::{
    def::AppNodeRunApi,
    error::NodeApiError,
    models::command::{GetUpdatedPayments, UpdatePaymentNote},
    types::payments::{
        BasicPaymentV2, PaymentCreatedIndex, PaymentUpdatedIndex,
        VecBasicPaymentV2,
    },
};
use node_client::client::NodeClient;
use tracing::warn;

use crate::ffs::Ffs;

/// Sync the app's local payment state from the user node.
///
/// We tail updated payments from the user node, merging results into our DB
/// until there are no more updates left to sync.
#[allow(private_bounds)]
pub async fn sync_payments<F: Ffs>(
    db: &Mutex<PaymentsDb<F>>,
    node_client: &impl AppNodeRunSyncApi,
    batch_size: u16,
) -> anyhow::Result<PaymentSyncSummary> {
    assert!(batch_size > 0);

    let mut start_index = db.lock().unwrap().state.latest_updated_index;

    let mut summary = PaymentSyncSummary {
        num_new: 0,
        num_updated: 0,
    };

    loop {
        // In every loop iteration, we fetch one batch of updated payments.

        let req = GetUpdatedPayments {
            // Remember, this start index is _exclusive_.
            // The payment w/ this index will _NOT_ be included in the response.
            start_index,
            limit: Some(batch_size),
        };

        let updated_payments = node_client
            .get_updated_payments(req)
            .await
            .context("Failed to fetch updated payments")?
            .payments;
        let updated_payments_len = updated_payments.len();

        {
            let mut locked_db = db.lock().unwrap();

            // Update the db: persist, then update in-memory state.
            let (new, updated) = locked_db
                .upsert_payments(updated_payments)
                .context("Failed to upsert payments")?;
            summary.num_new += new;
            summary.num_updated += updated;

            // Update the `start_index` we'll use for the next batch.
            start_index = locked_db.state.latest_updated_index;
        }

        // If the node returned fewer payments than our requested batch size,
        // then we are done (there are no more new payments after this batch).
        if updated_payments_len < usize::from(batch_size) {
            break;
        }
    }

    Ok(summary)
}

#[derive(Debug)]
pub struct PaymentSyncSummary {
    num_new: usize,
    num_updated: usize,
}

impl PaymentSyncSummary {
    /// Did any payments in the DB change in this sync?
    /// (i.e., do we need to update part of the UI?)
    pub fn any_changes(&self) -> bool {
        self.num_new > 0 || self.num_updated > 0
    }
}

/// The app's local [`BasicPaymentV2`] database, synced from the user node.
pub struct PaymentsDb<F> {
    ffs: F,
    state: PaymentsDbState,
}

/// Pure in-memory state of the [`PaymentsDb`].
#[derive(Debug, PartialEq)]
pub struct PaymentsDbState {
    /// All locally synced payments,
    /// from oldest to newest (reverse of the UI scroll order).
    payments: BTreeMap<PaymentCreatedIndex, BasicPaymentV2>,

    /// An index of currently pending payments, sorted by `created_at` index.
    //
    // For now, we only have a `pending` index, because it is sparse - there's
    // no point in indexing `finalized` as most payments are finalized. If we
    // later want filters for "offers only" or similar, we can add more.
    pending: BTreeSet<PaymentCreatedIndex>,

    /// The latest `updated_at` index of any payment in the db.
    ///
    /// Invariant:
    ///
    /// ```ignore
    /// latest_updated_index == payments.iter()
    ///     .map(|p| p.updated_index())
    ///     .max()
    /// ```
    latest_updated_index: Option<PaymentUpdatedIndex>,
}

/// The specific `AppNodeRunApi` method that we need to sync payments.
///
/// This lets us mock out the method in the tests below,
/// without also mocking out the entire `AppNodeRunApi` trait.
trait AppNodeRunSyncApi {
    /// GET /node/v1/payments/updated [`GetUpdatedPayments`]
    ///                            -> [`VecDbPaymentV2`]
    async fn get_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> Result<VecBasicPaymentV2, NodeApiError>;
}

impl AppNodeRunSyncApi for NodeClient {
    async fn get_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> Result<VecBasicPaymentV2, NodeApiError> {
        AppNodeRunApi::get_updated_payments(self, req).await
    }
}

impl<F: Ffs> PaymentsDb<F> {
    /// Read all the payments on-disk into a new `PaymentsDb`.
    pub fn read(ffs: F) -> anyhow::Result<Self> {
        let state = PaymentsDbState::read(&ffs)
            .context("Failed to read on-disk PaymentsDb state")?;

        Ok(Self { ffs, state })
    }

    /// Create a new empty `PaymentsDb`. Does not touch disk/storage.
    pub fn empty(ffs: F) -> Self {
        Self {
            ffs,
            state: PaymentsDbState::empty(),
        }
    }

    /// Clear the in-memory state and delete the on-disk payment db.
    pub fn delete(&mut self) -> io::Result<()> {
        self.state = PaymentsDbState::empty();
        self.ffs.delete_all()
    }

    #[inline]
    pub fn state(&self) -> &PaymentsDbState {
        &self.state
    }

    /// Upsert a batch of payments synced from the user node.
    ///
    /// Returns the number of new and updated payments, respectively.
    /// May return `(0, 0)` if nothing was inserted or updated.
    fn upsert_payments(
        &mut self,
        payments: impl IntoIterator<Item = BasicPaymentV2>,
    ) -> io::Result<(usize, usize)> {
        let mut num_new = 0;
        let mut num_updated = 0;
        for payment in payments {
            let (new, updated) = self.upsert_payment(payment)?;
            num_new += new;
            num_updated += updated;
        }
        Ok((num_new, num_updated))
    }

    /// Upserts a payment into the db.
    ///
    /// Persists the payment and updates in-memory state, including indices.
    ///
    /// Returns the number of new and updated payments, respectively.
    /// May return `(0, 0)` if nothing was inserted or updated.
    fn upsert_payment(
        &mut self,
        payment: BasicPaymentV2,
    ) -> io::Result<(usize, usize)> {
        let created_index = payment.created_index();

        let maybe_existing = self.state.payments.get(&created_index);
        let already_existed = maybe_existing.is_some();

        // Skip if payment exists and there is no change to it.
        if let Some(existing) = maybe_existing
            && payment == *existing
        {
            return Ok((0, 0));
        }

        // Persist the updated payment to storage. Since this is fallible, we
        // do this first, otherwise we may corrupt our in-memory state.
        Self::write_payment(&self.ffs, &payment)?;

        // --- 'Commit' by updating our in-memory state --- //

        // Update indices first to avoid a clone
        if payment.is_pending() {
            self.state.pending.insert(created_index);
        } else {
            self.state.pending.remove(&created_index);
        }
        // It is always true that `None < Some(_)`
        self.state.latest_updated_index = cmp::max(
            self.state.latest_updated_index,
            Some(payment.updated_index()),
        );

        // Update main payments map
        self.state.payments.insert(created_index, payment);

        if already_existed {
            Ok((0, 1))
        } else {
            Ok((1, 0))
        }
    }

    pub fn update_payment_note(
        &mut self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        let payment = self
            .state
            .get_mut_payment_by_created_index(&req.index)
            .context("Updating non-existent payment")?;

        payment.note = req.note;

        Self::write_payment(&self.ffs, payment)
            .context("Failed to write payment to local db")?;

        Ok(())
    }

    /// Write a payment to on-disk storage as JSON bytes. The caller is
    /// responsible for updating the in-memory [`PaymentsDb`] state and indices.
    // Making this an associated fn avoids some borrow checker issues.
    fn write_payment(ffs: &F, payment: &BasicPaymentV2) -> io::Result<()> {
        let filename = payment.created_index().to_string();
        let data =
            serde_json::to_vec(&payment).expect("Failed to serialize payment");
        ffs.write(&filename, &data)
    }

    /// Check the integrity of the whole PaymentsDb.
    ///
    /// (1.) The in-memory state should not be corrupted.
    /// (2.) The current on-disk state should match the in-memory state.
    #[cfg(test)]
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // (1.)
        self.state.debug_assert_invariants();

        // (2.)
        let on_disk_state = PaymentsDbState::read(&self.ffs)
            .expect("Failed to re-read on-disk state");
        assert_eq!(on_disk_state, self.state);
    }
}

impl PaymentsDbState {
    /// Create a new empty [`PaymentsDbState`]. Does not touch disk/storage.
    fn empty() -> Self {
        Self {
            payments: BTreeMap::new(),
            pending: BTreeSet::new(),
            latest_updated_index: None,
        }
    }

    /// Read the DB state from disk.
    fn read(ffs: &impl Ffs) -> anyhow::Result<Self> {
        let mut buf = Vec::<u8>::new();
        let mut payments = Vec::<BasicPaymentV2>::new();

        ffs.read_dir_visitor(|filename| {
            // Parse created_at index from filename; skip unrecognized files.
            let created_index = match PaymentCreatedIndex::from_str(filename) {
                Ok(idx) => idx,
                Err(e) => {
                    warn!(
                        %filename,
                        "Error: unrecognized filename in payments dir: {e:#}"
                    );
                    return Ok(());
                }
            };

            // Read payment into buffer
            buf.clear();
            ffs.read_into(filename, &mut buf)?;

            // Deserialize payment
            let payment = serde_json::from_slice::<BasicPaymentV2>(&buf)
                .with_context(|| filename.to_owned())
                .context("Failed to deserialize payment file")
                .map_err(io_error_invalid_data)?;

            // Sanity check: Index in filename should match index in payment.
            let payment_created_index = payment.created_index();
            if created_index != payment_created_index {
                return Err(io_error_invalid_data(format!(
                    "Payment DB corruption: filename index '{filename}'
                     different from index in contents '{payment_created_index}'"
                )));
            }

            // Collect the payment
            payments.push(payment);

            Ok(())
        })
        .context("Failed to read payments db, possibly corrupted?")?;

        Ok(Self::from_vec(payments))
    }

    pub(super) fn from_vec(payments: Vec<BasicPaymentV2>) -> Self {
        let payments = payments
            .into_iter()
            .map(|p| (p.created_index(), p))
            .collect();

        let pending = build_index::pending(&payments);
        let latest_updated_index = build_index::latest_updated_index(&payments);

        Self {
            payments,
            pending,
            latest_updated_index,
        }
    }

    pub fn num_payments(&self) -> usize {
        self.payments.len()
    }

    pub fn num_pending(&self) -> usize {
        self.pending.len()
    }

    pub fn num_finalized(&self) -> usize {
        self.payments.len() - self.pending.len()
    }

    pub fn num_pending_not_junk(&self) -> usize {
        self.pending
            .iter()
            .filter_map(|created_idx| self.payments.get(created_idx))
            .filter(|p| p.is_pending_not_junk())
            .count()
    }

    // TODO(max): If needed, we can add an index for this.
    pub fn num_finalized_not_junk(&self) -> usize {
        self.payments
            .values()
            .filter(|p| p.is_finalized_not_junk())
            .count()
    }

    pub fn latest_updated_index(&self) -> Option<PaymentUpdatedIndex> {
        self.latest_updated_index
    }

    /// Get a payment by its `PaymentCreatedIndex`.
    pub fn get_payment_by_created_index(
        &self,
        created_index: &PaymentCreatedIndex,
    ) -> Option<&BasicPaymentV2> {
        self.payments.get(created_index)
    }

    /// Get a mutable payment by its `PaymentCreatedIndex`.
    pub fn get_mut_payment_by_created_index(
        &mut self,
        created_index: &PaymentCreatedIndex,
    ) -> Option<&mut BasicPaymentV2> {
        self.payments.get_mut(created_index)
    }

    /// Get a payment by scroll index in UI order (newest to oldest).
    pub fn get_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<&BasicPaymentV2> {
        self.payments.values().nth_back(scroll_idx)
    }

    /// Get a payment by scroll index in UI order (newest to oldest).
    pub fn get_pending_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<&BasicPaymentV2> {
        self.pending
            .iter()
            .nth_back(scroll_idx)
            .and_then(|created_idx| self.payments.get(created_idx))
    }

    /// Get a "pending and not junk" payment by scroll index in UI order
    /// (newest to oldest).
    pub fn get_pending_not_junk_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<&BasicPaymentV2> {
        self.pending
            .iter()
            .rev()
            .filter_map(|created_idx| self.payments.get(created_idx))
            .filter(|p| p.is_pending_not_junk())
            .nth_back(scroll_idx)
    }

    /// Get a completed or failed payment by scroll index in UI order
    /// (newest to oldest).
    ///
    /// Performance is O(n) as scroll_idx approaches the total l# of payments.
    pub fn get_finalized_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<&BasicPaymentV2> {
        self.payments
            .values()
            .filter(|p| p.is_finalized())
            .nth_back(scroll_idx)
    }

    /// Get a completed or failed, not junk payment by scroll index in UI order
    /// (newest to oldest). scroll index here is also the "reverse" rank of all
    /// finalized payments. Also return the stable `vec_idx` to lookup this
    /// payment again.
    pub fn get_finalized_not_junk_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<&BasicPaymentV2> {
        self.payments
            .values()
            .filter(|p| p.is_finalized_not_junk())
            .nth_back(scroll_idx)
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.payments.is_empty()
    }

    /// Check the integrity of the in-memory state.
    #[cfg(test)]
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // --- `payments` invariants: --- //

        // Each payment is stored under its own created_at index
        for (idx, payment) in &self.payments {
            assert_eq!(*idx, payment.created_index());
        }

        // --- `pending` index invariants: --- //

        // Rebuilding the index recreates the same index exactly
        let rebuilt_pending_index = build_index::pending(&self.payments);
        assert_eq!(rebuilt_pending_index, self.pending);

        // All pending payments are in the index
        // (in case there is a bug in `index::build_pending`)
        self.payments
            .values()
            .filter(|p| p.is_pending())
            .all(|p| self.pending.contains(&p.created_index()));

        // --- `latest_updated_index` invariant: --- //

        let recomputed_latest_updated_index =
            build_index::latest_updated_index(&self.payments);
        assert_eq!(recomputed_latest_updated_index, self.latest_updated_index);
    }
}

mod build_index {
    use super::*;

    /// Build the `pending` index from the given payments.
    pub(super) fn pending(
        payments: &BTreeMap<PaymentCreatedIndex, BasicPaymentV2>,
    ) -> BTreeSet<PaymentCreatedIndex> {
        payments
            .iter()
            .filter(|(_, p)| p.is_pending())
            .map(|(idx, _)| *idx)
            .collect()
    }

    /// Find the latest [`PaymentUpdatedIndex`] from the given payments.
    pub(super) fn latest_updated_index(
        payments: &BTreeMap<PaymentCreatedIndex, BasicPaymentV2>,
    ) -> Option<PaymentUpdatedIndex> {
        payments.values().map(BasicPaymentV2::updated_index).max()
    }
}

/// Construct an [`io::Error`] of kind `InvalidData` from the given error.
fn io_error_invalid_data(
    error: impl Into<Box<dyn std::error::Error + Send + Sync>>,
) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod test_utils {
    use proptest::{
        prelude::{Strategy, any},
        sample::SizeRange,
    };

    use super::*;

    pub(super) struct MockNode {
        pub payments: BTreeMap<PaymentUpdatedIndex, BasicPaymentV2>,
    }

    impl MockNode {
        /// Construct from a map of updated_at index -> payment.
        pub(super) fn new(
            payments: BTreeMap<PaymentUpdatedIndex, BasicPaymentV2>,
        ) -> Self {
            Self { payments }
        }

        /// Construct from a map of created_at index -> payment.
        pub(super) fn from_payments(
            payments: BTreeMap<PaymentCreatedIndex, BasicPaymentV2>,
        ) -> Self {
            let payments = payments
                .into_values()
                .map(|p| (p.updated_index(), p))
                .collect();
            Self { payments }
        }
    }

    impl AppNodeRunSyncApi for MockNode {
        /// GET /node/v1/payments/updated [`GetUpdatedPayments`]
        ///                            -> [`VecDbPaymentV2`]
        async fn get_updated_payments(
            &self,
            req: GetUpdatedPayments,
        ) -> Result<VecBasicPaymentV2, NodeApiError> {
            let limit = req.limit.unwrap_or(u16::MAX);

            let payments = match req.start_index {
                Some(start_index) => self
                    .payments
                    .iter()
                    .filter(|(idx, _)| &start_index < *idx)
                    .take(limit as usize)
                    .map(|(_idx, payment)| payment.clone())
                    .collect(),
                None => self
                    .payments
                    .iter()
                    .take(limit as usize)
                    .map(|(_idx, payment)| payment.clone())
                    .collect(),
            };

            Ok(VecBasicPaymentV2 { payments })
        }
    }

    pub(super) fn any_payments(
        approx_size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = BTreeMap<PaymentCreatedIndex, BasicPaymentV2>>
    {
        proptest::collection::vec(any::<BasicPaymentV2>(), approx_size)
            .prop_map(|payments| {
                payments
                    .into_iter()
                    .map(|payment| (payment.created_index(), payment))
                    .collect::<BTreeMap<_, _>>()
            })
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashSet, time::Duration};

    use common::rng::{FastRng, Rng, RngExt};
    use lexe_api::types::payments::PaymentStatus;
    use proptest::{
        collection::vec, prelude::any, proptest, sample::Index,
        test_runner::Config,
    };
    use tempfile::tempdir;

    use super::{test_utils::MockNode, *};
    use crate::ffs::{FlatFileFs, test::MockFfs};

    #[test]
    fn read_from_empty() {
        let mock_ffs = MockFfs::new();
        let mock_ffs_db = PaymentsDb::read(mock_ffs).unwrap();
        assert!(mock_ffs_db.state.is_empty());
        mock_ffs_db.debug_assert_invariants();

        let tempdir = tempfile::tempdir().unwrap();
        let temp_fs =
            FlatFileFs::create_dir_all(tempdir.path().to_path_buf()).unwrap();
        let temp_fs_db = PaymentsDb::read(temp_fs).unwrap();
        assert!(temp_fs_db.state.is_empty());
        temp_fs_db.debug_assert_invariants();

        assert_eq!(mock_ffs_db.state, temp_fs_db.state);
    }

    #[test]
    fn test_upsert() {
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

        proptest!(
            Config::with_cases(10),
            |(
                rng: FastRng,
                payments in test_utils::any_payments(0..20),
                batch_sizes in vec(1_usize..20, 0..5),
            )| {
                let tempdir = tempdir().unwrap();
                let temp_fs = FlatFileFs::create_dir_all(tempdir.path().to_path_buf()).unwrap();
                let mut temp_fs_db = PaymentsDb::empty(temp_fs);

                let mock_ffs = MockFfs::from_rng(rng);
                let mut mock_ffs_db = PaymentsDb::empty(mock_ffs);

                let mut payments_iter = payments.clone().into_values();
                visit_batches(&mut payments_iter, batch_sizes, |new_payment_batch| {
                    let _ = mock_ffs_db.upsert_payments(
                        new_payment_batch.clone()
                    ).unwrap();
                    let _ = temp_fs_db.upsert_payments(new_payment_batch).unwrap();

                    mock_ffs_db.debug_assert_invariants();
                    temp_fs_db.debug_assert_invariants();
                });

                assert_eq!(mock_ffs_db.state, temp_fs_db.state);
            }
        );
    }

    #[tokio::test]
    async fn test_sync_empty() {
        let mock_node_client = MockNode::new(BTreeMap::new());
        let mock_ffs = MockFfs::new();
        let db = Mutex::new(PaymentsDb::empty(mock_ffs));

        sync_payments(&db, &mock_node_client, 5).await.unwrap();

        assert!(db.lock().unwrap().state.is_empty());
        db.lock().unwrap().debug_assert_invariants();
    }

    #[test]
    fn test_sync() {
        /// Assert that the payments in the db equal those in the mock node.
        fn assert_db_payments_eq(
            db_payments: &BTreeMap<PaymentCreatedIndex, BasicPaymentV2>,
            node_payments: &BTreeMap<PaymentUpdatedIndex, BasicPaymentV2>,
        ) {
            assert_eq!(db_payments.len(), node_payments.len());
            db_payments.iter().for_each(|(_created_idx, payment)| {
                let node_payment =
                    node_payments.get(&payment.updated_index()).unwrap();
                assert_eq!(payment, node_payment);
            });
            node_payments.iter().for_each(|(_updated_idx, payment)| {
                let db_payment =
                    db_payments.get(&payment.created_index()).unwrap();
                assert_eq!(payment, db_payment);
            });
        }

        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();

        proptest!(
            Config::with_cases(4),
            |(
                mut rng: FastRng,
                payments in test_utils::any_payments(1..20),
                req_batch_size in 1_u16..5,
                finalize_indexes in
                    proptest::collection::vec(any::<Index>(), 1..5),
            )| {
                let mut mock_node = MockNode::from_payments(payments);

                let mut rng2 = FastRng::from_u64(rng.gen_u64());
                let mock_ffs = MockFfs::from_rng(rng);

                // Sync empty DB from node
                let db = Mutex::new(PaymentsDb::empty(mock_ffs));
                rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                    .unwrap();
                assert_db_payments_eq(
                    &db.lock().unwrap().state.payments,
                    &mock_node.payments,
                );
                db.lock().unwrap().debug_assert_invariants();

                // Reread db from ffs and resync - should still match node
                let mock_ffs = db.into_inner().unwrap().ffs;
                let db = Mutex::new(PaymentsDb::read(mock_ffs).unwrap());
                rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                    .unwrap();
                assert_db_payments_eq(
                    &db.lock().unwrap().state.payments,
                    &mock_node.payments,
                );
                db.lock().unwrap().debug_assert_invariants();

                // Finalize some payments
                let finalize_some_payments = || {
                    let pending_payments = mock_node
                        .payments
                        .values()
                        .filter(|p| p.is_pending())
                        .cloned()
                        .collect::<Vec<_>>();

                    if pending_payments.is_empty() {
                        return;
                    }

                    // Simulates the current time
                    //
                    // We bump it before every update to ensure finalized
                    // payments have a later updated_at than in our DB
                    let mut current_time = db
                        .lock()
                        .unwrap()
                        .state
                        .latest_updated_index
                        .expect("DB should have payments")
                        .updated_at;

                    // The array indices of the payments inside
                    // `pending_payments` to finalize
                    let finalize_idxs = finalize_indexes
                        .into_iter()
                        .map(|index| index.index(pending_payments.len()))
                        // Collect into HashSet to dedup without sorting
                        .collect::<HashSet<_>>();

                    for finalize_idx in finalize_idxs {
                        // The updated_at index of the payment to finalize
                        let final_updated_idx =
                            pending_payments[finalize_idx].updated_index();

                        // Remove the payment to finalize from map
                        let mut payment = mock_node
                            .payments
                            .remove(&final_updated_idx)
                            .unwrap();

                        // The finalized status to set for this payment
                        let new_status = if rng2.gen_boolean() {
                            PaymentStatus::Completed
                        } else {
                            PaymentStatus::Failed
                        };

                        // Bump the current time so new updated_at is fresh
                        let bump_u64 = rng2.gen_range(1..=10);
                        let bump_dur = Duration::from_millis(bump_u64);
                        current_time = current_time.saturating_add(bump_dur);

                        // Update payment
                        payment.status = new_status;
                        payment.updated_at = current_time;

                        // Re-insert with new updated_at as the key
                        let new_updated_index = payment.updated_index();
                        mock_node
                            .payments
                            .insert(new_updated_index, payment);
                        }
                };

                finalize_some_payments();

                // resync -- should pick up the finalized payments
                rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                    .unwrap();

                let db_lock = db.lock().unwrap();
                assert_db_payments_eq(
                    &db_lock.state.payments,
                    &mock_node.payments,
                );
                db_lock.debug_assert_invariants();
            }
        );
    }
}
