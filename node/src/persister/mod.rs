use std::{io::Cursor, str::FromStr, sync::Arc, time::SystemTime};

use anyhow::{anyhow, ensure, Context};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use bitcoin::hash_types::BlockHash;
use common::{
    aes::AesMasterKey,
    api::{
        auth::BearerAuthToken,
        user::{Scid, Scids},
    },
    ln::{
        channel::LxOutPoint,
        payments::{
            BasicPayment, DbPayment, LxPaymentId, PaymentIndex, VecDbPayment,
            VecLxPaymentId,
        },
    },
    rng::{Crng, SysRng},
};
use gdrive::{oauth2::GDriveCredentials, GoogleVfs, GvfsRoot};
use lexe_api::{
    auth::BearerAuthenticator,
    error::BackendApiError,
    models::command::{GetNewPayments, PaymentIndexStruct, PaymentIndexes},
    types::Empty,
    vfs::{
        self, MaybeVfsFile, VecVfsFile, Vfs, VfsDirectory, VfsFile, VfsFileId,
        CHANNEL_MANAGER_FILENAME, PW_ENC_ROOT_SEED_FILENAME,
        SINGLETON_DIRECTORY, WALLET_CHANGESET_FILENAME,
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
        self,
        manager::{CheckedPayment, PersistedPayment},
        Payment,
    },
    persister,
    traits::{LexeInnerPersister, LexePersister},
    wallet::ChangeSet,
};
use lexe_std::Apply;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::{
    chain::{
        chainmonitor::Persist, channelmonitor::ChannelMonitorUpdate,
        transaction::OutPoint, ChannelMonitorUpdateStatus,
    },
    ln::channelmanager::ChannelManagerReadArgs,
    util::{
        config::UserConfig,
        ser::{ReadableArgs, Writeable},
    },
};
use secrecy::{ExposeSecret, Secret};
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, warn};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    api::BackendApiClient,
    approved_versions::ApprovedVersions,
};

/// Data discrepancy evaluation and resolution.
mod discrepancy;
/// Logic to conduct operations on multiple data backends at the same time.
mod multi;

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
const GDRIVE_CREDENTIALS_FILENAME: &str = "gdrive_credentials";
const GVFS_ROOT_FILENAME: &str = "gvfs_root";

pub struct NodePersister {
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    google_vfs: Option<Arc<GoogleVfs>>,
    channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    shutdown: NotifyOnce,
}

/// General helper for upserting well-formed [`VfsFile`]s.
pub(crate) async fn persist_file(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    file: &VfsFile,
) -> anyhow::Result<()> {
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    backend_api
        .upsert_file(file, token)
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
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<GDriveCredentials> {
    let file_id =
        VfsFileId::new(SINGLETON_DIRECTORY, GDRIVE_CREDENTIALS_FILENAME);
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    let file = backend_api
        .get_file(&file_id, token)
        .await
        .context("Failed to fetch file")?
        .maybe_file
        .context(
            "No GDriveCredentials VFS file returned from DB; \
             perhaps it was never provisioned?",
        )?;

    let gdrive_credentials =
        persister::decrypt_json_file(vfs_master_key, &file_id, file)?;

    Ok(gdrive_credentials)
}

pub(crate) async fn persist_gvfs_root(
    rng: &mut impl Crng,
    backend_api: &(dyn BackendApiClient + Send + Sync),
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
        .upsert_file(&file, token)
        .await
        .context("Could not upsert file")?;

    Ok(())
}

pub(crate) async fn read_gvfs_root(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<Option<GvfsRoot>> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, GVFS_ROOT_FILENAME);
    let token = authenticator
        .get_token(backend_api, SystemTime::now())
        .await
        .context("Could not get auth token")?;

    let maybe_gvfs_root = backend_api
        .get_file(&file_id, token)
        .await
        .context("Failed to fetch file")?
        .maybe_file
        .map(|file| {
            persister::decrypt_json_file(vfs_master_key, &file_id, file)
        })
        .transpose()?;

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
        VfsFileId::new(SINGLETON_DIRECTORY, PW_ENC_ROOT_SEED_FILENAME);
    google_vfs.file_exists(&file_id).await
}

/// Persists the given password-encrypted [`RootSeed`] to GDrive.
/// Uses CREATE semantics (i.e. errors if the file already exists) because it
/// seems dangerous to overwrite a (possibly different) [`RootSeed`] which could
/// be storing funds.
///
/// [`RootSeed`]: common::root_seed::RootSeed
pub(crate) async fn persist_password_encrypted_root_seed(
    google_vfs: &GoogleVfs,
    encrypted_seed: Vec<u8>,
) -> anyhow::Result<()> {
    let file = VfsFile::new(
        SINGLETON_DIRECTORY,
        PW_ENC_ROOT_SEED_FILENAME,
        encrypted_seed,
    );
    google_vfs
        .create_file(file)
        .await
        .context("Failed to create root seed file")?;
    Ok(())
}

/// Read the [`ApprovedVersions`] list from Google Drive, if it exists.
pub(crate) async fn read_approved_versions(
    google_vfs: &GoogleVfs,
    vfs_master_key: &AesMasterKey,
) -> anyhow::Result<Option<ApprovedVersions>> {
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, "approved_versions");
    let maybe_file = google_vfs
        .get_file(&file_id)
        .await
        .context("Could not fetch approved versions file")?;

    let approved_versions = match maybe_file {
        Some(file) => persister::decrypt_json_file::<ApprovedVersions>(
            vfs_master_key,
            &file_id,
            file,
        )
        .context("Failed to decrypt approved versions file")?,
        None => return Ok(None),
    };

    Ok(Some(approved_versions))
}

/// Persists the given [`ApprovedVersions`] to GDrive.
pub(crate) async fn persist_approved_versions(
    rng: &mut impl Crng,
    google_vfs: &GoogleVfs,
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

    google_vfs
        .upsert_file(file)
        .await
        .context("Failed to upsert approved versions file")?;

    Ok(())
}

impl NodePersister {
    /// Initialize a [`NodePersister`].
    /// `google_vfs` MUST be [`Some`] if we are running in staging or prod.
    pub(crate) fn new(
        backend_api: Arc<dyn BackendApiClient + Send + Sync>,
        authenticator: Arc<BearerAuthenticator>,
        vfs_master_key: Arc<AesMasterKey>,
        google_vfs: Option<Arc<GoogleVfs>>,
        channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
        eph_tasks_tx: mpsc::Sender<LxTask<()>>,
        shutdown: NotifyOnce,
    ) -> Self {
        Self {
            backend_api,
            authenticator,
            vfs_master_key,
            google_vfs,
            channel_monitor_persister_tx,
            eph_tasks_tx,
            shutdown,
        }
    }

    async fn get_token(&self) -> Result<BearerAuthToken, BackendApiError> {
        self.authenticator
            .get_token(&*self.backend_api, SystemTime::now())
            .await
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
            VfsFileId::new(SINGLETON_DIRECTORY, WALLET_CHANGESET_FILENAME);

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

    pub(crate) async fn read_payments_by_indexes(
        &self,
        req: PaymentIndexes,
    ) -> anyhow::Result<Vec<BasicPayment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch `DbPayment`s
            .get_payments_by_indexes(req, token)
            .await
            .context("Could not fetch `DbPayment`s")?
            .payments
            .into_iter()
            // Decrypt into `Payment`s
            .map(|p| payments::decrypt(&self.vfs_master_key, p))
            // Convert to `BasicPayment`s
            .map(|res| res.map(BasicPayment::from))
            // Convert Vec<Result<T, E>> -> Result<Vec<T>, E>
            .collect::<anyhow::Result<Vec<BasicPayment>>>()
    }

    pub(crate) async fn read_new_payments(
        &self,
        req: GetNewPayments,
    ) -> anyhow::Result<Vec<BasicPayment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch `DbPayment`s
            .get_new_payments(req, token)
            .await
            .context("Could not fetch `DbPayment`s")?
            .payments
            .into_iter()
            // Decrypt into `Payment`s
            .map(|p| payments::decrypt(&self.vfs_master_key, p))
            // Convert to `BasicPayment`s
            .map(|res| res.map(BasicPayment::from))
            // Convert Vec<Result<T, E>> -> Result<Vec<T>, E>
            .collect::<anyhow::Result<Vec<BasicPayment>>>()
    }

    pub(crate) async fn read_channel_manager(
        &self,
        config: &ArcSwap<UserConfig>,
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
        let file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
        );

        let maybe_plaintext = multi::read(
            &*self.backend_api,
            &self.authenticator,
            &self.vfs_master_key,
            self.google_vfs.as_deref(),
            &file_id,
        )
        .await
        .context("Failed to read from GDrive and Lexe")?;

        let maybe_manager = match maybe_plaintext {
            Some(chanman_bytes) => {
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
                    **config.load(),
                    channel_monitor_refs,
                );

                let mut reader = Cursor::new(chanman_bytes.expose_secret());
                let (blockhash, channel_manager) =
                    <(BlockHash, ChannelManagerType)>::read(
                        &mut reader,
                        read_args,
                    )
                    .map_err(|e| anyhow!("{:?}", e))
                    .context("Failed to deserialize ChannelManager")?;

                Some((blockhash, channel_manager))
            }
            None => None,
        };

        Ok(maybe_manager)
    }

    pub(crate) async fn read_channel_monitors(
        &self,
        keys_manager: Arc<LexeKeysManager>,
    ) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
        debug!("Reading channel monitors");

        let dir = VfsDirectory::new(vfs::CHANNEL_MONITORS_DIR);
        let token = self.get_token().await?;

        let plaintext_pairs = match self.google_vfs {
            Some(ref gvfs) => {
                // We're running on staging/prod
                // Fetch from both Google Drive and Lexe's DB.
                let (try_google_files, try_lexe_files) = tokio::join!(
                    gvfs.get_directory(&dir),
                    self.backend_api.get_directory(&dir, token),
                );
                let google_files =
                    try_google_files.context("Failed to fetch from Google")?;
                let lexe_files = try_lexe_files
                    .context("Failed to fetch from Lexe (`Some` branch)")?;

                discrepancy::evaluate_and_resolve_all(
                    &*self.backend_api,
                    &self.authenticator,
                    &self.vfs_master_key,
                    gvfs,
                    google_files,
                    lexe_files.files,
                )
                .await
                .context("Monitor evaluation and resolution failed")?
            }
            // We're running in dev/test. Just fetch from Lexe's DB.
            None => self
                .backend_api
                .get_directory(&dir, token)
                .await
                .context("Failed to fetch from Lexe (`None` branch)")?
                .files
                .into_iter()
                .map(|file| {
                    let file_id = file.id.clone();
                    let plaintext = persister::decrypt_file(
                        &self.vfs_master_key,
                        &file_id,
                        file,
                    )
                    .map(Secret::new)?;
                    anyhow::Ok((file_id, plaintext))
                })
                .collect::<anyhow::Result<Vec<_>>>()?,
        };

        let mut result = Vec::new();

        for (file_id, plaintext) in plaintext_pairs {
            let plaintext_bytes = plaintext.expose_secret();

            let mut plaintext_reader = Cursor::new(plaintext_bytes);
            let (blockhash, channel_monitor) =
                // This is ReadableArgs::read's foreign impl on the cmon tuple
                <(BlockHash, ChannelMonitorType)>::read(
                    &mut plaintext_reader,
                    (&*keys_manager, &*keys_manager),
                )
                .map_err(|e| anyhow!("{e:?}"))
                .context("Failed to deserialize Channel Monitor")?;

            let expected_txo = LxOutPoint::from_str(&file_id.filename)
                .context("Invalid funding txo string")?;
            let derived_txo = channel_monitor
                .get_funding_txo()
                .apply(|(txo, _script)| LxOutPoint::from(txo));

            ensure!(
                derived_txo == expected_txo,
                "Expected and derived txos don't match: \
                 {expected_txo} != {derived_txo}"
            );

            result.push((blockhash, channel_monitor));
        }

        Ok(result)
    }
}

#[async_trait]
impl Vfs for NodePersister {
    async fn get_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Option<VfsFile>, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api
            .get_file(file_id, token)
            .await
            .map(|MaybeVfsFile { maybe_file }| maybe_file)
    }

    async fn upsert_file(
        &self,
        file: &VfsFile,
        retries: usize,
    ) -> Result<Empty, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api
            .upsert_file_with_retries(file, token, retries)
            .await
    }

    async fn delete_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Empty, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api.delete_file(file_id, token).await
    }

    async fn get_directory(
        &self,
        dir: &VfsDirectory,
    ) -> Result<Vec<VfsFile>, BackendApiError> {
        let token = self.get_token().await?;
        self.backend_api
            .get_directory(dir, token)
            .await
            .map(|VecVfsFile { files }| files)
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
    async fn read_pending_payments(&self) -> anyhow::Result<Vec<Payment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch pending `DbPayment`s
            .get_pending_payments(token)
            .await
            .context("Could not fetch pending `DbPayment`s")?
            .payments
            .into_iter()
            // Decrypt into `Payment`s
            .map(|p| payments::decrypt(&self.vfs_master_key, p))
            // Convert Vec<Result<T, E>> -> Result<Vec<T>, E>
            .collect::<anyhow::Result<Vec<Payment>>>()
    }

    async fn read_finalized_payment_ids(
        &self,
    ) -> anyhow::Result<Vec<LxPaymentId>> {
        let token = self.get_token().await?;
        self.backend_api
            .get_finalized_payment_ids(token)
            .await
            .map(|VecLxPaymentId { ids }| ids)
            .context("Could not get ids of finalized payments")
    }

    async fn create_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment> {
        let mut rng = common::rng::SysRng::new();

        let db_payment =
            payments::encrypt(&mut rng, &self.vfs_master_key, &checked.0);
        let token = self.get_token().await?;

        self.backend_api
            .create_payment(db_payment, token)
            .await
            .context("create_payment API call failed")?;

        Ok(PersistedPayment(checked.0))
    }

    async fn persist_payment(
        &self,
        checked: CheckedPayment,
    ) -> anyhow::Result<PersistedPayment> {
        let mut rng = common::rng::SysRng::new();

        let db_payment =
            payments::encrypt(&mut rng, &self.vfs_master_key, &checked.0);
        let token = self.get_token().await?;

        self.backend_api
            .upsert_payment(db_payment, token)
            .await
            .context("upsert_payment API call failed")?;

        Ok(PersistedPayment(checked.0))
    }

    async fn persist_payment_batch(
        &self,
        checked_batch: Vec<CheckedPayment>,
    ) -> anyhow::Result<Vec<PersistedPayment>> {
        if checked_batch.is_empty() {
            return Ok(Vec::new());
        }

        let mut rng = common::rng::SysRng::new();
        let batch = checked_batch
            .iter()
            .map(|CheckedPayment(payment)| {
                payments::encrypt(&mut rng, &self.vfs_master_key, payment)
            })
            .collect::<Vec<DbPayment>>();

        let token = self.get_token().await?;
        self.backend_api
            .upsert_payment_batch(VecDbPayment { payments: batch }, token)
            .await
            .context("upsert_payment API call failed")?;

        let persisted_batch = checked_batch
            .into_iter()
            .map(|CheckedPayment(p)| PersistedPayment(p))
            .collect::<Vec<PersistedPayment>>();
        Ok(persisted_batch)
    }

    async fn get_payment(
        &self,
        index: PaymentIndex,
    ) -> anyhow::Result<Option<Payment>> {
        let req = PaymentIndexStruct { index };
        let token = self.get_token().await?;
        let maybe_payment = self
            .backend_api
            .get_payment(req, token)
            .await
            .context("Could not fetch `DbPayment`s")?
            .maybe_payment
            // Decrypt into `Payment`
            .map(|p| payments::decrypt(&self.vfs_master_key, p))
            .transpose()
            .context("Could not decrypt payment")?;

        if let Some(ref payment) = maybe_payment {
            ensure!(
                payment.id() == index.id,
                "ID of returned payment doesn't match"
            );
        }

        Ok(maybe_payment)
    }

    async fn persist_manager<CM: Writeable + Send + Sync>(
        &self,
        channel_manager: &CM,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");

        let file_id =
            VfsFileId::new(SINGLETON_DIRECTORY, vfs::CHANNEL_MANAGER_FILENAME);
        let file = self.encrypt_ldk_writeable(file_id, channel_manager);

        multi::upsert(
            &*self.backend_api,
            &self.authenticator,
            self.google_vfs.as_deref(),
            file,
        )
        .await
        .context("multi::upsert failed")
    }

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

        multi::upsert(
            &*self.backend_api,
            &self.authenticator,
            self.google_vfs.as_deref(),
            file,
        )
        .await
        .context("multi::upsert failed")
    }
}

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
                &*backend_api,
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
            let archive_file = persister::encrypt_plaintext_bytes(
                &mut rng,
                &vfs_master_key,
                archive_file_id,
                source_plaintext.expose_secret(),
            );

            // 3) Persist the monitor at the monitor archive namespace.
            multi::upsert(
                &*backend_api,
                &authenticator,
                maybe_google_vfs.as_deref(),
                archive_file,
            )
            .await
            .context("Failed to upsert archive file")?;

            // 4) Finally, delete the monitor at the regular namespace.
            multi::delete(
                &*backend_api,
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
