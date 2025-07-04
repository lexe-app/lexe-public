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
//! [`BasicPayment`]: lexe_api::types::payments::BasicPayment
//! [`PaymentDb`]: crate::payments::PaymentDb

use std::{io, str::FromStr, sync::Mutex};

use anyhow::{format_err, Context};
use lexe_api::{
    def::AppNodeRunApi,
    error::NodeApiError,
    models::command::{GetNewPayments, PaymentIndexes, UpdatePaymentNote},
    types::payments::{BasicPayment, PaymentIndex, VecBasicPayment},
};
use lexe_std::iter::IteratorExt;
use roaring::RoaringBitmap;
use tracing::{instrument, warn};

use crate::{client::NodeClient, ffs::Ffs};

/// The app's local [`BasicPayment`] database, synced from the user node.
pub struct PaymentDb<F> {
    ffs: F,
    state: PaymentDbState,
}

// We want efficient, ~O(1), random-access for pending payments and finalized
// payments (seperately, to match the primary wallet UI). Cursors don't work
// well with flutter's lazy lists, since they want random access from scroll
// index -> content.
//
// Performance when syncing payments is secondary, since that's done
// asynchronously and off the main UI thread, which is highly latency
// sensitive.
//
// All `BasicPayment`s are stored in an append-only, ordered `Vec`. (Although we
// can modify non-primary-key fields, like status or note).
//
// In the future, we could be even more clever and serialize+store the bitmap
// indexes on-disk. Then we wouldn't even need to load all the payments on
// startup and could instead lazy load them. Something to do later.
//
/// Pure in-memory state of the [`PaymentDb`].
#[derive(Debug, PartialEq)]
pub struct PaymentDbState {
    // All locally synced payments.
    //
    // Sorted from oldest to newest (reverse of the UI scroll order).
    payments: Vec<BasicPayment>,

    // An index of currently pending payments. Used during sync, when we ask
    // the node for any updates to these pending payments.
    //
    // Invariant:
    //
    // ```
    // pending.contains(vec_idx) == payments[vec_idx].is_pending()
    // ```
    pending: RoaringBitmap,

    // An index of currently pending and not junk payments (see
    // [`BasicPayment::is_junk`]). The wallet page displays these payments
    // under the "Pending" section by default.
    //
    // Invariant:
    //
    // ```
    // pending_not_junk.contains(vec_idx) == payments[vec_idx].is_pending_not_junk()
    // ```
    pending_not_junk: RoaringBitmap,

    // An index of currently finalized and not junk payments (see
    // [`BasicPayment::is_junk`]). The wallet page displays these payments
    // under the "Completed" section by default.
    //
    // Invariant:
    //
    // ```
    // finalized_not_junk.contains(vec_idx) == payments[vec_idx].is_finalized_not_junk()
    // ```
    finalized_not_junk: RoaringBitmap,
}

#[derive(Debug)]
pub struct PaymentSyncSummary {
    num_updated: usize,
    num_new: usize,
}

/// The specific API methods from [`AppNodeRunApi`] that we need to sync
/// payments.
///
/// This lets us mock these methods out in the tests below, without also mocking
/// out the entire [`AppNodeRunApi`] trait.
pub(crate) trait AppNodeRunSyncApi {
    async fn get_payments_by_indexes(
        &self,
        indexes: PaymentIndexes,
    ) -> Result<VecBasicPayment, NodeApiError>;

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<VecBasicPayment, NodeApiError>;
}

// -- impl PaymentDb -- //

fn io_err_invalid_data<E>(err: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::InvalidData, err)
}

impl<F: Ffs> PaymentDb<F> {
    /// Create a new empty `PaymentDb`. Does not touch disk/storage.
    pub fn empty(ffs: F) -> Self {
        Self {
            ffs,
            state: PaymentDbState::empty(),
        }
    }

    /// Read all the payments on-disk into a new `PaymentDb`.
    pub fn read(ffs: F) -> anyhow::Result<Self> {
        let state = PaymentDbState::read(&ffs)
            .context("Failed to read on-disk PaymentDb state")?;

        Ok(Self { ffs, state })
    }

    /// Clear the in-memory state and delete the on-disk payment db.
    pub fn delete(&mut self) -> io::Result<()> {
        self.state = PaymentDbState::empty();
        self.ffs.delete_all()
    }

    #[inline]
    pub fn state(&self) -> &PaymentDbState {
        &self.state
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
        let on_disk_state = PaymentDbState::read(&self.ffs)
            .expect("Failed to re-read on-disk state");
        assert_eq!(on_disk_state, self.state);
    }

    /// Write a payment to on-disk storage. Does not update `PaymentDb` indexes
    /// or in-memory state though.
    // Making this an associated fn avoids some borrow checker issues.
    fn write_payment(ffs: &F, payment: &BasicPayment) -> io::Result<()> {
        let idx = payment.index();
        let filename = idx.to_string();
        let data =
            serde_json::to_vec(&payment).expect("Failed to serialize payment");
        ffs.write(&filename, &data)
    }

    /// Insert a batch of new payments synced from the user node.
    ///
    /// A new payment batch should satisfy:
    ///
    /// (1) all payments are sorted and contain no duplicates.
    /// (2) all payments are newer than any current payment in the DB.
    fn insert_new_payments(
        &mut self,
        mut new_payments: Vec<BasicPayment>,
    ) -> io::Result<()> {
        let oldest_new_payment = match new_payments.first() {
            Some(p) => p,
            // No new payments; nothing to do.
            None => return Ok(()),
        };

        // (1)
        let not_sorted_or_unique = new_payments
            .iter()
            .is_strict_total_order_by_key(BasicPayment::index);
        if !not_sorted_or_unique {
            return Err(io_err_invalid_data(
                "new payments batch is not sorted or contains duplicates",
            ));
        }

        // (2)
        let all_new_payments_are_newer = self.state.latest_payment_index()
            < Some(oldest_new_payment.index());
        if !all_new_payments_are_newer {
            return Err(io_err_invalid_data(
                "new payments contains older payments than db",
            ));
        }

        let mut vec_idx = self.state.num_payments() as u32;

        for new_payment in &new_payments {
            //
            // Insert into respective indexes. Since it's a new payment, it
            // should not already exist in one of the indexes.
            //

            if new_payment.is_pending() {
                let not_already_in = self.state.pending.insert(vec_idx);
                let already_in = !not_already_in;
                if already_in {
                    return Err(io_err_invalid_data(
                        "new payment is somehow already in our pending index",
                    ));
                }
            }

            if new_payment.is_pending_not_junk() {
                let not_already_in =
                    self.state.pending_not_junk.insert(vec_idx);
                let already_in = !not_already_in;
                if already_in {
                    return Err(io_err_invalid_data(
                        "new payment is somehow already in our pending_not_junk index",
                    ));
                }
            }

            if new_payment.is_finalized_not_junk() {
                let not_already_in =
                    self.state.finalized_not_junk.insert(vec_idx);
                let already_in = !not_already_in;
                if already_in {
                    return Err(io_err_invalid_data(
                        "new payment is somehow already in our finalized_not_junk index",
                    ));
                }
            }

            // Persist payment to ffs
            Self::write_payment(&self.ffs, new_payment)?;

            vec_idx += 1;
        }

        self.state.payments.append(&mut new_payments);
        self.debug_assert_invariants();

        Ok(())
    }

    /// Update a batch of currently pending payments w/ updated values from the
    /// node.
    fn update_pending_payments(
        &mut self,
        pending_payments_updates: Vec<BasicPayment>,
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
        updated_payment: BasicPayment,
    ) -> io::Result<usize> {
        let updated_payment_index = updated_payment.index();

        //
        // Get the current, pending payment.
        //

        let vec_idx = self
            .state
            .get_vec_idx_by_payment_index(updated_payment_index)
            .expect(
                "PaymentDb is corrupted! We are missing a pending payment \
                 that should exist!",
            );
        let existing_payment = self.state.payments.get_mut(vec_idx).unwrap();

        // No change to payment; skip.
        if &updated_payment == existing_payment {
            return Ok(0);
        }

        // Payment is changed; persist the updated payment to storage.
        Self::write_payment(&self.ffs, &updated_payment)?;

        //
        // Update indexes
        //

        let vec_idx = vec_idx as u32;

        // pending -> !pending
        if existing_payment.is_pending() && !updated_payment.is_pending() {
            let was_in = self.state.pending.remove(vec_idx);
            assert!(
                was_in,
                "PaymentDb is corrupted! (pending) payment was not in index!"
            );
        }

        // pending_not_junk -> !pending_not_junk
        if existing_payment.is_pending_not_junk()
            && !updated_payment.is_pending_not_junk()
        {
            let was_in = self.state.pending_not_junk.remove(vec_idx);
            assert!(
                was_in,
                "PaymentDb is corrupted! (pending_not_junk) payment was not in index!"
            );
        }

        // !finalized_not_junk -> finalized_not_junk
        if !existing_payment.is_finalized_not_junk()
            && updated_payment.is_finalized_not_junk()
        {
            let was_not_in = self.state.finalized_not_junk.insert(vec_idx);
            assert!(
                was_not_in,
                "PaymentDb is corrupted! (finalized_not_junk) payment was already in index!"
            );
        }

        //
        // Update in-memory state.
        //

        *existing_payment = updated_payment;

        Ok(1)
    }

    pub fn update_payment_note(
        &mut self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        let vec_idx = self
            .state
            .get_vec_idx_by_payment_index(&req.index)
            .context("Updating non-existent payment")?;

        let payment = self.state.get_mut_payment_by_vec_idx(vec_idx).unwrap();
        payment.note = req.note;

        Self::write_payment(&self.ffs, payment)
            .context("Failed to write payment to local db")?;
        Ok(())
    }
}

// -- impl PaymentDbState -- //

impl PaymentDbState {
    fn empty() -> Self {
        Self {
            payments: Vec::new(),
            pending: RoaringBitmap::new(),
            pending_not_junk: RoaringBitmap::new(),
            finalized_not_junk: RoaringBitmap::new(),
        }
    }

    /// Check the integrity of the in-memory state.
    ///
    /// (1.) The payments are currently sorted by `PaymentIndex` from oldest to
    ///      newest.
    /// (2.) Re-computing the indexes should exactly match the current one.
    /// (3.) Sanity check the invariants of indexes.
    /// (4.) Some indexes are subsets of others.
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        // (1.)
        assert!(self
            .payments
            .iter()
            .is_strict_total_order_by_key(BasicPayment::index));

        // (2.)
        let rebuilt_pending_index = Self::build_pending_index(&self.payments);
        assert_eq!(rebuilt_pending_index, self.pending);

        let rebuilt_pending_not_junk_index =
            Self::build_pending_not_junk_index(&self.payments);
        assert_eq!(rebuilt_pending_not_junk_index, self.pending_not_junk);

        let rebuilt_finalized_not_junk_index =
            Self::build_finalized_not_junk_index(&self.payments);
        assert_eq!(rebuilt_finalized_not_junk_index, self.finalized_not_junk);

        // (3.)
        for (vec_idx, payment) in self.payments.iter().enumerate() {
            let vec_idx = vec_idx as u32;

            assert_eq!(payment.is_pending(), self.pending.contains(vec_idx));
            assert_eq!(
                payment.is_pending_not_junk(),
                self.pending_not_junk.contains(vec_idx),
            );
            assert_eq!(
                payment.is_finalized_not_junk(),
                self.finalized_not_junk.contains(vec_idx),
            );

            assert_eq!(
                payment.is_junk(),
                !self.pending_not_junk.contains(vec_idx)
                    && !self.finalized_not_junk.contains(vec_idx),
            );
        }

        // (4.)
        assert!(self.num_pending() >= self.num_pending_not_junk());
        assert!(self.pending_not_junk.is_subset(&self.pending));

        assert!(self.num_finalized() >= self.num_finalized_not_junk());
        // > no finalized index yet.
        // assert!(self.finalized_not_junk.is_subset(&self.finalized));
    }

    /// Build a `RoaringBitmap` index that matches the given binary `filter`.
    /// The filter returns `Some(vec_idx)` for a given [`BasicPayment`] if that
    /// payment should be in the index.
    fn build_index<F>(payments: &[BasicPayment], filter: F) -> RoaringBitmap
    where
        F: Fn((usize, &BasicPayment)) -> Option<u32>,
    {
        let iter = payments.iter().enumerate().filter_map(filter);
        RoaringBitmap::from_sorted_iter(iter).expect(
            "The indexes must be sorted, since we're iterating from 0..n",
        )
    }

    fn build_pending_index(payments: &[BasicPayment]) -> RoaringBitmap {
        Self::build_index(payments, |(vec_idx, payment)| {
            payment.is_pending().then_some(vec_idx as u32)
        })
    }

    fn build_pending_not_junk_index(
        payments: &[BasicPayment],
    ) -> RoaringBitmap {
        Self::build_index(payments, |(vec_idx, payment)| {
            payment.is_pending_not_junk().then_some(vec_idx as u32)
        })
    }

    fn build_finalized_not_junk_index(
        payments: &[BasicPayment],
    ) -> RoaringBitmap {
        Self::build_index(payments, |(vec_idx, payment)| {
            payment.is_finalized_not_junk().then_some(vec_idx as u32)
        })
    }

    /// Read the DB state from disk.
    fn read<F: Ffs>(ffs: &F) -> anyhow::Result<Self> {
        let mut buf: Vec<u8> = Vec::new();
        let mut payments: Vec<BasicPayment> = Vec::new();

        ffs.read_dir_visitor(|filename| {
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
            ffs.read_into(filename, &mut buf)?;

            let payment: BasicPayment = serde_json::from_slice(&buf)
                .with_context(|| {
                    format!(
                        "Failed to deserialize payment file ('{filename}')"
                    )
                })
                .map_err(io_err_invalid_data)?;

            // Sanity check.
            if &payment_index != payment.index() {
                return Err(io_err_invalid_data(
                    format!("Payment DB corruption: payment file ('{filename}') contents don't match filename??")
                ));
            }

            payments.push(payment);
            Ok(())
        })
        .context("Failed to read payments db, possibly corrupted?")?;

        Ok(Self::from_unsorted_vec(payments))
    }

    fn from_unsorted_vec(mut payments: Vec<BasicPayment>) -> Self {
        payments.sort_unstable_by(|x, y| x.index.cmp(&y.index));
        // dedup just to be safe : )
        payments.dedup_by(|x, y| x.index == y.index);

        let pending = Self::build_pending_index(&payments);
        let pending_not_junk = Self::build_pending_not_junk_index(&payments);
        let finalized_not_junk =
            Self::build_finalized_not_junk_index(&payments);

        let state = Self {
            payments,
            pending,
            pending_not_junk,
            finalized_not_junk,
        };

        state.debug_assert_invariants();
        state
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.payments.is_empty()
    }

    pub fn num_payments(&self) -> usize {
        self.payments.len()
    }

    pub fn num_pending(&self) -> usize {
        self.pending.len() as usize
    }

    pub fn num_finalized(&self) -> usize {
        self.num_payments() - self.num_pending()
    }

    pub fn num_pending_not_junk(&self) -> usize {
        self.pending_not_junk.len() as usize
    }

    pub fn num_finalized_not_junk(&self) -> usize {
        self.finalized_not_junk.len() as usize
    }

    /// The latest/newest payment that the `PaymentDb` has synced from the user
    /// node.
    pub fn latest_payment_index(&self) -> Option<&PaymentIndex> {
        self.payments.last().map(|payment| payment.index())
    }

    fn pending_indexes(&self) -> Vec<PaymentIndex> {
        self.pending
            .iter()
            .map(|vec_idx| self.payments[vec_idx as usize].index)
            .collect()
    }

    pub fn get_vec_idx_by_payment_index(
        &self,
        payment_index: &PaymentIndex,
    ) -> Option<usize> {
        self.payments
            .binary_search_by_key(&payment_index, BasicPayment::index)
            .ok()
    }

    /// Get a payment by its stable db `vec_idx`.
    pub fn get_payment_by_vec_idx(
        &self,
        vec_idx: usize,
    ) -> Option<&BasicPayment> {
        self.payments.get(vec_idx)
    }

    pub fn get_mut_payment_by_vec_idx(
        &mut self,
        vec_idx: usize,
    ) -> Option<&mut BasicPayment> {
        self.payments.get_mut(vec_idx)
    }

    /// Get a payment by scroll index in UI order (newest to oldest).
    /// Also return the stable `vec_idx` to lookup this payment again.
    pub fn get_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<(usize, &BasicPayment)> {
        // vec_idx | scroll_idx | payment timestamp
        // 0       | 2          | 23
        // 1       | 1          | 50
        // 2       | 0          | 75
        //
        // vec_idx := num_payments - scroll_idx - 1

        let n = self.num_payments();
        if scroll_idx >= n {
            return None;
        }

        let vec_idx = n - scroll_idx - 1;
        Some((vec_idx, &self.payments[vec_idx]))
    }

    /// Get a pending payment by scroll index in UI order (newest to oldest).
    /// Also return the stable `vec_idx` to lookup this payment again.
    pub fn get_pending_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<(usize, &BasicPayment)> {
        // early exit
        let num_pending = self.num_pending();
        if scroll_idx >= num_pending {
            return None;
        }

        // scroll_idx == reverse rank, i.e., rank in the reversed list of only
        // pending payments.
        let reverse_rank = scroll_idx;

        // since `RoaringBitmap::select` operates on normal rank, we need to
        // convert from reverse rank to normal rank.
        let rank = num_pending - reverse_rank - 1;

        // `select` returns the index of the pending payment at the given rank.
        let vec_idx = self
            .pending
            .select(rank as u32)
            .expect("We've already checked the payment index is in-bounds")
            as usize;

        Some((vec_idx, &self.payments[vec_idx]))
    }

    /// Get a completed or failed payment by scroll index in UI order
    /// (newest to oldest). scroll index here is also the "reverse" rank of all
    /// finalized payments.
    /// Also return the stable `vec_idx` to lookup this payment again.
    pub fn get_finalized_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<(usize, &BasicPayment)> {
        // early exit
        let num_finalized = self.num_finalized();
        if scroll_idx >= num_finalized {
            return None;
        }

        // BELOW: some cleverness to avoid having a second `finalized` index.

        // scroll_idx == reverse_rank, i.e., rank in the reversed list of only
        // finalized payments. This is also our initial estimate of the true
        // reverse rank. If we know how many pending payments are below our
        // estimate, then we know how many finalized payments we'll need to skip
        // to get to the true reverse rank.

        // our index in the full reversed list
        let rev_idx = self.num_payments() - scroll_idx - 1;
        // the number of pending payments at or above `rev_idx` in the full
        // reversed list.
        let num_pending_at_or_above =
            self.pending.rank(rev_idx as u32) as usize;
        // the number of pending payments below `rev_idx` in the full reversed
        // list.
        let num_pending_below = self.num_pending() - num_pending_at_or_above;

        let (vec_idx, payment) = self
            .payments
            .iter()
            .enumerate()
            .rev()
            // scroll_idx is our initial "reverse rank estimate". this would be
            // the true reverse rank if there were no pending payments below us
            // in the reverse ordering.
            .skip(scroll_idx)
            // if there are some pending payments below us, we need to correct
            // our initial estimate and skip that many finalized payments to get
            // the correct reverse rank
            .filter(|(_vec_idx, payment)| payment.is_finalized())
            .nth(num_pending_below)
            .expect("We've already checked the payment is in-bounds");

        Some((vec_idx, payment))
    }

    /// Get a pending && not junk payment by scroll index in UI order (newest to
    /// oldest). Also return the stable `vec_idx` to lookup this payment
    /// again.
    pub fn get_pending_not_junk_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<(usize, &BasicPayment)> {
        // early exit
        let n = self.num_pending_not_junk();
        if scroll_idx >= n {
            return None;
        }

        // scroll_idx == reverse rank, i.e., rank in the reversed list of only
        // pending && not junk payments.
        let reverse_rank = scroll_idx;

        // since `RoaringBitmap::select` operates on normal rank, we need to
        // convert from reverse rank to normal rank.
        let rank = n - reverse_rank - 1;

        // `select` returns the index of the pending payment at the given rank.
        let vec_idx = self
            .pending_not_junk
            .select(rank as u32)
            .expect("We've already checked the payment index is in-bounds")
            as usize;

        Some((vec_idx, &self.payments[vec_idx]))
    }

    /// Get a completed or failed, not junk payment by scroll index in UI order
    /// (newest to oldest). scroll index here is also the "reverse" rank of all
    /// finalized payments. Also return the stable `vec_idx` to lookup this
    /// payment again.
    pub fn get_finalized_not_junk_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<(usize, &BasicPayment)> {
        // early exit
        let n = self.num_finalized_not_junk();
        if scroll_idx >= n {
            return None;
        }

        // scroll_idx == reverse rank, i.e., rank in the reversed list of only
        // finalized && not junk payments.
        let reverse_rank = scroll_idx;

        // since `RoaringBitmap::select` operates on normal rank, we need to
        // convert from reverse rank to normal rank.
        let rank = n - reverse_rank - 1;

        // `select` returns the index of the finalized && not junk payment at
        // the given rank.
        let vec_idx = self
            .finalized_not_junk
            .select(rank as u32)
            .expect("We've already checked the payment index is in-bounds")
            as usize;

        Some((vec_idx, &self.payments[vec_idx]))
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
pub async fn sync_payments<F: Ffs, N: AppNodeRunSyncApi>(
    db: &Mutex<PaymentDb<F>>,
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
#[instrument(skip_all, name = "(pending)")]
async fn sync_pending_payments<F: Ffs, N: AppNodeRunSyncApi>(
    db: &Mutex<PaymentDb<F>>,
    node: &N,
    batch_size: u16,
) -> anyhow::Result<usize> {
    let pending_idxs = {
        let lock = db.lock().unwrap();

        // No pending payments; nothing to do : )
        if lock.state.pending.is_empty() {
            return Ok(0);
        }

        lock.state.pending_indexes()
    };

    let mut num_updated = 0;

    for pending_idxs_batch in pending_idxs.chunks(usize::from(batch_size)) {
        // Request the current state of all payments we believe are pending.
        let req = PaymentIndexes {
            indexes: pending_idxs_batch.to_vec(),
        };
        let resp_payments = node
            .get_payments_by_indexes(req)
            .await
            .context("Failed to request updated pending payments from node")?
            .payments;

        // Sanity check response.
        if resp_payments.len() > pending_idxs_batch.len() {
            return Err(format_err!(
                "Node returned more payments than we expected!"
            ));
        }

        // for (pending_id, resp_payment) in
        //     pending_ids_batch.iter().zip(resp_payments.iter())
        // {
        //     assert_eq!(
        //         pending_id,
        //         &resp_payment.payment_id(),
        //         "Node returned payment with different id!"
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
#[instrument(skip_all, name = "(new)")]
async fn sync_new_payments<F: Ffs, N: AppNodeRunSyncApi>(
    db: &Mutex<PaymentDb<F>>,
    node: &N,
    batch_size: u16,
) -> anyhow::Result<usize> {
    let mut num_new = 0;
    let mut latest_payment_index =
        db.lock().unwrap().state.latest_payment_index().copied();

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
            .context("Failed to fetch new payments")?
            .payments;

        let resp_payments_len = resp_payments.len();
        num_new += resp_payments_len;

        // Update the db. Persist new payments on-disk. Add pending payments to
        // index.
        {
            let mut lock = db.lock().unwrap();
            lock.insert_new_payments(resp_payments)
                .context("Failed to insert new payments")?;
            latest_payment_index = lock.state.latest_payment_index().copied();
        }

        // If the node returns fewer payments than our requested batch size,
        // then we are done (there are no more new payments after this batch).
        if resp_payments_len < usize::from(batch_size) {
            break;
        }
    }

    Ok(num_new)
}

// --- impl AppNodeRunSyncApi --- //

impl AppNodeRunSyncApi for NodeClient {
    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
    ) -> Result<VecBasicPayment, NodeApiError> {
        AppNodeRunApi::get_payments_by_indexes(self, req).await
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<VecBasicPayment, NodeApiError> {
        AppNodeRunApi::get_new_payments(self, req).await
    }
}

// -- Tests -- //

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use common::rng::{FastRng, RngExt};
    use lexe_api::{
        error::NodeApiError,
        types::payments::{PaymentStatus, VecBasicPayment},
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
    use crate::ffs::{test::MockFfs, FlatFileFs};

    struct MockNode {
        payments: BTreeMap<PaymentIndex, BasicPayment>,
    }

    impl MockNode {
        fn new(payments: BTreeMap<PaymentIndex, BasicPayment>) -> Self {
            Self { payments }
        }
    }

    impl AppNodeRunSyncApi for MockNode {
        /// POST /v1/payments/indexes [`PaymentIndexes`]
        ///                        -> [`VecDbPayment`]
        async fn get_payments_by_indexes(
            &self,
            req: PaymentIndexes,
        ) -> Result<VecBasicPayment, NodeApiError> {
            let payments = req
                .indexes
                .into_iter()
                .filter_map(|idx_i| {
                    self.payments
                        .iter()
                        .find(|(idx_j, _p)| &idx_i == *idx_j)
                        .map(|(_idx, p)| p)
                        .cloned()
                })
                .collect();
            Ok(VecBasicPayment { payments })
        }

        /// GET /app/payments/new [`GetNewPayments`] -> [`VecBasicPayment`]
        async fn get_new_payments(
            &self,
            req: GetNewPayments,
        ) -> Result<VecBasicPayment, NodeApiError> {
            let iter = match req.start_index {
                Some(idx) => {
                    // Advance the iter until we find the first key where
                    // key > req.start_index
                    let mut iter = self.payments.iter().peekable();
                    while let Some((key, _value)) = iter.peek() {
                        if *key > &idx {
                            break;
                        } else {
                            iter.next();
                        }
                    }
                    iter
                }
                // Match the other branch's return type
                None => self.payments.iter().peekable(),
            };

            let limit = req.limit.unwrap_or(u16::MAX);

            let payments = iter
                .take(limit as usize)
                .map(|(_key, value)| value.clone())
                .collect::<Vec<_>>();
            Ok(VecBasicPayment { payments })
        }
    }

    #[test]
    fn read_from_empty() {
        let mock_ffs = MockFfs::new();
        let mock_ffs_db = PaymentDb::read(mock_ffs).unwrap();
        assert!(mock_ffs_db.state.is_empty());

        let tempdir = tempdir().unwrap();
        let temp_fs =
            FlatFileFs::create_dir_all(tempdir.path().to_path_buf()).unwrap();
        let temp_fs_db = PaymentDb::read(temp_fs).unwrap();
        assert!(temp_fs_db.state.is_empty());

        assert_eq!(mock_ffs_db.state, temp_fs_db.state);
    }

    fn arb_payments(
        approx_size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = BTreeMap<PaymentIndex, BasicPayment>> {
        vec(any::<BasicPayment>(), approx_size).prop_map(|payments| {
            payments
                .into_iter()
                .map(|payment| (*payment.index(), payment))
                .collect::<BTreeMap<_, _>>()
        })
    }

    fn arb_payment_db_state(
        approx_size: impl Into<SizeRange>,
    ) -> impl Strategy<Value = PaymentDbState> {
        vec(any::<BasicPayment>(), approx_size)
            .prop_map(PaymentDbState::from_unsorted_vec)
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
            rng: FastRng,
            payments in arb_payments(0..20),
            batch_sizes in vec(1_usize..20, 0..5),
        )| {
            let tempdir = tempdir().unwrap();
            let temp_fs = FlatFileFs::create_dir_all(tempdir.path().to_path_buf()).unwrap();
            let mut temp_fs_db = PaymentDb::empty(temp_fs);

            let mock_ffs = MockFfs::from_rng(rng);
            let mut mock_ffs_db = PaymentDb::empty(mock_ffs);

            let mut payments_iter = payments.clone().into_values();
            visit_batches(&mut payments_iter, batch_sizes, |new_payment_batch| {
                mock_ffs_db.insert_new_payments(new_payment_batch.clone()).unwrap();
                temp_fs_db.insert_new_payments(new_payment_batch).unwrap();
            });

            assert_eq!(
                mock_ffs_db.state().latest_payment_index(),
                payments.last_key_value().map(|(k, _v)| k),
            );
            assert_eq!(
                temp_fs_db.state().latest_payment_index(),
                payments.last_key_value().map(|(k, _v)| k),
            );

            assert_eq!(mock_ffs_db.state, temp_fs_db.state);
        });
    }

    fn assert_get_by_scroll_idx<F1, F2>(
        db_state: &PaymentDbState,
        actual_fn: F1,
        naive_filter_fn: F2,
        scroll_idx: usize,
    ) where
        F1: Fn(&PaymentDbState, usize) -> Option<(usize, &BasicPayment)>,
        F2: Fn(&BasicPayment) -> bool,
    {
        let actual = actual_fn(db_state, scroll_idx);
        let naive = db_state
            .payments
            .iter()
            .enumerate()
            .rev()
            .filter(|(_vec_idx, payment)| naive_filter_fn(payment))
            .nth(scroll_idx);
        assert_eq!(actual, naive);
        assert_eq!(
            actual.map(|(_, payment)| payment),
            actual.and_then(
                |(vec_idx, _)| db_state.get_payment_by_vec_idx(vec_idx)
            ),
        );
    }

    #[test]
    fn test_get_payment_kinds() {
        let config = proptest::test_runner::Config::with_cases(10);

        proptest!(config, |(db_state in arb_payment_db_state(0..10))| {
            let n = db_state.num_payments();

            // include a few extra indices after `n` just to make sure we don't
            // choke on out-of-range
            for scroll_idx in 0..(n+5) {
                assert_get_by_scroll_idx(
                    &db_state,
                    PaymentDbState::get_payment_by_scroll_idx,
                    |_payment| true,
                    scroll_idx,
                );

                assert_get_by_scroll_idx(
                    &db_state,
                    PaymentDbState::get_pending_payment_by_scroll_idx,
                    BasicPayment::is_pending,
                    scroll_idx,
                );

                assert_get_by_scroll_idx(
                    &db_state,
                    PaymentDbState::get_pending_not_junk_payment_by_scroll_idx,
                    BasicPayment::is_pending_not_junk,
                    scroll_idx,
                );

                assert_get_by_scroll_idx(
                    &db_state,
                    PaymentDbState::get_finalized_payment_by_scroll_idx,
                    BasicPayment::is_finalized,
                    scroll_idx,
                );

                assert_get_by_scroll_idx(
                    &db_state,
                    PaymentDbState::get_finalized_not_junk_payment_by_scroll_idx,
                    BasicPayment::is_finalized_not_junk,
                    scroll_idx,
                );
            }
        });
    }

    #[tokio::test]
    async fn test_sync_empty() {
        let mock_node = MockNode::new(BTreeMap::new());
        let mock_ffs = MockFfs::new();
        let db = Mutex::new(PaymentDb::empty(mock_ffs));

        sync_payments(&db, &mock_node, 5).await.unwrap();

        assert!(db.lock().unwrap().state.is_empty());
    }

    fn assert_db_payments_eq(
        db_payments: &[BasicPayment],
        node_payments: &BTreeMap<PaymentIndex, BasicPayment>,
    ) {
        assert_eq!(db_payments.len(), node_payments.len());
        assert!(db_payments.iter().eq(node_payments.values()));
    }

    #[test]
    fn test_sync() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let config = proptest::test_runner::Config::with_cases(4);

        proptest!(config, |(
            mut rng: FastRng,
            payments in arb_payments(1..20),
            req_batch_size in 1_u16..5,
            finalize_idxs in vec(any::<Index>(), 1..5),
        )| {
            let mut mock_node = MockNode::new(payments);

            let mut rng2 = FastRng::from_u64(rng.gen_u64());
            let mock_ffs = MockFfs::from_rng(rng);
            let db = Mutex::new(PaymentDb::empty(mock_ffs));

            rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                .unwrap();

            assert_db_payments_eq(&db.lock().unwrap().state.payments, &mock_node.payments);

            // reread and resync db from ffs -- should not change

            let mock_ffs = db.into_inner().unwrap().ffs;
            let db = Mutex::new(PaymentDb::read(mock_ffs).unwrap());

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
                    let new_status = if rng2.gen_boolean() {
                        PaymentStatus::Completed
                    } else {
                        PaymentStatus::Failed
                    };
                    mock_node
                        .payments
                        .get_mut(payment.index())
                        .unwrap()
                        .status = new_status;
                }
            }

            // resync -- should pick up the finalized payments

            rt.block_on(sync_payments(&db, &mock_node, req_batch_size))
                .unwrap();

            let db_lock = db.lock().unwrap();
            assert_db_payments_eq(&db_lock.state.payments, &mock_node.payments);
        });
    }
}
