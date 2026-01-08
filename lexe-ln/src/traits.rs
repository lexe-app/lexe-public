use std::{
    cmp, collections::HashMap, future::Future, ops::Deref, str::FromStr,
};

use anyhow::Context;
use async_trait::async_trait;
use common::{api::user::NodePk, ln::channel::LxOutPoint, time::TimestampMs};
use lexe_api::{
    models::command::{GetUpdatedPaymentMetadata, GetUpdatedPayments},
    types::payments::{
        DbPaymentMetadata, DbPaymentV2, LxPaymentId, PaymentUpdatedIndex,
    },
    vfs::{self, Vfs, VfsDirectory, VfsFileId},
};
use lexe_std::fmt::DisplayOption;
use lexe_tokio::notify_once::NotifyOnce;
use lightning::{
    chain::chainmonitor::Persist,
    events::{Event, ReplayEvent},
    ln::msgs::RoutingMessageHandler,
    util::ser::Writeable,
};
use tracing::warn;

use crate::{
    alias::{
        LexeChainMonitorType, LexeChannelManagerType, LexePeerManagerType,
        SignerType,
    },
    event::{EventHandleError, EventId},
    payments::{
        PaymentMetadata, PaymentV2, PaymentWithMetadata,
        manager::{CheckedPayment, PersistedPayment},
    },
};

/// Defines all the persister methods needed in shared Lexe LN logic.
#[async_trait]
pub trait LexeInnerPersister: Vfs + Persist<SignerType> {
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

    /// NOTE: The implementor *must* call `set_created_at_once` on the payment
    /// before persisting.
    async fn upsert_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment>;

    /// NOTE: The implementor *must* call `set_created_at_once` on the payment
    /// before persisting.
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
    /// 6. Merge, dedupe by ID (taking latest versions), sort by effective
    ///    updated_at = max(payment.updated_at, metadata.updated_at).
    ///
    /// The client uses END as the next query's start_index. This ensures no
    /// updates are missed: any item past END in the "shorter" list will be
    /// included in subsequent queries.
    ///
    /// **Edge case**: If one list is empty, there are no updates in that table
    /// past start_index. We can safely use the non-empty list's end as END,
    /// since there are no updates to miss in the empty table.
    ///
    /// Returns `(PaymentWithMetadata, effective_updated_at)` tuples sorted by
    /// `(effective_updated_at, id)`. The "effective" updated_at is defined as
    /// `max(payment.updated_at, metadata.updated_at)` - see step 6 above.
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
        db_payments.retain(|p| {
            let Ok(updated_at) = TimestampMs::try_from(p.updated_at) else {
                return false;
            };
            let Ok(id) = LxPaymentId::from_str(&p.id) else {
                return false;
            };
            PaymentUpdatedIndex { updated_at, id } <= end_index
        });
        db_metadatas.retain(|m| {
            let Ok(updated_at) = TimestampMs::try_from(m.updated_at) else {
                return false;
            };
            let Ok(id) = LxPaymentId::from_str(&m.id) else {
                return false;
            };
            PaymentUpdatedIndex { updated_at, id } <= end_index
        });

        // Collect IDs from both lists
        let payment_ids = db_payments
            .iter()
            .map(|p| p.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let metadata_ids = db_metadatas
            .iter()
            .map(|m| m.id.clone())
            .collect::<std::collections::HashSet<_>>();

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

        // Collect unique IDs with their effective updated_at index for sorting.
        // Effective updated_at = max(payment_updated_at, metadata_updated_at)
        // since either can change independently.
        let mut entries: Vec<(String, i64)> = payments_map
            .keys()
            .map(|id| {
                let payment_updated =
                    payments_map.get(id).map(|p| p.updated_at).unwrap_or(0);
                let metadata_updated =
                    metadatas_map.get(id).map(|m| m.updated_at).unwrap_or(0);
                let effective_updated_at =
                    cmp::max(payment_updated, metadata_updated);
                (id.clone(), effective_updated_at)
            })
            .collect();

        // Sort by effective updated_at, then by id for stability
        entries.sort_by(|(id_a, updated_a), (id_b, updated_b)| {
            updated_a.cmp(updated_b).then_with(|| id_a.cmp(id_b))
        });

        // Decrypt and return with effective updated_at timestamps
        entries
            .into_iter()
            .map(|(id, effective_updated_at)| {
                let db_payment = payments_map
                    .remove(&id)
                    .context("Payment missing from map")?;
                let db_metadata = metadatas_map.remove(&id);
                let pwm = self
                    .decrypt_payment_with_metadata(db_payment, db_metadata)?;
                let updated_at = TimestampMs::try_from(effective_updated_at)
                    .context("Invalid effective_updated_at")?;
                Ok((pwm, updated_at))
            })
            .collect()
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

/// A 'trait alias' defining all the requirements of a Lexe persister.
pub trait LexePersister:
    Clone + Send + Sync + 'static + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

impl<PS> LexePersister for PS where
    PS: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target: LexeInnerPersister + Send + Sync>
{
}

/// A 'trait alias' defining all the requirements of a Lexe channel manager.
pub trait LexeChannelManager<PS: LexePersister>:
    Clone + Send + Sync + 'static + Deref<Target = LexeChannelManagerType<PS>>
{
}

impl<CM, PS> LexeChannelManager<PS> for CM
where
    CM: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target = LexeChannelManagerType<PS>>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe chain monitor.
pub trait LexeChainMonitor<PS: LexePersister>:
    Send + Sync + 'static + Deref<Target = LexeChainMonitorType<PS>>
{
}

impl<CM, PS> LexeChainMonitor<PS> for CM
where
    CM: Send + Sync + 'static + Deref<Target = LexeChainMonitorType<PS>>,
    PS: LexePersister,
{
}

/// A 'trait alias' defining all the requirements of a Lexe peer manager.
pub trait LexePeerManager<CM, PS, RMH>:
    Clone + Send + Sync + 'static + Deref<Target = LexePeerManagerType<CM, RMH>>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    // TODO(max): Tried to create a `LexeRoutingMessageHandler` alias for these
    // bounds so the don't propagate everywhere, but couldn't get it to work.
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
{
    /// Returns `true` if we're connected to a peer with `node_pk`.
    fn is_connected(&self, node_pk: &NodePk) -> bool {
        // TODO(max): This LDK fn is O(n) in the # of peers...
        self.peer_by_node_id(&node_pk.0).is_some()
    }
}

impl<PM, CM, PS, RMH> LexePeerManager<CM, PS, RMH> for PM
where
    PM: Clone
        + Send
        + Sync
        + 'static
        + Deref<Target = LexePeerManagerType<CM, RMH>>,
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
{
}

/// A 'trait alias' defining all the requirements of a Lexe event handler.
pub trait LexeEventHandler: Clone + Send + Sync + 'static {
    /// Given a LDK [`Event`], get a future which handles it.
    /// The BGP passes this future to LDK for async event handling.
    fn get_ldk_handler_future(
        &self,
        event: Event,
    ) -> impl Future<Output = Result<(), ReplayEvent>> + Send;

    /// Handle an event.
    fn handle_event(
        &self,
        event_id: &EventId,
        event: Event,
    ) -> impl Future<Output = Result<(), EventHandleError>> + Send;

    fn persister(&self) -> &impl LexePersister;
    fn shutdown(&self) -> &NotifyOnce;
}
