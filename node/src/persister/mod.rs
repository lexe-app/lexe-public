use std::{
    io::Cursor,
    ops::Deref,
    str::FromStr,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use anyhow::{anyhow, ensure, Context};
use async_trait::async_trait;
use bitcoin::hash_types::BlockHash;
use common::{
    aes::AesMasterKey,
    api::{
        auth::{BearerAuthToken, BearerAuthenticator},
        qs::{GetNewPayments, GetPaymentByIndex, GetPaymentsByIndexes},
        vfs::{VfsDirectory, VfsFile, VfsFileId},
        Scid, User,
    },
    backoff,
    constants::{
        IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
    },
    ln::{
        channel::LxOutPoint,
        network::LxNetwork,
        payments::{BasicPayment, DbPayment, LxPaymentId, PaymentIndex},
        peer::ChannelPeer,
    },
    rng::{Crng, SysRng},
    shutdown::ShutdownChannel,
    Apply,
};
use gdrive::{oauth2::GDriveCredentials, GoogleVfs, GvfsRoot};
use lexe_ln::{
    alias::{
        BroadcasterType, ChannelMonitorType, FeeEstimatorType,
        NetworkGraphType, ProbabilisticScorerType, RouterType, SignerType,
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
    traits::LexeInnerPersister,
    wallet::db::{DbData, WalletDb},
};
use lightning::{
    chain::{
        chainmonitor::{MonitorUpdateId, Persist},
        channelmonitor::ChannelMonitorUpdate,
        transaction::OutPoint,
        ChannelMonitorUpdateStatus,
    },
    ln::channelmanager::ChannelManagerReadArgs,
    routing::{
        gossip::NetworkGraph,
        scoring::{ProbabilisticScorer, ProbabilisticScoringDecayParameters},
    },
    util::ser::{ReadableArgs, Writeable},
};
use secrecy::{ExposeSecret, Secret};
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    api::BackendApiClient,
    approved_versions::ApprovedVersions,
    channel_manager::USER_CONFIG,
};

/// Data discrepancy evaluation and resolution.
mod discrepancy;

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const SCORER_FILENAME: &str = "scorer";
const GDRIVE_CREDENTIALS_FILENAME: &str = "gdrive_credentials";
const GVFS_ROOT_FILENAME: &str = "gvfs_root";

// Non-singleton objects use a fixed directory with dynamic filenames
const CHANNEL_MONITORS_DIR: &str = "channel_monitors";

pub struct NodePersister {
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    google_vfs: Option<Arc<GoogleVfs>>,
    user: User,
    shutdown: ShutdownChannel,
    channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
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
    network: LxNetwork,
) -> bool {
    // This fn barely does anything, but we want it close to the impl of
    // `persist_password_encrypted_root_seed` for ez inspection
    let filename = format!("{network}_root_seed");
    let file_id = VfsFileId::new(SINGLETON_DIRECTORY, filename);
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
    network: LxNetwork,
    encrypted_seed: Vec<u8>,
) -> anyhow::Result<()> {
    // We include network in the filename as a safeguard against mixing seeds up
    let filename = format!("{network}_root_seed");
    let file = VfsFile::new(SINGLETON_DIRECTORY, filename, encrypted_seed);

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
        user: User,
        shutdown: ShutdownChannel,
        channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    ) -> Self {
        Self {
            backend_api,
            authenticator,
            vfs_master_key,
            google_vfs,
            user,
            shutdown,
            channel_monitor_persister_tx,
        }
    }

    /// Sugar for calling [`persister::encrypt_ldk_writeable`].
    #[inline]
    fn encrypt_ldk_writeable(
        &self,
        dirname: impl Into<String>,
        filename: impl Into<String>,
        writeable: &impl Writeable,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        let vfile_id = VfsFileId::new(dirname.into(), filename.into());
        persister::encrypt_ldk_writeable(
            &mut rng,
            &self.vfs_master_key,
            vfile_id,
            writeable,
        )
    }

    async fn get_token(&self) -> anyhow::Result<BearerAuthToken> {
        self.authenticator
            .get_token(&*self.backend_api, SystemTime::now())
            .await
            .context("Could not get auth token")
    }

    pub(crate) async fn read_scid(&self) -> anyhow::Result<Option<Scid>> {
        debug!("Fetching scid");
        let token = self.get_token().await?;
        self.backend_api
            .get_scid(self.user.node_pk, token)
            .await
            .context("Could not fetch scid")
    }

    pub(crate) async fn read_wallet_db(
        &self,
        wallet_db_persister_tx: mpsc::Sender<()>,
    ) -> anyhow::Result<WalletDb> {
        debug!("Reading wallet db");
        let file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            WALLET_DB_FILENAME.to_owned(),
        );
        let token = self.get_token().await?;

        let maybe_file = self
            .backend_api
            .get_file(&file_id, token)
            .await
            .context("Could not fetch wallet db from db")?;

        let wallet_db = match maybe_file {
            Some(file) => {
                debug!("Decrypting and deserializing existing wallet db");
                let db_data = persister::decrypt_json_file::<DbData>(
                    &self.vfs_master_key,
                    &file_id,
                    file,
                )?;

                WalletDb::from_inner(db_data, wallet_db_persister_tx)
            }
            None => {
                debug!("No wallet db found, creating a new one");

                WalletDb::new(wallet_db_persister_tx)
            }
        };

        Ok(wallet_db)
    }

    pub(crate) async fn read_payments_by_indexes(
        &self,
        req: GetPaymentsByIndexes,
    ) -> anyhow::Result<Vec<BasicPayment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch `DbPayment`s
            .get_payments_by_indexes(req, token)
            .await
            .context("Could not fetch `DbPayment`s")?
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
        channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
        keys_manager: Arc<LexeKeysManager>,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        router: Arc<RouterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        debug!("Reading channel manager");
        let file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
        );

        let maybe_plaintext = read_from_gdrive_and_lexe(
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
                let mut state_buf = Cursor::new(chanman_bytes.expose_secret());

                let mut channel_monitor_mut_refs = Vec::new();
                for (_, channel_monitor) in channel_monitors.iter_mut() {
                    channel_monitor_mut_refs.push(channel_monitor);
                }
                let read_args = ChannelManagerReadArgs::new(
                    keys_manager.clone(),
                    keys_manager.clone(),
                    keys_manager,
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    router,
                    logger,
                    USER_CONFIG,
                    channel_monitor_mut_refs,
                );

                let (blockhash, channel_manager) = <(
                    BlockHash,
                    ChannelManagerType,
                )>::read(
                    &mut state_buf, read_args
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize ChannelManager")?;

                Some((blockhash, channel_manager))
            }
            None => None,
        };

        Ok(maybe_manager)
    }

    // Replaces equivalent method in lightning_persister::FilesystemPersister
    pub(crate) async fn read_channel_monitors(
        &self,
        keys_manager: Arc<LexeKeysManager>,
    ) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
        debug!("Reading channel monitors");

        let dir = VfsDirectory::new(CHANNEL_MONITORS_DIR);
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
                    lexe_files,
                )
                .await
                .context("Monitor evaluation and resolution failed")?
            }
            // We're running in dev/test. Just fetch from Lexe's DB.
            None => {
                let files =
                    self.backend_api
                        .get_directory(&dir, token)
                        .await
                        .context("Failed to fetch from Lexe (`None` branch)")?;
                files
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
                    .collect::<anyhow::Result<Vec<_>>>()?
            }
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

    pub(crate) async fn read_scorer(
        &self,
        graph: Arc<NetworkGraphType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<ProbabilisticScorerType> {
        debug!("Reading probabilistic scorer");
        let params = ProbabilisticScoringDecayParameters::default();

        let file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            SCORER_FILENAME.to_owned(),
        );
        let token = self.get_token().await?;

        let maybe_file = self
            .backend_api
            .get_file(&file_id, token)
            .await
            .context("Could not fetch probabilistic scorer from DB")?;

        let scorer = match maybe_file {
            Some(file) => {
                let data = persister::decrypt_file(
                    &self.vfs_master_key,
                    &file_id,
                    file,
                )?;
                let mut state_buf = Cursor::new(&data);

                ProbabilisticScorer::read(
                    &mut state_buf,
                    (params, Arc::clone(&graph), logger),
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize ProbabilisticScorer")?
            }
            None => ProbabilisticScorer::new(params, graph, logger),
        };

        Ok(scorer)
    }

    pub(crate) async fn read_network_graph(
        &self,
        network: LxNetwork,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<NetworkGraphType> {
        debug!("Reading network graph");
        let file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            NETWORK_GRAPH_FILENAME.to_owned(),
        );
        let token = self.get_token().await?;

        let maybe_file = self
            .backend_api
            .get_file(&file_id, token)
            .await
            .context("Could not fetch network graph from DB")?;

        let network_graph = match maybe_file {
            Some(file) => {
                let data = persister::decrypt_file(
                    &self.vfs_master_key,
                    &file_id,
                    file,
                )?;
                let mut state_buf = Cursor::new(&data);

                NetworkGraph::read(&mut state_buf, logger.clone())
                    // LDK DecodeError is Debug but doesn't impl
                    // std::error::Error
                    .map_err(|e| anyhow!("{e:?}"))
                    .context("Failed to deserialize NetworkGraph")?
            }
            None => NetworkGraph::new(network.to_bitcoin(), logger),
        };

        Ok(network_graph)
    }
}

#[async_trait]
impl LexeInnerPersister for NodePersister {
    #[inline]
    fn encrypt_json(
        &self,
        dirname: impl Into<String>,
        filename: impl Into<String>,
        value: &impl Serialize,
    ) -> VfsFile {
        let mut rng = SysRng::new();
        let vfile_id = VfsFileId::new(dirname.into(), filename.into());
        persister::encrypt_json(&mut rng, &self.vfs_master_key, vfile_id, value)
    }

    async fn persist_file(
        &self,
        file: VfsFile,
        retries: usize,
    ) -> anyhow::Result<()> {
        let dirname = &file.id.dir.dirname;
        let filename = &file.id.filename;
        let bytes = file.data.len();
        debug!("Persisting file {dirname}/{filename} <{bytes} bytes>");
        let token = self.get_token().await?;

        self.backend_api
            .upsert_file_with_retries(&file, token, retries)
            .await
            .map(|_| ())
            .context("Could not persist basic file")
    }

    async fn persist_manager<W: Writeable + Send + Sync>(
        &self,
        channel_manager: &W,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");

        let file = self.encrypt_ldk_writeable(
            SINGLETON_DIRECTORY,
            CHANNEL_MANAGER_FILENAME,
            channel_manager,
        );

        upsert_to_gdrive_and_lexe(
            &*self.backend_api,
            &self.authenticator,
            self.google_vfs.as_deref(),
            file,
        )
        .await
        .context("upsert_to_gdrive_and_lexe failed")
    }

    async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()> {
        debug!("Persisting network graph");
        let token = self.get_token().await?;

        let file = self.encrypt_ldk_writeable(
            SINGLETON_DIRECTORY,
            NETWORK_GRAPH_FILENAME,
            network_graph,
        );

        self.backend_api
            .upsert_file(&file, token)
            .await
            .map(|_| ())
            .context("Could not persist network graph")
    }

    async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()> {
        debug!("Persisting probabilistic scorer");
        let token = self.get_token().await?;

        let file = self.encrypt_ldk_writeable(
            SINGLETON_DIRECTORY,
            SCORER_FILENAME,
            scorer_mutex.lock().unwrap().deref(),
        );

        self.backend_api
            .upsert_file(&file, token)
            .await
            .map(|_| ())
            .context("Could not persist scorer")
    }

    async fn persist_channel_peer(
        &self,
        _channel_peer: ChannelPeer,
    ) -> anyhow::Result<()> {
        // User nodes only ever have one channel peer (the LSP), whose address
        // often changes in between restarts, so there is nothing to do here.
        Ok(())
    }

    async fn read_pending_payments(&self) -> anyhow::Result<Vec<Payment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch pending `DbPayment`s
            .get_pending_payments(token)
            .await
            .context("Could not fetch pending `DbPayment`s")?
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
            .upsert_payment_batch(batch, token)
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
        let req = GetPaymentByIndex { index };
        let token = self.get_token().await?;
        let maybe_payment = self
            .backend_api
            .get_payment(req, token)
            .await
            .context("Could not fetch `DbPayment`s")?
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
}

impl Persist<SignerType> for NodePersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitorType,
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        let funding_txo = LxOutPoint::from(funding_txo);
        info!("Persisting new channel {funding_txo}");

        let file = self.encrypt_ldk_writeable(
            CHANNEL_MONITORS_DIR,
            funding_txo.to_string(),
            monitor,
        );

        // Generate a future for making a few attempts to persist the channel
        // monitor. It will be executed by the channel monitor persistence task.
        //
        // NOTE that despite this being a new monitor, we upsert instead of
        // create due to an occasional race where a channel monitor persist
        // succeeds but the node shuts down before the channel manager is
        // repersisted, causing the create_file call to fail at the next boot.
        let api_call_fut = Box::pin({
            let backend_api = self.backend_api.clone();
            let authenticator = self.authenticator.clone();
            let maybe_google_vfs = self.google_vfs.clone();
            async move {
                upsert_to_gdrive_and_lexe(
                    &*backend_api,
                    &authenticator,
                    maybe_google_vfs.as_deref(),
                    file,
                )
                .await
                .context("Failed to persist new channel monitor")
            }
        });

        let sequence_num = None;
        let kind = ChannelMonitorUpdateKind::New;

        let update = LxChannelMonitorUpdate {
            funding_txo,
            update_id,
            api_call_fut,
            sequence_num,
            kind,
        };

        // Queue up the channel monitor update for persisting. Shut down if we
        // can't send the update for some reason.
        if let Err(e) = self.channel_monitor_persister_tx.try_send(update) {
            // NOTE: Although failing to send the channel monutor update to the
            // channel monitor persistence task is a serious error, we do not
            // return a PermanentFailure here because that force closes the
            // channel, when it is much more likely that it's simply just been
            // too long since the last time we synced to the chain tip.
            error!("Fatal error: Couldn't send channel monitor update: {e:#}");
            self.shutdown.send();
        }

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
        update_id: MonitorUpdateId,
    ) -> ChannelMonitorUpdateStatus {
        let funding_txo = LxOutPoint::from(funding_txo);
        info!("Updating persisted channel {funding_txo}");

        let file = self.encrypt_ldk_writeable(
            CHANNEL_MONITORS_DIR,
            funding_txo.to_string(),
            monitor,
        );

        // Generate a future for making a few attempts to persist the channel
        // monitor. It will be executed by the channel monitor persistence task.
        let api_call_fut = Box::pin({
            let backend_api = self.backend_api.clone();
            let authenticator = self.authenticator.clone();
            let maybe_google_vfs = self.google_vfs.clone();
            async move {
                upsert_to_gdrive_and_lexe(
                    &*backend_api,
                    &authenticator,
                    maybe_google_vfs.as_deref(),
                    file,
                )
                .await
                .context("Failed to persist updated channel monitor")
            }
        });

        let sequence_num = update.as_ref().map(|u| u.update_id);
        let kind = ChannelMonitorUpdateKind::Updated;

        let update = LxChannelMonitorUpdate {
            funding_txo,
            update_id,
            api_call_fut,
            sequence_num,
            kind,
        };

        // Queue up the channel monitor update for persisting. Shut down if we
        // can't send the update for some reason.
        if let Err(e) = self.channel_monitor_persister_tx.try_send(update) {
            // NOTE: Although failing to send the channel monutor update to the
            // channel monitor persistence task is a serious error, we do not
            // return a PermanentFailure here because that force closes the
            // channel, when it is much more likely that it's simply just been
            // too long since the last time we synced to the chain tip.
            error!("Fatal error: Couldn't send channel monitor update: {e:#}");
            self.shutdown.send();
        }

        // As documented in the `Persist` trait docs, return `InProgress`,
        // which freezes the channel until persistence succeeds.
        ChannelMonitorUpdateStatus::InProgress
    }
}

/// Helper to read a VFS from both Google Drive and Lexe's DB.
/// The read from GDrive is skipped if `maybe_google_vfs` is [`None`].
async fn read_from_gdrive_and_lexe(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    maybe_google_vfs: Option<&GoogleVfs>,
    file_id: &VfsFileId,
) -> anyhow::Result<Option<Secret<Vec<u8>>>> {
    let read_from_lexe = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .get_file(file_id, token)
            .await
            .with_context(|| format!("{file_id}"))
            .context("Couldn't fetch from Lexe")
    };

    let maybe_plaintext = match maybe_google_vfs {
        Some(gvfs) => {
            let read_from_google = async {
                gvfs.get_file(file_id)
                    .await
                    .with_context(|| format!("{file_id}"))
                    .context("Couldn't fetch from Google")
            };

            let (try_maybe_google_file, try_maybe_lexe_file) =
                tokio::join!(read_from_google, read_from_lexe);
            let maybe_google_file = try_maybe_google_file?;
            let maybe_lexe_file = try_maybe_lexe_file?;

            discrepancy::evaluate_and_resolve(
                backend_api,
                authenticator,
                vfs_master_key,
                gvfs,
                file_id,
                maybe_google_file,
                maybe_lexe_file,
            )
            .await
            .context("Evaluation and resolution failed")?
        }
        None => {
            let maybe_lexe_file = read_from_lexe.await?;
            maybe_lexe_file
                .map(|file| {
                    persister::decrypt_file(vfs_master_key, file_id, file)
                        .map(Secret::new)
                        .context("Failed to decrypt file")
                })
                .transpose()?
        }
    };

    Ok(maybe_plaintext)
}

/// Helper to upsert an important VFS file to both Google Drive and Lexe's DB.
///
/// - The upsert to GDrive is skipped if `maybe_google_vfs` is [`None`].
/// - Up to [`IMPORTANT_PERSIST_RETRIES`] additional attempts will be made if
///   the first attempt fails.
async fn upsert_to_gdrive_and_lexe(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    maybe_google_vfs: Option<&GoogleVfs>,
    file: VfsFile,
) -> anyhow::Result<()> {
    let google_upsert_future = async {
        match maybe_google_vfs {
            Some(gvfs) => {
                let mut try_upsert = gvfs
                    .upsert_file(file.clone())
                    .await
                    .context("(First attempt)");

                let mut backoff_iter = backoff::get_backoff_iter();
                for i in 0..IMPORTANT_PERSIST_RETRIES {
                    if try_upsert.is_ok() {
                        break;
                    }

                    tokio::time::sleep(backoff_iter.next().unwrap()).await;

                    try_upsert = gvfs
                        .upsert_file(file.clone())
                        .await
                        .with_context(|| format!("(Retry #{i})"));
                }

                try_upsert.context("Failed to upsert to GVFS")
            }
            None => Ok(()),
        }
    };
    let lexe_upsert_future = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .upsert_file_with_retries(&file, token, IMPORTANT_PERSIST_RETRIES)
            .await
            .map(|_| ())
            .context("Failed to upsert to Lexe DB")
    };

    // Since Google is the source of truth (and upserting to Google is more
    // likely to fail), do Google first. This introduces some latency but
    // prevents us from having to roll back Lexe's state if it fails.
    google_upsert_future.await?;
    lexe_upsert_future.await?;

    Ok(())
}

/// Helper to delete a VFS file from both Google Drive and Lexe's DB.
/// The deletion from GDrive is skipped if `maybe_google_vfs` is [`None`].
async fn delete_from_gdrive_and_lexe(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    maybe_google_vfs: Option<&GoogleVfs>,
    file_id: &VfsFileId,
) -> anyhow::Result<()> {
    let delete_from_google = async {
        match maybe_google_vfs {
            Some(gvfs) => gvfs
                .delete_file(file_id)
                .await
                .with_context(|| format!("{file_id}"))
                .context("Couldn't delete from Google"),
            None => Ok(()),
        }
    };
    let delete_from_lexe = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .delete_file(file_id, token)
            .await
            .with_context(|| format!("{file_id}"))
            .context("Couldn't delete from Lexe")
    };

    let (try_delete_google_file, try_delete_lexe_file) =
        tokio::join!(delete_from_google, delete_from_lexe);
    try_delete_google_file?;
    try_delete_lexe_file?;

    Ok(())
}
