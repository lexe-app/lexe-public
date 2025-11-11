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

use std::{io::Cursor, str::FromStr, sync::Arc, time::SystemTime};

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
        GetNewPayments, GetUpdatedPayments, LxPaymentIdStruct, VecLxPaymentId,
    },
    types::{
        Empty,
        payments::{
            BasicPaymentV1, BasicPaymentV2, LxPaymentId, VecDbPaymentV2,
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
        self, Payment,
        manager::{CheckedPayment, PersistedPayment},
    },
    persister,
    traits::{LexeInnerPersister, LexePersister},
    wallet::ChangeSet,
};
use lexe_std::{Apply, backoff};
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
        self.backend_api
            .get_new_payments(req, token)
            .await
            .context("Could not fetch `DbPaymentV1`s")?
            .payments
            .into_iter()
            .map(|p| payments::decrypt(&self.vfs_master_key, p.data))
            .map(|res| res.map(BasicPaymentV1::from))
            .collect::<anyhow::Result<Vec<BasicPaymentV1>>>()
    }

    pub(crate) async fn read_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> anyhow::Result<Vec<BasicPaymentV2>> {
        // TODO(max): We will have to do some zipping with payment metadata.
        let token = self.get_token().await?;
        self.backend_api
            .get_updated_payments(req, token)
            .await
            .context("Could not fetch `DbPaymentV2`s")?
            .payments
            .into_iter()
            .map(|db_payment| {
                let created_at = TimestampMs::try_from(db_payment.created_at)
                    .context("Invalid created_at timestamp")?;
                let updated_at = TimestampMs::try_from(db_payment.updated_at)
                    .context("Invalid updated_at timestamp")?;
                let payment =
                    payments::decrypt(&self.vfs_master_key, db_payment.data)?;
                let basic_payment =
                    payment.into_basic_payment(created_at, updated_at);
                Ok(basic_payment)
            })
            .collect::<anyhow::Result<Vec<BasicPaymentV2>>>()
    }

    pub(crate) async fn read_payment_by_id(
        &self,
        id: LxPaymentId,
    ) -> anyhow::Result<Option<BasicPaymentV2>> {
        let token = self.get_token().await?;
        let maybe_payment = self
            .backend_api
            .get_payment_by_id(LxPaymentIdStruct { id }, token)
            .await
            .context("Could not fetch payment")?
            .maybe_payment
            .map(|db_payment| {
                let created_at = TimestampMs::try_from(db_payment.created_at)
                    .context("Invalid created_at timestamp")?;
                let updated_at = TimestampMs::try_from(db_payment.updated_at)
                    .context("Invalid updated_at timestamp")?;
                let payment =
                    payments::decrypt(&self.vfs_master_key, db_payment.data)?;
                let basic_payment =
                    payment.into_basic_payment(created_at, updated_at);
                Ok::<_, anyhow::Error>(basic_payment)
            })
            .transpose()?;

        Ok(maybe_payment)
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
                let payment =
                    payments::decrypt(&self.vfs_master_key, payment.data)?;
                let basic_payment =
                    payment.into_basic_payment(created_at, updated_at);
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
    async fn get_pending_payments(&self) -> anyhow::Result<Vec<Payment>> {
        let token = self.get_token().await?;
        self.backend_api
            .get_pending_payments(token)
            .await
            .context("Could not fetch pending `DbPaymentV2`s")?
            .payments
            .into_iter()
            .map(|p| payments::decrypt(&self.vfs_master_key, p.data))
            .collect::<anyhow::Result<Vec<Payment>>>()
    }

    async fn upsert_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment> {
        let mut rng = SysRng::new();

        let payment = checked.0;
        let created_at = payment.created_at();
        let updated_at = TimestampMs::now();
        let db_payment = payments::encrypt(
            &mut rng,
            &self.vfs_master_key,
            &payment,
            updated_at,
        );
        let token = self.get_token().await?;

        self.backend_api
            .upsert_payment(db_payment, token)
            .await
            .context("upsert_payment API call failed")?;

        Ok(PersistedPayment {
            payment,
            created_at,
            updated_at,
        })
    }

    async fn upsert_payment_batch(
        &self,
        checked_batch: Vec<CheckedPayment>,
    ) -> anyhow::Result<Vec<PersistedPayment>> {
        if checked_batch.is_empty() {
            return Ok(Vec::new());
        }

        let mut rng = SysRng::new();
        let updated_at = TimestampMs::now();
        let payments = checked_batch
            .iter()
            .map(|CheckedPayment(payment)| {
                payments::encrypt(
                    &mut rng,
                    &self.vfs_master_key,
                    payment,
                    updated_at,
                )
            })
            .collect();
        let batch = VecDbPaymentV2 { payments };

        let token = self.get_token().await?;
        self.backend_api
            .upsert_payment_batch(batch, token)
            .await
            .context("upsert_payment API call failed")?;

        let persisted_batch = checked_batch
            .into_iter()
            .map(|CheckedPayment(payment)| {
                let created_at = payment.created_at();
                PersistedPayment {
                    payment,
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
    ) -> anyhow::Result<Option<Payment>> {
        let req = LxPaymentIdStruct { id };
        let token = self.get_token().await?;
        let maybe_payment = self
            .backend_api
            .get_payment_by_id(req, token)
            .await
            .context("Could not fetch payment")?
            .maybe_payment
            // Decrypt into `Payment`
            .map(|p| payments::decrypt(&self.vfs_master_key, p.data))
            .transpose()
            .context("Could not decrypt payment")?;

        if let Some(payment) = &maybe_payment {
            ensure!(payment.id() == id, "ID of returned payment doesn't match");
        }

        Ok(maybe_payment)
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
