use std::{
    cmp,
    collections::{HashMap, HashSet},
    str::FromStr,
};

use anyhow::{Context, ensure};
use async_trait::async_trait;
use common::{
    aes::AesMasterKey,
    constants,
    ln::channel::LxOutPoint,
    rng::{Crng, SysRng},
    time::TimestampMs,
};
use lexe_api::{
    models::command::{GetUpdatedPaymentMetadata, GetUpdatedPayments},
    types::payments::{
        DbPaymentMetadata, DbPaymentV2, LxPaymentId, PaymentUpdatedIndex,
    },
    vfs::{self, Vfs, VfsDirectory, VfsFile, VfsFileId},
};
use lexe_std::fmt::DisplayOption;
use lightning::{events::Event, util::ser::Writeable};
use serde::{Serialize, de::DeserializeOwned};
use tracing::{info, warn};

use crate::{
    alias::LexeChainMonitorType,
    event::EventId,
    migrations::{self, Migrations},
    payments::{
        PaymentMetadata, PaymentV2, PaymentWithMetadata,
        manager::{CheckedPayment, PersistedPayment},
    },
    traits::LexePersister,
};

// --- VFS encryption / decryption helpers --- //

/// Serializes a LDK [`Writeable`] to bytes, encrypts the serialized bytes, and
/// packages everything up into a [`VfsFile`] which is ready to be persisted.
pub fn encrypt_ldk_writeable(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    writeable: &impl Writeable,
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        // - Writeable can write to any LDK lightning::util::ser::Writer
        // - Writer is impl'd for all types that impl std::io::Write
        // - Write is impl'd for Vec<u8>
        // Therefore a Writeable can be written to a Vec<u8>.
        writeable
            .write(mut_vec_u8)
            .expect("Serialization into an in-memory buffer should never fail");
    })
}

/// Serializes an object to JSON bytes, encrypts the serialized bytes, and
/// packages everything up into a [`VfsFile`] which is ready to be persisted.
pub fn encrypt_json(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    value: &impl Serialize,
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        serde_json::to_writer(mut_vec_u8, value)
            .expect("JSON serialization was not implemented correctly");
    })
}

/// Encrypt some arbitrary plaintext bytes to a [`VfsFile`].
///
/// You should prefer [`encrypt_json`] and [`encrypt_ldk_writeable`] over this,
/// since those fns avoid the need to write to an intermediate plaintext buffer.
pub fn encrypt_bytes(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    plaintext_bytes: &[u8],
) -> VfsFile {
    encrypt_file(rng, vfs_master_key, file_id, &|mut_vec_u8| {
        mut_vec_u8.extend(plaintext_bytes)
    })
}

fn encrypt_file(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    file_id: VfsFileId,
    write_data_cb: &dyn Fn(&mut Vec<u8>),
) -> VfsFile {
    // bind the dirname and filename so files can't be moved around. the
    // owner identity is already bound by the key derivation path.
    //
    // this is only a best-effort mitigation however. files in an untrusted
    // storage can still be deleted or rolled back to an earlier version
    // without detection currently.
    let dirname = &file_id.dir.dirname;
    let filename = &file_id.filename;
    let aad = &[dirname.as_bytes(), filename.as_bytes()];
    let data_size_hint = None;
    let data = vfs_master_key.encrypt(rng, aad, data_size_hint, write_data_cb);

    // Print a warning if the ciphertext is greater than 1 MB.
    // We are interested in large LDK types as well as the WalletDb.
    let data_len = data.len();
    if data_len > 1_000_000 {
        info!("{dirname}/{filename} is >1MB: {data_len} bytes");
    }

    VfsFile { id: file_id, data }
}

/// Decrypt a file previously encrypted using `encrypt_file`.
///
/// Since the file is probably coming from an untrusted source, be sure to pass
/// in an `expected_file_id` which contains the `dirname` and `filename` that we
/// expect. The `returned_file` which came from the untrusted DB will be
/// validated against the `expected_file_id`.
///
/// If successful, returns the decrypted plaintext bytes contained in the file.
pub fn decrypt_file(
    vfs_master_key: &AesMasterKey,
    expected_file_id: &VfsFileId,
    returned_file: VfsFile,
) -> anyhow::Result<Vec<u8>> {
    let dirname = &expected_file_id.dir.dirname;
    let filename = &expected_file_id.filename;
    let returned_dirname = &returned_file.id.dir.dirname;
    let returned_filename = &returned_file.id.filename;
    ensure!(
        returned_dirname == dirname,
        "Dirnames don' match: {returned_dirname} != {dirname}"
    );
    ensure!(
        returned_filename == filename,
        "Filenames don' match: {returned_filename} != {filename}"
    );

    let aad = &[dirname.as_bytes(), filename.as_bytes()];
    vfs_master_key
        .decrypt(aad, returned_file.data)
        .with_context(|| format!("{expected_file_id}"))
        .context("Failed to decrypt encrypted VFS file")
}

/// Exactly [`decrypt_file`], but also attempts to deserialize the decrypted
/// JSON plaintext bytes into the expected type.
#[inline]
pub fn decrypt_json_file<D: DeserializeOwned>(
    vfs_master_key: &AesMasterKey,
    expected_file_id: &VfsFileId,
    returned_file: VfsFile,
) -> anyhow::Result<D> {
    let json_bytes =
        decrypt_file(vfs_master_key, expected_file_id, returned_file)
            .context("Decryption failed")?;
    let value = serde_json::from_slice(json_bytes.as_slice())
        .with_context(|| format!("{expected_file_id}"))
        .context("JSON deserialization failed")?;

    Ok(value)
}

// --- LexePersisterMethods --- //

/// Defines all persister methods used in shared Lexe LN logic.
#[async_trait]
pub trait LexePersisterMethods: Vfs {
    // --- Required methods: general --- //

    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()>;

    async fn persist_channel_monitor<PS: LexePersister>(
        &self,
        chain_monitor: &LexeChainMonitorType<PS>,
        funding_txo: &LxOutPoint,
    ) -> anyhow::Result<()>;

    // --- Required methods: payments --- //

    async fn get_pending_payments(&self) -> anyhow::Result<Vec<PaymentV2>>;

    async fn get_payment_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<PaymentV2>>;

    async fn get_payment_metadata_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<PaymentMetadata>>;

    /// NOTE: The implementor *must* call `set_created_at_idempotent` on the
    /// payment before persisting.
    async fn upsert_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment>;

    /// NOTE: The implementor *must* call `set_created_at_idempotent` on the
    /// payment before persisting.
    async fn upsert_payment_batch(
        &self,
        checked_batch: Vec<CheckedPayment>,
    ) -> anyhow::Result<Vec<PersistedPayment>>;

    // --- Required methods: db-level payments --- //

    async fn get_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> anyhow::Result<Vec<DbPaymentV2>>;

    async fn get_updated_payment_metadata(
        &self,
        req: GetUpdatedPaymentMetadata,
    ) -> anyhow::Result<Vec<DbPaymentMetadata>>;

    async fn get_payments_by_ids(
        &self,
        ids: Vec<LxPaymentId>,
    ) -> anyhow::Result<Vec<DbPaymentV2>>;

    async fn get_payment_metadatas_by_ids(
        &self,
        ids: Vec<LxPaymentId>,
    ) -> anyhow::Result<Vec<DbPaymentMetadata>>;

    async fn upsert_payments(
        &self,
        payments: Vec<DbPaymentV2>,
    ) -> anyhow::Result<()>;

    async fn upsert_payment_metadatas(
        &self,
        metadatas: Vec<DbPaymentMetadata>,
    ) -> anyhow::Result<()>;

    // --- Required methods: payments encryption --- //

    fn encrypt_pwm(
        &self,
        rng: &mut SysRng,
        payment: &PaymentV2,
        metadata: Option<&PaymentMetadata>,
        created_at: TimestampMs,
        updated_at: TimestampMs,
    ) -> anyhow::Result<(DbPaymentV2, Option<DbPaymentMetadata>)>;

    fn decrypt_payment_with_metadata(
        &self,
        db_payment: DbPaymentV2,
        db_metadata: Option<DbPaymentMetadata>,
    ) -> anyhow::Result<PaymentWithMetadata>;

    // TODO(max): Should be able to remove this once we remove
    // get_pending_payments_with_metadata.
    fn decrypt_metadata(
        &self,
        db_metadata: DbPaymentMetadata,
    ) -> anyhow::Result<PaymentMetadata>;

    // --- Required methods: wallet --- //

    /// Read the legacy (<= node-v0.9.1) wallet changeset, if it exists.
    async fn read_wallet_changeset_legacy(
        &self,
    ) -> anyhow::Result<Option<bdk_wallet::ChangeSet>>;

    // --- Provided methods --- //

    async fn get_payment_with_metadata_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<PaymentWithMetadata>> {
        let (payment_result, metadata_result) = tokio::join!(
            self.get_payment_by_id(id),
            self.get_payment_metadata_by_id(id),
        );

        match payment_result? {
            Some(payment) => {
                let metadata = metadata_result?
                    .unwrap_or_else(|| PaymentMetadata::empty(id));
                Ok(Some(PaymentWithMetadata { payment, metadata }))
            }
            None => Ok(None),
        }
    }

    // TODO(max): Should be able to remove this once PaymentsManager takes
    // PaymentV2 instead of PaymentWithMetadata.
    async fn get_pending_payments_with_metadata(
        &self,
    ) -> anyhow::Result<Vec<PaymentWithMetadata>> {
        let payments = self.get_pending_payments().await?;
        let ids = payments.iter().map(|p| p.id()).collect();
        let db_metadatas = self.get_payment_metadatas_by_ids(ids).await?;

        // Build a map of id -> db_metadata for efficient lookup
        let mut metadata_map = db_metadatas
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<HashMap<_, _>>();

        let pwms = payments
            .into_iter()
            .map(|payment| {
                let id = payment.id();
                let metadata = metadata_map
                    .remove(&id.to_string())
                    .map(|db_m| self.decrypt_metadata(db_m))
                    .transpose()?
                    .unwrap_or_else(|| PaymentMetadata::empty(id));
                Ok(PaymentWithMetadata { payment, metadata })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        Ok(pwms)
    }

    /// Fetch updated payments and metadata, synchronizing both streams.
    ///
    /// Both the payments and metadata tables can be updated independently.
    /// To allow the client to tail both consistently, we:
    ///
    /// 1. Fetch both `get_updated_payments` and `get_updated_payment_metadata`
    ///    with the same start_index.
    /// 2. Compute END = min(last_payment_index, last_metadata_index).
    /// 3. Filter both lists to items with index <= END.
    /// 4. For any payment in the filtered list, ensure we have its metadata.
    /// 5. For any metadata in the filtered list, ensure we have its payment.
    /// 6. Merge, dedupe by ID (taking latest versions), sort by pwm_updated_at:
    ///    `pwm_updated_at = max(payment.updated_at, metadata.updated_at)`.
    ///
    /// The client uses END as the next query's start_index. This ensures no
    /// updates are missed: any item past END in the "shorter" list will be
    /// included in subsequent queries.
    ///
    /// **Edge case**: If one list is empty, there are no updates in that table
    /// past start_index. We can safely use the non-empty list's end as END,
    /// since there are no updates to miss in the empty table.
    ///
    /// Returns `(PaymentWithMetadata, pwm_updated_at)` tuples sorted by
    /// `(pwm_updated_at, id)`. pwm_updated_at is defined as
    /// `max(payment.updated_at, metadata.updated_at)` from step 6 above.
    async fn get_updated_payments_with_metadata(
        &self,
        req: GetUpdatedPayments,
    ) -> anyhow::Result<Vec<(PaymentWithMetadata, TimestampMs)>> {
        // Fetch both updated payments and metadata in parallel
        let (payments_result, metadata_result) = tokio::join!(
            self.get_updated_payments(GetUpdatedPayments {
                start_index: req.start_index,
                limit: req.limit,
            }),
            self.get_updated_payment_metadata(GetUpdatedPaymentMetadata {
                start_index: req.start_index,
                limit: req.limit,
            }),
        );

        let mut db_payments = payments_result?;
        let mut db_metadatas = metadata_result?;

        // If both empty, nothing to do
        if db_payments.is_empty() && db_metadatas.is_empty() {
            return Ok(Vec::new());
        }

        // The queries above could race; an update might be seen in one list but
        // not the other. If one of the two lists is empty, we can ensure we
        // don't advance the cursor past updates that were committed between the
        // two queries by doing a re-fetch of the empty list to confirm that the
        // list is indeed empty. In practice, this should rarely happen, as
        // payments and metadata are usually persisted together, the queries
        // above occur within milliseconds, and an update would have to slip in
        // between the two to trigger.
        if db_payments.is_empty() {
            warn!(
                start_index = %DisplayOption(req.start_index),
                "Defensive re-fetch of empty payments list"
            );
            db_payments = self
                .get_updated_payments(GetUpdatedPayments {
                    start_index: req.start_index,
                    limit: req.limit,
                })
                .await?;
        }
        if db_metadatas.is_empty() {
            warn!(
                start_index = %DisplayOption(req.start_index),
                "Defensive re-fetch of empty metadata list"
            );
            db_metadatas = self
                .get_updated_payment_metadata(GetUpdatedPaymentMetadata {
                    start_index: req.start_index,
                    limit: req.limit,
                })
                .await?;
        }

        // Find the maximum updated index in both lists
        let max_payment_idx = db_payments
            .iter()
            .max_by_key(|p| (p.updated_at, &p.id))
            .map(|p| {
                let updated_at = TimestampMs::try_from(p.updated_at)
                    .context("Invalid payment updated_at")?;
                let id = LxPaymentId::from_str(&p.id)
                    .context("Invalid payment id")?;
                anyhow::Ok(PaymentUpdatedIndex { updated_at, id })
            })
            .transpose()
            .context("Invalid payment index")?;
        let max_metadata_idx = db_metadatas
            .iter()
            .max_by_key(|m| (m.updated_at, &m.id))
            .map(|m| {
                let updated_at = TimestampMs::try_from(m.updated_at)
                    .context("Invalid metadata updated_at")?;
                let id = LxPaymentId::from_str(&m.id)
                    .context("Invalid metadata id")?;
                anyhow::Ok(PaymentUpdatedIndex { updated_at, id })
            })
            .transpose()?;

        // Compute END index = min(max_payment_idx, max_metadata_idx).
        // If one list is empty, use the other's max. This is safe because an
        // empty list means there are no updates in that table past start_index.
        let end_index = match (max_payment_idx, max_metadata_idx) {
            (Some(p), Some(m)) => cmp::min(p, m),
            (Some(p), None) => p,
            (None, Some(m)) => m,
            (None, None) => return Ok(Vec::new()),
        };

        // Filter both lists to items <= END
        db_payments = db_payments
            .into_iter()
            .map(|p| {
                let updated_at = TimestampMs::try_from(p.updated_at)
                    .context("Invalid payment updated_at")?;
                let id = LxPaymentId::from_str(&p.id)
                    .context("Invalid payment id")?;
                let idx = PaymentUpdatedIndex { updated_at, id };
                anyhow::Ok((p, idx))
            })
            // Retain index <= END; error out if any of the conversions failed.
            .filter_map(|r| match r {
                Ok((p, idx)) if idx <= end_index => Some(Ok(p)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<anyhow::Result<_>>()?;
        db_metadatas = db_metadatas
            .into_iter()
            .map(|m| {
                let updated_at = TimestampMs::try_from(m.updated_at)
                    .context("Invalid metadata updated_at")?;
                let id = LxPaymentId::from_str(&m.id)
                    .context("Invalid metadata id")?;
                let idx = PaymentUpdatedIndex { updated_at, id };
                anyhow::Ok((m, idx))
            })
            // Retain index <= END; error out if any of the conversions failed.
            .filter_map(|r| match r {
                Ok((m, idx)) if idx <= end_index => Some(Ok(m)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<anyhow::Result<_>>()?;

        // Collect IDs from both lists
        let payment_ids = db_payments
            .iter()
            .map(|p| p.id.clone())
            .collect::<HashSet<_>>();
        let metadata_ids = db_metadatas
            .iter()
            .map(|m| m.id.clone())
            .collect::<HashSet<_>>();

        // Find IDs that need their counterpart fetched
        let payment_ids_needing_metadata = payment_ids
            .difference(&metadata_ids)
            .filter_map(|id| LxPaymentId::from_str(id).ok())
            .collect::<Vec<_>>();
        let metadata_ids_needing_payment = metadata_ids
            .difference(&payment_ids)
            .filter_map(|id| LxPaymentId::from_str(id).ok())
            .collect::<Vec<_>>();

        // Fetch missing payments and metadata
        if !metadata_ids_needing_payment.is_empty() {
            let missing_payments = self
                .get_payments_by_ids(metadata_ids_needing_payment)
                .await?;
            db_payments.extend(missing_payments);
        }
        if !payment_ids_needing_metadata.is_empty() {
            let missing_metadata = self
                .get_payment_metadatas_by_ids(payment_ids_needing_metadata)
                .await?;
            db_metadatas.extend(missing_metadata);
        }

        // Build id -> payment/metadata maps for efficient pairing during
        // decrypt. Dedupe by taking the latest version since db_payments /
        // db_metadatas may contain duplicates from the initial fetch, re-fetch,
        // or by-id fetch.
        let mut payments_map: HashMap<String, DbPaymentV2> = HashMap::new();
        let mut metadatas_map: HashMap<String, DbPaymentMetadata> =
            HashMap::new();
        for p in db_payments {
            payments_map
                .entry(p.id.clone())
                .and_modify(|existing| {
                    if p.updated_at > existing.updated_at {
                        *existing = p.clone();
                    }
                })
                .or_insert(p);
        }
        for m in db_metadatas {
            metadatas_map
                .entry(m.id.clone())
                .and_modify(|existing| {
                    if m.updated_at > existing.updated_at {
                        *existing = m.clone();
                    }
                })
                .or_insert(m);
        }

        // Collect unique IDs with their pwm_updated_at index for sorting.
        // pwm_updated_at = max(payment_updated_at, metadata_updated_at)
        // since either can change independently.
        let mut entries: Vec<(String, i64)> = payments_map
            .keys()
            .map(|id| {
                let payment_updated =
                    payments_map.get(id).map(|p| p.updated_at).unwrap_or(0);
                let metadata_updated =
                    metadatas_map.get(id).map(|m| m.updated_at).unwrap_or(0);
                let pwm_updated_at =
                    cmp::max(payment_updated, metadata_updated);
                (id.clone(), pwm_updated_at)
            })
            .collect();

        // Sort by pwm_updated_at, then by id for stability
        entries.sort_unstable_by(|(id_a, updated_a), (id_b, updated_b)| {
            updated_a.cmp(updated_b).then_with(|| id_a.cmp(id_b))
        });

        // Decrypt and return with pwm_updated_at timestamps
        entries
            .into_iter()
            .map(|(id, pwm_updated_at)| {
                let db_payment = payments_map
                    .remove(&id)
                    .context("Payment missing from map")?;
                let db_metadata = metadatas_map.remove(&id);
                let pwm = self
                    .decrypt_payment_with_metadata(db_payment, db_metadata)?;
                let pwm_updated_at = TimestampMs::try_from(pwm_updated_at)
                    .context("Invalid pwm_updated_at")?;
                Ok((pwm, pwm_updated_at))
            })
            .collect()
    }

    /// Migrate payments from `PaymentV1` to `PaymentV2` + `PaymentMetadata`.
    ///
    /// Idempotent: Creates a marker file at `migrations/payments_v2` once
    /// complete, and skips the migration if the marker file already exists.
    #[tracing::instrument(skip_all, name = "(migrate-payments-v2)")]
    async fn migrate_to_payments_v2(
        &self,
        initial_migrations: &Migrations,
    ) -> anyhow::Result<()> {
        // Check if migration has already run
        if initial_migrations.is_applied(migrations::MARKER_PAYMENTS_V2) {
            return Ok(());
        }

        info!("Proceeding with payments_v2 migration");

        // Get stop timestamp - we'll migrate all payments up to this point
        let stop = TimestampMs::now();

        let mut rng = SysRng::new();
        let mut start_index: Option<PaymentUpdatedIndex> = None;
        let mut total_batches = 0;
        let mut total_migrated = 0;

        loop {
            let req = GetUpdatedPayments {
                start_index,
                limit: Some(constants::MAX_PAYMENTS_BATCH_SIZE),
            };

            // A batch of `(PaymentWithMetadata, pwm_updated_at)` to migrate.
            //
            // `get_updated_payments_with_metadata` transitively calls
            // `payments::encryption::decrypt_pwm` which accepts both v1 and v2
            // serialization formats. If we crash in the middle of a migration,
            // we'll repeat work in the next migration attempt, but that's OK.
            let batch = self.get_updated_payments_with_metadata(req).await?;

            // Only migrate payments <= the `stop` timestamp.
            let mut batch = batch;
            while batch
                .last()
                .is_some_and(|(_pwm, pwm_updated_at)| *pwm_updated_at > stop)
            {
                batch.pop();
            }

            // If nothing to migrate, we're done.
            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();

            // Update start_index (for the next iteration) to the last payment
            // we'll process in this iteration.
            let (last_pwm, last_pwm_updated_at) =
                batch.last().expect("Checked non-empty above");
            start_index = Some(PaymentUpdatedIndex {
                updated_at: *last_pwm_updated_at,
                id: last_pwm.payment.id(),
            });

            // Encrypt, which serializes as `PaymentV2` + `PaymentMetadata`.
            // Then separate out payments from metadatas.
            let now = TimestampMs::now();
            let mut db_payments = Vec::with_capacity(batch_len);
            let mut db_metadatas = Vec::with_capacity(batch_len);
            for (pwm, _) in batch.iter_mut() {
                let created_at = pwm.payment.set_created_at_idempotent(now);
                let updated_at = now;
                let (db_payment, db_metadata) = self
                    .encrypt_pwm(
                        &mut rng,
                        &pwm.payment,
                        Some(&pwm.metadata),
                        created_at,
                        updated_at,
                    )
                    .context("Failed to encrypt pwm")?;
                db_payments.push(db_payment);
                if let Some(metadata) = db_metadata {
                    db_metadatas.push(metadata);
                }
            }

            // Write metadatas first - v1 payments contain embedded metadata,
            // so we must ensure metadata is written before overwriting with v2,
            // otherwise data would be lost in the event of a crash.
            if !db_metadatas.is_empty() {
                self.upsert_payment_metadatas(db_metadatas)
                    .await
                    .context("Failed to upsert payment metadatas")?;
            }

            // Metadata batch succeeded, write the payments batch.
            self.upsert_payments(db_payments)
                .await
                .context("Failed to upsert payments")?;

            total_batches += 1;
            total_migrated += batch_len;

            info!("Migrated batch {total_batches} ({batch_len} payments)");
        }

        // Persist marker file (empty file)
        Migrations::mark_applied(self, migrations::MARKER_PAYMENTS_V2).await?;

        info!(
            "Migration complete: \
             {total_migrated} payments in {total_batches} batches"
        );

        Ok(())
    }

    /// Reads all persisted events, along with their event IDs.
    async fn read_events(&self) -> anyhow::Result<Vec<(EventId, Event)>> {
        let dir = VfsDirectory::new(vfs::EVENTS_DIR);
        let ids_and_events = self
            .read_dir_maybereadable(&dir)
            .await?
            .into_iter()
            .map(|(file_id, event)| {
                let event_id = EventId::from_str(&file_id.filename)
                    .with_context(|| file_id.filename.clone())
                    .context("Couldn't parse event ID from filename")?;
                Ok((event_id, event))
            })
            .collect::<anyhow::Result<_>>()
            .context("Error while reading events")?;
        Ok(ids_and_events)
    }

    async fn persist_event(
        &self,
        event: &Event,
        event_id: &EventId,
    ) -> anyhow::Result<()> {
        let filename = event_id.to_string();
        let file_id = VfsFileId::new(vfs::EVENTS_DIR, filename);
        // With LDK's fallible event handling, persistence failures return
        // `ReplayEvent` to LDK, which handles replays for us. A single retry
        // handles transient errors while avoiding excessive retry loops.
        let retries = 1;
        self.persist_ldk_writeable(file_id, &event, retries).await
    }

    async fn remove_event(&self, event_id: &EventId) -> anyhow::Result<()> {
        let filename = event_id.to_string();
        let file_id = VfsFileId::new(vfs::EVENTS_DIR, filename);
        self.remove_file(&file_id).await
    }
}
