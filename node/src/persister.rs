//! # Node persister
//!
//! ## Channel manager & channel monitor persistence
//!
//! Previously, all writes were persisted immediately in both GDrive and Lexe
//! DB. However, since GDrive takes 2.5s for some writes and multiple writes are
//! needed per payment, we saw unacceptable payment latencies of 30s+.
//!
//! As a temporary workaround, we currently treat Lexe's DB as the 'primary'
//! data store.
//!
//! Reads: Read from just Lexe's DB. This is required for safety, since
//! asynchronous writes may fail long after many Lexe DB updates have already
//! been persisted - it's not safe revert to very old states.
//!
//! Writes: When persisting the channel manager or channel monitor, we consider
//! Lexe's DB to be the 'primary' data store, so we return to the caller once
//! persistence in Lexe's DB is complete. However, we also trigger a task to
//! backup the channel state to GDrive asynchronously. This sacrifices some
//! rollback-resistance for significantly improved latency.
//!
//! In the future, we will persist all critical channel state onto multiple
//! independent (and thus rollback-resistant) VSS servers. Asynchronous backups
//! to GDrive will still be available as an option to our users.
//!
//! ### In detail
//!
//! Channel manager:
//! - Read: Read from Lexe DB.
//! - Write: Write to Lexe DB and trigger an asynchronous GDrive backup.
//!
//! Channel monitors:
//! - Read: Read from Lexe DB.
//! - Write: Write to Lexe DB and trigger an asynchronous GDrive backup.
//! - Archive: Write to and delete from both Lexe's DB and GDrive synchronously.

use std::{
    cmp,
    collections::{HashMap, HashSet},
    io::Cursor,
    str::FromStr,
    sync::Arc,
    time::SystemTime,
};

use anyhow::{Context, anyhow, ensure};
use async_trait::async_trait;
use bitcoin::hash_types::BlockHash;
use common::{
    aes::AesMasterKey,
    api::{
        auth::BearerAuthToken,
        user::{Scid, Scids},
    },
    constants,
    ln::channel::LxOutPoint,
    rng::{Crng, SysRng},
    time::TimestampMs,
};
use gdrive::{GoogleVfs, GvfsRoot, oauth2::GDriveCredentials};
use lexe_api::{
    auth::BearerAuthenticator,
    def::NodeBackendApi,
    error::{BackendApiError, BackendErrorKind},
    models::command::{
        GetNewPayments, GetUpdatedPaymentMetadata, GetUpdatedPayments,
        LxPaymentIdStruct, VecLxPaymentId,
    },
    types::{
        Empty,
        payments::{
            BasicPaymentV1, BasicPaymentV2, DbPaymentMetadata, DbPaymentV2,
            LxPaymentId, PaymentUpdatedIndex, VecDbPaymentMetadata,
            VecDbPaymentV2,
        },
    },
    vfs::{
        self, SINGLETON_DIRECTORY, Vfs, VfsDirectory, VfsDirectoryList,
        VfsFile, VfsFileId,
    },
};
use lexe_ln::{
    alias::{
        BroadcasterType, ChannelMonitorType, FeeEstimatorType,
        LexeChainMonitorType, MessageRouterType, RouterType, SignerType,
    },
    channel_monitor::{ChannelMonitorUpdateKind, LxChannelMonitorUpdate},
    keys_manager::LexeKeysManager,
    logger::LexeTracingLogger,
    payments::{
        self, PaymentWithMetadata,
        manager::{CheckedPayment, PersistedPayment},
        v1::PaymentV1,
    },
    persister,
    traits::{LexeInnerPersister, LexePersister},
    wallet::ChangeSet,
};
use lexe_std::{Apply, backoff, fmt::DisplayOption};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::{
    chain::{
        ChannelMonitorUpdateStatus, chainmonitor::Persist,
        channelmonitor::ChannelMonitorUpdate, transaction::OutPoint,
    },
    ln::channelmanager::ChannelManagerReadArgs,
    util::{
        config::UserConfig,
        ser::{ReadableArgs, Writeable},
    },
};
use secrecy::ExposeSecret;
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, warn};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    approved_versions::ApprovedVersions,
    client::NodeBackendClient,
};

/// Data discrepancy evaluation and resolution.
mod discrepancy;
/// Logic to conduct operations on multiple data backends at the same time.
mod multi;

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
const GDRIVE_CREDENTIALS_FILENAME: &str = "gdrive_credentials";
const GVFS_ROOT_FILENAME: &str = "gvfs_root";

pub struct NodePersister {
    backend_api: Arc<NodeBackendClient>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    google_vfs: Option<Arc<GoogleVfs>>,
    channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    gdrive_persister_tx: mpsc::Sender<VfsFile>,
    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    shutdown: NotifyOnce,
}

/// General helper for upserting well-formed [`VfsFile`]s.
pub(crate) async fn persist_file(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    file: &VfsFile,
) -> anyhow::Result<()> {
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    backend_api
        .upsert_file(&file.id, file.data.clone().into(), token)
        .await
        .context("Could not upsert file")?;

    Ok(())
}

/// Encrypts the [`GDriveCredentials`] to a [`VfsFile`] which can be persisted.
// Normally this function would do the upsert too, but the &GDriveCredentials is
// typically behind a tokio::sync::watch::Ref which is not Send.
#[inline]
pub(crate) fn encrypt_gdrive_credentials(
    rng: &mut impl Crng,
    vfs_master_key: &AesMasterKey,
    credentials: &GDriveCredentials,
) -> VfsFile {
    let file_id =
        VfsFileId::new(SINGLETON_DIRECTORY, GDRIVE_CREDENTIALS_FILENAME);
    persister::encrypt_json(rng, vfs_master_key, file_id, &credentials)
}

pub(crate) async fn read_gdrive_credentials(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<Option<GDriveCredentials>> {
    let file_id =
        VfsFileId::new(SINGLETON_DIRECTORY, GDRIVE_CREDENTIALS_FILENAME);
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    match backend_api.get_file(&file_id, token).await {
        Ok(data) => {
            let file = VfsFile::from_parts(file_id.clone(), data);
            let credentials =
                persister::decrypt_json_file(vfs_master_key, &file_id, file)
                    .context("Failed to decrypt GDrive credentials file")?;
            Ok(Some(credentials))
        }
        Err(BackendApiError {
            kind: BackendErrorKind::NotFound,
            ..
        }) => Ok(None),
        Err(e) => Err(e).context("Failed to fetch file")?,
    }
}

pub(crate) async fn persist_gvfs_root(
    rng: &mut impl Crng,
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    gvfs_root: &GvfsRoot,
) -> anyhow::Result<()> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, GVFS_ROOT_FILENAME);
    let file =
        persister::encrypt_json(rng, vfs_master_key, file_id, &gvfs_root);

    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    backend_api
        .upsert_file(&file.id, file.data.into(), token)
        .await
        .context("Could not upsert file")?;

    Ok(())
}

pub(crate) async fn read_gvfs_root(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<Option<GvfsRoot>> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, GVFS_ROOT_FILENAME);
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    let maybe_gvfs_root = match backend_api.get_file(&file_id, token).await {
        Ok(data) => {
            let file = VfsFile::from_parts(file_id.clone(), data);
            let gvfs_root =
                persister::decrypt_json_file(vfs_master_key, &file_id, file)?;
            Some(gvfs_root)
        }
        Err(BackendApiError {
            kind: BackendErrorKind::NotFound,
            ..
        }) => None,
        Err(e) => return Err(e).context("Failed to fetch file"),
    };

    Ok(maybe_gvfs_root)
}

/// Checks whether a password-encrypted [`RootSeed`] exists in Google Drive.
/// Does not check if the backup is well-formed, matches the current seed, etc.
///
/// [`RootSeed`]: common::root_seed::RootSeed
#[inline]
pub(crate) async fn password_encrypted_root_seed_exists(
    google_vfs: &GoogleVfs,
) -> bool {
    // This fn barely does anything, but we want it close to the impl of
    // `persist_password_encrypted_root_seed` for ez inspection
    let file_id =
        VfsFileId::new(SINGLETON_DIRECTORY, vfs::PW_ENC_ROOT_SEED_FILENAME);
    google_vfs.file_exists(&file_id).await
}

/// Persists the given password-encrypted [`RootSeed`] to GDrive.
///
/// [`RootSeed`]: common::root_seed::RootSeed
pub(crate) async fn upsert_password_encrypted_root_seed(
    google_vfs: &GoogleVfs,
    encrypted_seed: Vec<u8>,
) -> anyhow::Result<()> {
    let file = VfsFile::new(
        SINGLETON_DIRECTORY,
        vfs::PW_ENC_ROOT_SEED_FILENAME,
        encrypted_seed,
    );
    // Upsert, not create, as the user may have rotated their password
    google_vfs
        .upsert_file(&file.id, file.data.into())
        .await
        .context("Failed to create root seed file")?;
    Ok(())
}

/// Read the [`ApprovedVersions`] list from backend storage, if it exists.
// XXX(max): For real protection, this *must* read from 3rd party VSS rather
// than the Lexe DB.
pub(crate) async fn read_approved_versions(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<Option<ApprovedVersions>> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, "approved_versions");
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    match backend_api.get_file(&file_id, token).await {
        Ok(data) => {
            let file = VfsFile::from_parts(file_id.clone(), data);
            let approved_versions =
                persister::decrypt_json_file(vfs_master_key, &file_id, file)?;
            Ok(Some(approved_versions))
        }
        Err(BackendApiError {
            kind: BackendErrorKind::NotFound,
            ..
        }) => Ok(None),
        Err(e) => Err(e).context("Failed to fetch file"),
    }
}

/// Persists the given [`ApprovedVersions`] to backend storage.
// XXX(max): For real protection, this *must* persist to 3rd party VSS rather
// than the Lexe DB.
pub(crate) async fn persist_approved_versions(
    rng: &mut impl Crng,
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    approved_versions: &ApprovedVersions,
) -> anyhow::Result<()> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, "approved_versions");
    // While encrypting this isn't totally necessary, it's a good safeguard in
    // case a user's GDrive gets compromised and the attacker wants to force the
    // user to approve an old (vulnerable) version. Adding this layer of
    // encryption prevents the attacker from writing their own ApprovedVersions
    // if they don't have access to the user's root seed.
    let file = persister::encrypt_json(
        rng,
        vfs_master_key,
        file_id,
        approved_versions,
    );

    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    backend_api
        .upsert_file(&file.id, file.data.into(), token)
        .await
        .context("Failed to upsert approved versions file")?;

    Ok(())
}

impl NodePersister {
    /// Initialize a [`NodePersister`].
    /// `google_vfs` MUST be [`Some`] if we are running in staging or prod.
    pub(crate) fn new(
        backend_api: Arc<NodeBackendClient>,
        authenticator: Arc<BearerAuthenticator>,
        vfs_master_key: Arc<AesMasterKey>,
        google_vfs: Option<Arc<GoogleVfs>>,
        channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
        gdrive_persister_tx: mpsc::Sender<VfsFile>,
        eph_tasks_tx: mpsc::Sender<LxTask<()>>,
        shutdown: NotifyOnce,
    ) -> Self {
        Self {
            backend_api,
            authenticator,
            vfs_master_key,
            google_vfs,
            channel_monitor_persister_tx,
            gdrive_persister_tx,
            eph_tasks_tx,
            shutdown,
        }
    }

    pub(crate) async fn get_token(
        &self,
    ) -> Result<BearerAuthToken, BackendApiError> {
        self.authenticator
            .get_token(&*self.backend_api, SystemTime::now())
            .await
    }

    /// Get a reference to the underlying [`NodeBackendClient`].
    pub(crate) fn backend_api(&self) -> &NodeBackendClient {
        self.backend_api.as_ref()
    }

    /// Get a reference to the underlying [`BearerAuthenticator`].
    pub(crate) fn authenticator(&self) -> &BearerAuthenticator {
        self.authenticator.as_ref()
    }

    /// Get a reference to the underlying [`AesMasterKey`].
    pub(crate) fn vfs_master_key(&self) -> &AesMasterKey {
        self.vfs_master_key.as_ref()
    }

    /// Upserts a file to GDrive with the given # of `retries` if this
    /// [`NodePersister`] contains a [`GoogleVfs`], otherwise does nothing.
    // TODO(max): This fn should be reused in more places.
    pub(crate) async fn upsert_gdrive_if_available(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        retries: usize,
    ) -> anyhow::Result<()> {
        let gvfs = match self.google_vfs.as_ref() {
            Some(gvfs) => gvfs,
            None => return Ok(()),
        };

        let mut upsert_result = gvfs.upsert_file(file_id, data.clone()).await;

        let mut backoff_iter = backoff::get_backoff_iter();
        for i in 0..retries {
            if upsert_result.is_ok() {
                break;
            }

            tokio::time::sleep(backoff_iter.next().unwrap()).await;

            upsert_result = gvfs
                .upsert_file(file_id, data.clone())
                .await
                .with_context(|| format!("Retry #{i}"));
        }

        upsert_result
            .context("Failed to upsert to GDrive")
            .with_context(|| file_id.clone())
    }

    pub(crate) async fn read_scids(&self) -> anyhow::Result<Vec<Scid>> {
        debug!("Fetching scids");
        let token = self.get_token().await?;
        self.backend_api
            .get_scids(token)
            .await
            .map(|Scids { scids }| scids)
            .context("Could not fetch scids")
    }

    pub(crate) async fn read_wallet_changeset(
        &self,
    ) -> anyhow::Result<Option<ChangeSet>> {
        let file_id =
            VfsFileId::new(SINGLETON_DIRECTORY, vfs::WALLET_CHANGESET_FILENAME);

        let maybe_changeset =
            self.read_bytes(&file_id).await?.and_then(|bytes| {
                match serde_json::from_slice::<ChangeSet>(&bytes) {
                    Ok(changeset) => Some(changeset),
                    Err(e) => {
                        // If deserialization fails, just proceed with an empty
                        // wallet, since the data will just be `full_sync`'d.
                        // TODO(max): Ideally we log the JSON structure here for
                        // debugging, but we need to preserve privacy.
                        // let changeset_json =
                        //     String::from_utf8_lossy(&changeset_bytes);
                        error!(
                            // %changeset_json,
                            "Failed to deserialize wallet changeset!! \
                             Proceeding with empty wallet: {e:#}"
                        );
                        None
                    }
                }
            });

        Ok(maybe_changeset)
    }

    pub(crate) async fn read_new_payments(
        &self,
        req: GetNewPayments,
    ) -> anyhow::Result<Vec<BasicPaymentV1>> {
        let token = self.get_token().await?;

        // NOTE: This fn is used in the app->node handler for /app/payments/new,
        // so the node->backend client code must remain until all app and
        // sdk-sidecar clients have been updated.
        #[allow(deprecated)]
        let db_payments = self
            .backend_api
            .get_new_payments(req, token.clone())
            .await
            .context("Could not fetch `DbPaymentV1`s")?
            .payments;

        // Fetch corresponding metadata
        let ids = db_payments
            .iter()
            .filter_map(|p| LxPaymentId::from_str(&p.id).ok())
            .collect::<Vec<LxPaymentId>>();
        let mut metadatas = self
            .backend_api
            .get_payment_metadata_by_ids(VecLxPaymentId { ids }, token)
            .await
            .context("Could not fetch payment metadata")?
            .metadatas
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<HashMap<String, DbPaymentMetadata>>();

        // Decrypt payments with their metadata
        db_payments
            .into_iter()
            .map(|p| {
                let created_at = p.created_at;
                let metadata = metadatas.remove(&p.id);
                let db_payment = DbPaymentV2::from_v1(p, created_at);
                payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    db_payment,
                    metadata,
                )
                .context("Could not decrypt payment")
            })
            .map(|res| {
                res.and_then(|pwm| {
                    PaymentV1::try_from(pwm)
                        .context("Failed to convert payment to v1")
                        .map(BasicPaymentV1::from)
                })
            })
            .collect::<anyhow::Result<Vec<BasicPaymentV1>>>()
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
    pub(crate) async fn read_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> anyhow::Result<Vec<BasicPaymentV2>> {
        let token = self.get_token().await?;

        // Fetch both updated payments and metadata in parallel
        let payments_req = GetUpdatedPayments {
            start_index: req.start_index,
            limit: req.limit,
        };
        let metadata_req = GetUpdatedPaymentMetadata {
            start_index: req.start_index,
            limit: req.limit,
        };

        let (payments_result, metadata_result) = tokio::join!(
            self.backend_api
                .get_updated_payments(payments_req, token.clone()),
            self.backend_api
                .get_updated_payment_metadata(metadata_req, token.clone()),
        );

        let mut db_payments = payments_result
            .context("Could not fetch updated payments")?
            .payments;
        let mut db_metadatas = metadata_result
            .context("Could not fetch updated metadata")?
            .metadatas;

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
                .backend_api
                .get_updated_payments(
                    GetUpdatedPayments {
                        start_index: req.start_index,
                        limit: req.limit,
                    },
                    token.clone(),
                )
                .await
                .context("Could not re-fetch payments")?
                .payments;
        }
        if db_metadatas.is_empty() {
            warn!(
                start_index = %DisplayOption(req.start_index),
                "Defensive re-fetch of empty metadata list"
            );
            db_metadatas = self
                .backend_api
                .get_updated_payment_metadata(
                    GetUpdatedPaymentMetadata {
                        start_index: req.start_index,
                        limit: req.limit,
                    },
                    token.clone(),
                )
                .await
                .context("Could not re-fetch metadata")?
                .metadatas;
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
            (None, None) => unreachable!("handled above"),
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
                .backend_api
                .get_payments_by_ids(
                    VecLxPaymentId {
                        ids: metadata_ids_needing_payment,
                    },
                    token.clone(),
                )
                .await
                .context("Could not fetch missing payments")?
                .payments;
            db_payments.extend(missing_payments);
        }

        if !payment_ids_needing_metadata.is_empty() {
            let missing_metadata = self
                .backend_api
                .get_payment_metadata_by_ids(
                    VecLxPaymentId {
                        ids: payment_ids_needing_metadata,
                    },
                    token,
                )
                .await
                .context("Could not fetch missing metadata")?
                .metadatas;
            db_metadatas.extend(missing_metadata);
        }

        // Build id -> payment/metadata maps for efficient pairing during
        // decrypt. Dedupe by taking the latest version since
        // db_payments/db_metadatas may contain duplicates from the
        // initial fetch, re-fetch, or by-id fetch.
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
        // Effective updated_at = max(payment_updated_idx, metadata_updated_idx)
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

        // Decrypt and return
        entries
            .into_iter()
            .map(|(id, _)| {
                let db_payment = payments_map
                    .remove(&id)
                    .context("Payment missing from map")?;
                let db_metadata = metadatas_map.remove(&id);

                let created_at = TimestampMs::try_from(db_payment.created_at)
                    .context("Invalid created_at timestamp")?;
                let updated_at = TimestampMs::try_from(db_payment.updated_at)
                    .context("Invalid updated_at timestamp")?;

                let pwm = payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    db_payment,
                    db_metadata,
                )
                .context("Could not decrypt payment")?;

                let basic_payment =
                    pwm.into_basic_payment(created_at, updated_at);
                Ok(basic_payment)
            })
            .collect::<anyhow::Result<Vec<BasicPaymentV2>>>()
    }

    pub(crate) async fn read_payment_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<BasicPaymentV2>> {
        let token = self.get_token().await?;

        let req = LxPaymentIdStruct { id };
        let (payment_result, metadata_result) = tokio::join!(
            self.backend_api.get_payment_by_id(req, token.clone()),
            self.backend_api.get_payment_metadata_by_id(req, token),
        );

        let maybe_payment = payment_result
            .context("Could not fetch payment")?
            .maybe_payment;
        let maybe_metadata = metadata_result
            .context("Could not fetch metadata")?
            .maybe_metadata;

        let maybe_basic_payment = maybe_payment
            .map(|db_payment| {
                let created_at = TimestampMs::try_from(db_payment.created_at)
                    .context("Invalid created_at timestamp")?;
                let updated_at = TimestampMs::try_from(db_payment.updated_at)
                    .context("Invalid updated_at timestamp")?;
                let pwm = payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    db_payment,
                    maybe_metadata,
                )
                .context("Could not decrypt payment")?;
                let basic_payment =
                    pwm.into_basic_payment(created_at, updated_at);
                Ok::<_, anyhow::Error>(basic_payment)
            })
            .transpose()?;

        Ok(maybe_basic_payment)
    }

    pub(crate) async fn read_payments_by_ids(
        &self,
        ids: Vec<LxPaymentId>,
    ) -> anyhow::Result<Vec<BasicPaymentV2>> {
        let token = self.get_token().await?;

        let payments = self
            .backend_api
            .get_payments_by_ids(VecLxPaymentId { ids }, token)
            .await
            .context("Could not fetch payments")?
            .payments
            .into_iter()
            .map(|payment| {
                let created_at = TimestampMs::try_from(payment.created_at)
                    .context("Invalid created_at timestamp")?;
                let updated_at = TimestampMs::try_from(payment.updated_at)
                    .context("Invalid updated_at timestamp")?;
                let pwm = payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    payment,
                    None,
                )
                .context("Could not decrypt payment")?;
                let basic_payment =
                    pwm.into_basic_payment(created_at, updated_at);
                Ok::<_, anyhow::Error>(basic_payment)
            })
            .collect::<anyhow::Result<Vec<BasicPaymentV2>>>()?;

        Ok(payments)
    }

    /// NOTE: See module docs for info on how manager/monitor persist works.
    pub(crate) async fn read_channel_manager(
        &self,
        config: UserConfig,
        channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
        keys_manager: Arc<LexeKeysManager>,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        router: Arc<RouterType>,
        message_router: Arc<MessageRouterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        debug!("Reading channel manager");
        let file_id =
            VfsFileId::new(SINGLETON_DIRECTORY, vfs::CHANNEL_MANAGER_FILENAME);

        let channel_monitor_refs = channel_monitors
            .iter()
            .map(|(_hash, monitor)| monitor)
            .collect::<Vec<_>>();
        let read_args = ChannelManagerReadArgs::new(
            keys_manager.clone(),
            keys_manager.clone(),
            keys_manager,
            fee_estimator,
            chain_monitor,
            broadcaster,
            router,
            message_router,
            logger,
            config,
            channel_monitor_refs,
        );

        // XXX(max): Read channel manager from multiple independent VSS stores
        self.read_readableargs(&file_id, read_args)
            .await
            .context("Failed to read channel manager")
    }

    /// Fetches channel monitor bytes without deserializing.
    /// This allows fetching to happen concurrently with other operations.
    pub(crate) async fn fetch_channel_monitor_bytes(
        &self,
    ) -> anyhow::Result<Vec<(VfsFileId, Vec<u8>)>> {
        debug!("Fetching channel monitor bytes");
        let dir = VfsDirectory::new(vfs::CHANNEL_MONITORS_DIR);
        self.read_dir_bytes(&dir).await
    }

    /// Deserializes channel monitors from previously fetched bytes.
    /// NOTE: See module docs for info on how manager/monitor persist works.
    pub(crate) fn deserialize_channel_monitors(
        ids_and_bytes: Vec<(VfsFileId, Vec<u8>)>,
        keys_manager: &LexeKeysManager,
    ) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
        debug!("Deserializing channel monitors");

        // XXX(max): Read channel manager from multiple independent VSS stores
        let read_args = (keys_manager, keys_manager);
        let mut values = Vec::with_capacity(ids_and_bytes.len());

        // Deserialize each channel monitor.
        for (file_id, bytes) in &ids_and_bytes {
            let mut reader = Cursor::new(bytes);
            let value =
                <(BlockHash, ChannelMonitorType)>::read(&mut reader, read_args)
                    .map_err(|err| {
                        anyhow!(
                            "ChannelMonitor deserialization failed for file: \
                             {file_id}: {err:?}"
                        )
                    })?;
            values.push(value);
        }

        // Check that each monitor's funding txo matches the file_id.
        for ((file_id, _bytes), (_blockhash, channel_monitor)) in
            ids_and_bytes.iter().zip(values.iter())
        {
            let expected_txo = LxOutPoint::from_str(&file_id.filename)
                .with_context(|| file_id.filename.clone())
                .context("Invalid funding txo string")?;
            let derived_txo = channel_monitor
                .get_funding_txo()
                .apply(|(txo, _script)| LxOutPoint::from(txo));

            ensure!(
                derived_txo == expected_txo,
                "Expected and derived txos don't match: \
                 {expected_txo} != {derived_txo}"
            );
        }

        Ok(values)
    }
}

#[async_trait]
impl Vfs for NodePersister {
    async fn get_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Option<VfsFile>, BackendApiError> {
        let token = self.get_token().await?;
        match self.backend_api.get_file(file_id, token).await {
            Ok(data) => Ok(Some(VfsFile::from_parts(file_id.clone(), data))),
            Err(BackendApiError {
                kind: BackendErrorKind::NotFound,
                ..
            }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn upsert_file(
        &self,
        file_id: &VfsFileId,
        data: bytes::Bytes,
        retries: usize,
    ) -> Result<Empty, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api
            .upsert_file_with_retries(file_id, data, token, retries)
            .await
    }

    async fn delete_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Empty, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api.delete_file(file_id, token).await
    }

    async fn list_directory(
        &self,
        dir: &VfsDirectory,
    ) -> Result<VfsDirectoryList, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api.list_directory(dir, token).await
    }

    #[inline]
    fn encrypt_ldk_writeable<W: Writeable>(
        &self,
        file_id: VfsFileId,
        writeable: &W,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_ldk_writeable(
            &mut rng,
            &self.vfs_master_key,
            file_id,
            writeable,
        )
    }

    #[inline]
    fn encrypt_json<T: Serialize>(
        &self,
        file_id: VfsFileId,
        value: &T,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_json(&mut rng, &self.vfs_master_key, file_id, value)
    }

    #[inline]
    fn encrypt_bytes(
        &self,
        file_id: VfsFileId,
        plaintext_bytes: &[u8],
    ) -> VfsFile {
        let mut rng = SysRng::new();
        persister::encrypt_bytes(
            &mut rng,
            &self.vfs_master_key,
            file_id,
            plaintext_bytes,
        )
    }

    #[inline]
    fn decrypt_file(
        &self,
        expected_file_id: &VfsFileId,
        file: VfsFile,
    ) -> anyhow::Result<Vec<u8>> {
        persister::decrypt_file(&self.vfs_master_key, expected_file_id, file)
    }
}

#[async_trait]
impl LexeInnerPersister for NodePersister {
    async fn get_pending_payments(
        &self,
    ) -> anyhow::Result<Vec<PaymentWithMetadata>> {
        let token = self.get_token().await?;

        // Fetch pending payments
        let db_payments = self
            .backend_api
            .get_pending_payments(token.clone())
            .await
            .context("Could not fetch pending `DbPaymentV2`s")?
            .payments;

        // Fetch corresponding metadata
        let ids = db_payments
            .iter()
            .filter_map(|p| LxPaymentId::from_str(&p.id).ok())
            .collect::<Vec<LxPaymentId>>();
        let mut metadatas = self
            .backend_api
            .get_payment_metadata_by_ids(VecLxPaymentId { ids }, token)
            .await
            .context("Could not fetch payment metadata")?
            .metadatas
            .into_iter()
            .map(|m| (m.id.clone(), m))
            .collect::<HashMap<String, DbPaymentMetadata>>();

        // Decrypt payments with their metadata
        db_payments
            .into_iter()
            .map(|payments| {
                let metadata = metadatas.remove(&payments.id);
                payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    payments,
                    metadata,
                )
                .context("Could not decrypt payment")
            })
            .collect()
    }

    async fn upsert_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment> {
        let mut rng = SysRng::new();

        let mut pwm = checked.0;
        let now = TimestampMs::now();
        let created_at = pwm.payment.created_at().unwrap_or(now);
        let updated_at = now;

        // Ensure the payment's created_at field is set before persisting,
        // since it may be None if this is the payment's first persist.
        pwm.payment.set_created_at_once(created_at);

        let (db_payment, db_metadata) = payments::encryption::encrypt_pwm(
            &mut rng,
            &self.vfs_master_key,
            &pwm,
            created_at,
            updated_at,
        )
        .context("Failed to encrypt payment")?;
        let token = self.get_token().await?;

        let payment_fut =
            self.backend_api.upsert_payment(db_payment, token.clone());
        let metadata_fut = async {
            match db_metadata {
                Some(m) =>
                    self.backend_api.upsert_payment_metadata(m, token).await,
                None => Ok(Empty {}),
            }
        };

        let (payment_result, metadata_result) =
            tokio::join!(payment_fut, metadata_fut);
        payment_result.context("upsert_payment API call failed")?;
        metadata_result.context("upsert_payment_metadata API call failed")?;

        Ok(PersistedPayment {
            pwm,
            created_at,
            updated_at,
        })
    }

    async fn upsert_payment_batch(
        &self,
        mut checked_batch: Vec<CheckedPayment>,
    ) -> anyhow::Result<Vec<PersistedPayment>> {
        if checked_batch.is_empty() {
            return Ok(Vec::new());
        }

        let mut rng = SysRng::new();
        let now = TimestampMs::now();
        let updated_at = now;

        let mut payments = Vec::with_capacity(checked_batch.len());
        let mut metadatas = Vec::with_capacity(checked_batch.len());

        for CheckedPayment(pwm) in checked_batch.iter_mut() {
            // Ensure the payment's created_at field is set,
            // as it may be None if this is the payment's first persist.
            let created_at = pwm.payment.created_at().unwrap_or(now);
            pwm.payment.set_created_at_once(created_at);

            let (db_payment, db_metadata) = payments::encryption::encrypt_pwm(
                &mut rng,
                &self.vfs_master_key,
                pwm,
                created_at,
                updated_at,
            )
            .context("Failed to encrypt payment")?;

            payments.push(db_payment);
            if let Some(metadata) = db_metadata {
                metadatas.push(metadata);
            }
        }

        let batch = VecDbPaymentV2 { payments };
        let token = self.get_token().await?;

        let payments_batch_fut =
            self.backend_api.upsert_payment_batch(batch, token.clone());
        let metadatas_batch_fut = async {
            if metadatas.is_empty() {
                return anyhow::Ok(());
            }

            let metadata_batch = VecDbPaymentMetadata { metadatas };
            self.backend_api
                .upsert_payment_metadata_batch(metadata_batch, token)
                .await?;
            anyhow::Ok(())
        };

        let (payments_batch_result, metadatas_batch_result) =
            tokio::join!(payments_batch_fut, metadatas_batch_fut);
        payments_batch_result
            .context("upsert_payment_batch API call failed")?;
        metadatas_batch_result
            .context("upsert_payment_metadata_batch API call failed")?;

        let persisted_batch = checked_batch
            .into_iter()
            .map(|CheckedPayment(pwm)| {
                let created_at = pwm.payment.created_at().unwrap_or(now);
                PersistedPayment {
                    pwm,
                    created_at,
                    updated_at,
                }
            })
            .collect::<Vec<PersistedPayment>>();

        Ok(persisted_batch)
    }

    async fn get_payment_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<PaymentWithMetadata>> {
        let token = self.get_token().await?;

        let req = LxPaymentIdStruct { id };
        let (try_payment, try_metadata) = tokio::join!(
            self.backend_api.get_payment_by_id(req, token.clone()),
            self.backend_api.get_payment_metadata_by_id(req, token),
        );

        let maybe_payment = try_payment
            .context("Could not fetch payment")?
            .maybe_payment;
        let maybe_metadata = try_metadata
            .context("Could not fetch payment metadata")?
            .maybe_metadata;

        let maybe_pwm = maybe_payment
            .map(|p| {
                payments::encryption::decrypt_pwm(
                    &self.vfs_master_key,
                    p,
                    maybe_metadata,
                )
            })
            .transpose()
            .context("Could not decrypt payment")?;

        if let Some(ref pwm) = maybe_pwm {
            ensure!(
                pwm.payment.id() == id,
                "ID of returned payment doesn't match"
            );
        }

        Ok(maybe_pwm)
    }

    /// NOTE: See module docs for info on how manager/monitor persist works.
    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");

        let file_id =
            VfsFileId::new(SINGLETON_DIRECTORY, vfs::CHANNEL_MANAGER_FILENAME);
        let retries = constants::IMPORTANT_PERSIST_RETRIES;

        let file = self.encrypt_ldk_writeable(file_id, channel_manager);

        // Trigger async persistence to GDrive
        self.gdrive_persister_tx
            .try_send(file.clone())
            .context("GDrive persister channel full (from manager)")?;

        // Persist to Lexe VFS
        // XXX(max): Channel manager should be persisted to multiple VSS stores.
        self.persist_file(file, retries)
            .await
            .context("Failed to persist channel manager")
    }

    /// NOTE: See module docs for info on how manager/monitor persist works.
    async fn persist_channel_monitor<PS: LexePersister>(
        &self,
        chain_monitor: &LexeChainMonitorType<PS>,
        funding_txo: &LxOutPoint,
    ) -> anyhow::Result<()> {
        let file = {
            let locked_monitor =
                chain_monitor.get_monitor((*funding_txo).into()).map_err(
                    |e| anyhow!("No monitor for this funding_txo: {e:?}"),
                )?;

            // NOTE: The VFS filename uses the `ToString` impl of `LxOutPoint`
            // rather than `lightning::chain::transaction::OutPoint` or
            // `bitcoin::OutPoint`! `LxOutPoint`'s FromStr/Display impls are
            // guaranteed to roundtrip, and will be stable across LDK versions.
            let filename = funding_txo.to_string();
            let file_id = VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, filename);
            self.encrypt_ldk_writeable(file_id, &*locked_monitor)
        };

        // Trigger async persistence to GDrive
        self.gdrive_persister_tx
            .try_send(file.clone())
            .context("GDrive persister channel full (from monitor)")?;

        // Persist to Lexe VFS
        // XXX(max): Channel monitor should be persisted to multiple VSS stores.
        let retries = constants::IMPORTANT_PERSIST_RETRIES;
        self.persist_file(file, retries)
            .await
            .context("Failed to persist channel monitor")
    }
}

/// NOTE: See module docs for info on how manager/monitor persist works.
impl Persist<SignerType> for NodePersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitorType,
    ) -> ChannelMonitorUpdateStatus {
        let kind = ChannelMonitorUpdateKind::New;
        let funding_txo = LxOutPoint::from(funding_txo);
        let update_id = monitor.get_latest_update_id();
        let update = LxChannelMonitorUpdate::new(kind, funding_txo, update_id);
        let update_span = update.span();

        update_span.in_scope(|| {
            info!("Persisting channel monitor");

            // Queue up the channel monitor update for persisting. Shut down if
            // we can't send the update for some reason.
            if let Err(e) = self.channel_monitor_persister_tx.try_send(update) {
                // NOTE: Although failing to send the channel monutor update to
                // the channel monitor persistence task is a serious error, we
                // do not return a PermanentFailure here because that force
                // closes the channel.
                error!("Fatal: Couldn't send channel monitor update: {e:#}");
                self.shutdown.send();
            }
        });

        // As documented in the `Persist` trait docs, return `InProgress`,
        // which freezes the channel until persistence succeeds.
        ChannelMonitorUpdateStatus::InProgress
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        // TODO: We may want to use the id inside for rollback protection
        update: Option<&ChannelMonitorUpdate>,
        monitor: &ChannelMonitorType,
    ) -> ChannelMonitorUpdateStatus {
        let kind = ChannelMonitorUpdateKind::Updated;
        let funding_txo = LxOutPoint::from(funding_txo);
        let update_id = update
            .as_ref()
            .map(|u| u.update_id)
            .unwrap_or_else(|| monitor.get_latest_update_id());
        let update = LxChannelMonitorUpdate::new(kind, funding_txo, update_id);
        let update_span = update.span();

        update_span.in_scope(|| {
            info!("Persisting channel monitor");

            // Queue up the channel monitor update for persisting. Shut down if
            // we can't send the update for some reason.
            if let Err(e) = self.channel_monitor_persister_tx.try_send(update) {
                // NOTE: Although failing to send the channel monutor update to
                // the channel monitor persistence task is a serious error, we
                // do not return a PermanentFailure here because that force
                // closes the channel.
                error!("Fatal: Couldn't send channel monitor update: {e:#}");
                self.shutdown.send();
            }
        });

        // As documented in the `Persist` trait docs, return `InProgress`,
        // which freezes the channel until persistence succeeds.
        ChannelMonitorUpdateStatus::InProgress
    }

    fn archive_persisted_channel(&self, funding_txo: OutPoint) {
        let backend_api = self.backend_api.clone();
        let authenticator = self.authenticator.clone();
        let vfs_master_key = self.vfs_master_key.clone();
        let maybe_google_vfs = self.google_vfs.clone();

        // LDK suggests that instead of deleting the monitor,
        // we should archive the monitor to hedge against data loss.
        let try_archive_fut = async move {
            info!("Archiving channel monitor");

            let filename = funding_txo.to_string();
            let source_file_id =
                VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, filename.clone());
            let archive_file_id =
                VfsFileId::new(vfs::CHANNEL_MONITORS_ARCHIVE_DIR, filename);

            // 1) Read and decrypt the monitor from the regular monitors dir.
            // We need to decrypt since the ciphertext is bound to its path,
            // but we can avoid a needless deserialization + reserialization.
            let source_plaintext = multi::read(
                &backend_api,
                &authenticator,
                &vfs_master_key,
                maybe_google_vfs.as_deref(),
                &source_file_id,
            )
            .await
            .context("Couldn't read source monitor")?
            .context("No source monitor exists")?;

            // 2) Reencrypt the monitor for the monitor archive namespace.
            let mut rng = SysRng::new();
            let archive_file = persister::encrypt_bytes(
                &mut rng,
                &vfs_master_key,
                archive_file_id,
                source_plaintext.expose_secret(),
            );

            // 3) Persist the monitor at the monitor archive namespace.
            multi::upsert(
                &backend_api,
                &authenticator,
                maybe_google_vfs.as_deref(),
                archive_file,
            )
            .await
            .context("Failed to upsert archive file")?;

            // 4) Finally, delete the monitor at the regular namespace.
            multi::delete(
                &backend_api,
                &authenticator,
                maybe_google_vfs.as_deref(),
                &source_file_id,
            )
            .await
            .context("Couldn't delete archived channel monitor")?;

            anyhow::Ok(())
        };

        const SPAN_NAME: &str = "(chan-monitor-archiver)";
        let task = LxTask::spawn_with_span(
            SPAN_NAME,
            info_span!(SPAN_NAME, %funding_txo),
            async move {
                match try_archive_fut.await {
                    Ok(()) => info!("Success: archived channel monitor"),
                    Err(e) => warn!("Couldn't archive monitor: {e:#}"),
                }
            },
        );
        let _ = self.eph_tasks_tx.try_send(task);
    }
}
