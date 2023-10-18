use std::{
    collections::{HashMap, HashSet},
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
        error::{NodeApiError, NodeErrorKind},
        qs::{GetNewPayments, GetPaymentByIndex, GetPaymentsByIds},
        vfs::{VfsDirectory, VfsFile, VfsFileId},
        Scid, User,
    },
    backoff,
    cli::Network,
    constants::{
        IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
    },
    ln::{
        channel::LxOutPoint,
        payments::{BasicPayment, DbPayment, LxPaymentId, PaymentIndex},
        peer::ChannelPeer,
    },
    rng::{Crng, SysRng},
    shutdown::ShutdownChannel,
    Apply,
};
use futures::future::TryFutureExt;
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
use serde::Serialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    api::BackendApiClient,
    channel_manager::USER_CONFIG,
};

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const SCORER_FILENAME: &str = "scorer";
const GDRIVE_CREDENTIALS_FILENAME: &str = "gdrive_credentials";
const GVFS_ROOT_FILENAME: &str = "gvfs_root";

// Non-singleton objects use a fixed directory with dynamic filenames
pub(crate) const CHANNEL_MONITORS_DIRECTORY: &str = "channel_monitors";

pub struct NodePersister {
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    google_vfs: Option<Arc<GoogleVfs>>,
    user: User,
    shutdown: ShutdownChannel,
    channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
}

/// Encrypts the given [`GDriveCredentials`] and upserts it into Lexe's DB.
// This function is only used during provisioning (hence why we return
// NodeErrorKind::Provision), but we define it here so that its implementation
// is not separated from `read_gdrive_credentials`.
pub(crate) async fn persist_gdrive_credentials(
    rng: &mut impl Crng,
    backend_api: &(dyn BackendApiClient + Send + Sync),
    vfs_master_key: &AesMasterKey,
    credentials: &GDriveCredentials,
    token: BearerAuthToken,
) -> Result<(), NodeApiError> {
    let file_id =
        VfsFileId::new(SINGLETON_DIRECTORY, GDRIVE_CREDENTIALS_FILENAME);
    let file =
        persister::encrypt_json(rng, vfs_master_key, file_id, &credentials);

    backend_api
        .upsert_file(&file, token)
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Provision,
            msg: format!("Could not persist GDrive credentials: {e:#}"),
        })?;

    Ok(())
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
    network: Network,
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
    network: Network,
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

impl NodePersister {
    /// Initialize a [`NodePersister`].
    /// `google_vfs` MUST be [`Some`] if we are running on testnet or mainnet.
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

    pub(crate) async fn read_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
    ) -> anyhow::Result<Vec<BasicPayment>> {
        let token = self.get_token().await?;
        self.backend_api
            // Fetch `DbPayment`s
            .get_payments_by_ids(req, token)
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
        let token = self.get_token().await?;

        let maybe_chanman_bytes = match self.google_vfs {
            Some(ref gvfs) => {
                // Fetch the channel manager from Google *and* Lexe.
                let (try_maybe_google_file, try_maybe_lexe_file) = tokio::join!(
                    gvfs.get_file(&file_id),
                    self.backend_api.get_file(&file_id, token),
                );
                let maybe_google_file = try_maybe_google_file
                    .context("Error fetching from Google")?;
                let maybe_lexe_file = try_maybe_lexe_file
                    .context("Error fetching from Lexe (`Some` branch)")?;

                self.decrypt_channel_manager_and_fix_errors(
                    &file_id,
                    gvfs,
                    maybe_google_file,
                    maybe_lexe_file,
                )
                .await
                .context("Chan man decryption or error resolution failed")?
            }
            // We're running in dev/test. Only fetch from Lexe's DB.
            None => {
                let maybe_file = self
                    .backend_api
                    .get_file(&file_id, token)
                    .await
                    .context("Error fetching from Lexe (`None` branch)")?;
                maybe_file
                    .map(|file| {
                        persister::decrypt_file(
                            &self.vfs_master_key,
                            &file_id,
                            file,
                        )
                        .context("Failed to decrypt file (`None` branch)")
                    })
                    .transpose()?
            }
        };

        let maybe_manager = match maybe_chanman_bytes {
            Some(chanman_bytes) => {
                let mut state_buf = Cursor::new(&chanman_bytes);

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

        let dir = VfsDirectory::new(CHANNEL_MONITORS_DIRECTORY);
        let token = self.get_token().await?;

        let all_files = match self.google_vfs {
            Some(ref gvfs) => {
                // We're running on testnet/mainnet.
                // Fetch from both Google Drive and Lexe's DB.
                let (try_google_files, try_lexe_files) = tokio::join!(
                    gvfs.get_directory(&dir),
                    self.backend_api.get_directory(&dir, token),
                );
                let google_files =
                    try_google_files.context("Failed to fetch from Google")?;
                let lexe_files = try_lexe_files
                    .context("Failed to fetch from Lexe (`Some` branch)")?;

                self.compare_channel_monitors_and_fix_errors(
                    gvfs,
                    google_files,
                    lexe_files,
                )
                .await
                .context("Monitor error resolution failed")?
            }
            // We're running in dev/test. Just fetch from Lexe's DB.
            None => self
                .backend_api
                .get_directory(&dir, token)
                .await
                .context("Failed to fetch from Lexe (`None` branch)")?,
        };

        let mut result = Vec::new();

        for file in all_files {
            let given = LxOutPoint::from_str(&file.id.filename)
                .context("Invalid funding txo string")?;

            let file_id = VfsFileId {
                dir: dir.clone(),
                filename: file.id.filename.clone(),
            };
            let data =
                persister::decrypt_file(&self.vfs_master_key, &file_id, file)?;
            let mut state_buf = Cursor::new(&data);

            let (blockhash, channel_monitor) =
                // This is ReadableArgs::read's foreign impl on the cmon tuple
                <(BlockHash, ChannelMonitorType)>::read(
                    &mut state_buf,
                    (&*keys_manager, &*keys_manager),
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize Channel Monitor")?;

            let (derived, _script) = channel_monitor.get_funding_txo();
            ensure!(derived.txid == given.txid.0, "outpoint txid don' match");
            ensure!(derived.index == given.index, "outpoint index don' match");

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
        network: Network,
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
            None => NetworkGraph::new(network.0, logger),
        };

        Ok(network_graph)
    }

    /// Given the [`Option<VfsFile>`]s for the channel manager returned to us by
    /// both Google and Lexe, get the contained decrypted channel manager bytes.
    ///
    /// - If the files are the same, we decrypt either one and return the bytes.
    /// - If the files are different, or one exists and the other doesn't, we'll
    ///   reason about likely scenarios and resolve the discrepancy accordingly.
    /// - If no files were returned, we return [`None`].
    ///
    /// This function must ONLY be called by [`read_channel_manager`], as its
    /// reasoning is tightly coupled with assumptions relating to the channel
    /// manager. It is extracted as a helper primarily so that the original
    /// function body is easier to read.
    ///
    /// [`read_channel_manager`]: Self::read_channel_manager
    async fn decrypt_channel_manager_and_fix_errors(
        &self,
        file_id: &VfsFileId,
        gvfs: &GoogleVfs,
        maybe_google_file: Option<VfsFile>,
        maybe_lexe_file: Option<VfsFile>,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        if maybe_google_file == maybe_lexe_file {
            // Encrypted files match, therefore the contents match.
            // Proceed to decrypt either one of them.
            return maybe_google_file
                .map(|file| {
                    persister::decrypt_file(&self.vfs_master_key, file_id, file)
                })
                .transpose();
        }
        // Uh oh! There was a discrepancy between Google and Lexe.
        // We'll have to do extra work to fix it.

        // Decrypt the VFS files, thereby validating and authenticating them. We
        // clone before decryption so that we don't have to re-encrypt the files
        // later when we fix the discrepancy.
        let maybe_google_bytes = maybe_google_file
            .clone()
            .map(|file| {
                persister::decrypt_file(&self.vfs_master_key, file_id, file)
                    .context("Failed to decrypt file from Google")
            })
            .transpose()?;
        let maybe_lexe_bytes = maybe_lexe_file
            .clone()
            .map(|file| {
                persister::decrypt_file(&self.vfs_master_key, file_id, file)
                    .context("Failed to decrypt file from Lexe")
            })
            .transpose()?;

        // Fix the discrepancy.
        match (maybe_google_file, maybe_lexe_file) {
            (Some(google_file), Some(_lexe_file)) => {
                // The file was found in both GDrive and in Lexe's DB, but they
                // were different. This could happen if a channel manager
                // persist has a partial failure, updating one copy but not the
                // other. GDrive offers rollback protection, so it is the
                // primary source of truth. Update Lexe with the Google version.
                warn!(
                    "Data out of sync between Google and Lexe; \
                      updating Lexe's version"
                );
                let token = self.get_token().await?;
                self.backend_api
                    .upsert_file(&google_file, token)
                    .await
                    .context("Failed to correct Lexe DB version")?;
            }
            (Some(ref google_file), None) => {
                // The channel manager was found in GDrive but not in Lexe's DB.
                // This should basically never happen unless (1) Lexe somehow
                // lost their data or (2) the user requested to delete their
                // account but somehow added the data back to their My Drive.
                // Let's make some noise and copy the data into Lexe's DB.
                error!(
                    "Channel manager found in gdrive but not in Lexe's DB; /
                    copying to Lexe's DB"
                );
                let token = self.get_token().await?;
                self.backend_api
                    .create_file(google_file, token)
                    .await
                    .context("Failed to correct Lexe DB version")?;
            }
            (None, Some(lexe_file)) => {
                // Lexe has a valid channel manager in its DB, yet none was
                // returned from a successful API call to Google.
                // In other words, the user exists but they are missing
                // *critical* information in their Google Drive.
                //
                // We assume that the user has lost their backup (by deleting
                // the LexeData dir in My Drive despite all the "DO NOT DELETE"
                // warnings to the contrary) and needs help with funds recovery.
                // At this point, the user has no choice but to use the channel
                // manager from Lexe's DB (which may have been rolled back).
                //
                // We'll make some noise then copy Lexe's channel manager back
                // into the user's Google Drive.
                error!(
                    "Channel manager found in Lexe's DB but not in \
                    Google Drive. The user appears to have lost their \
                    backup. To prevent potential funds loss, we will \
                    recover using the channel manager from Lexe's DB."
                );
                gvfs.create_file(lexe_file)
                    .await
                    .context("Failed to copy Lexe's data to GDrive")?;
            }
            (None, None) => unreachable!("Early exit checked for equality"),
        }

        // For security, the GDrive version always takes precedence.
        Ok(maybe_google_bytes.or(maybe_lexe_bytes))
    }

    /// Compares the channel monitor files returned by both Google and Lexe,
    /// returning a [`Vec<VfsFile>`] which we consider to be the "correct" set
    /// of monitors.
    ///
    /// If Google and Lexe returned different results, we'll reason about likely
    /// scenarios in each case and resolve the discrepancies accordingly.
    ///
    /// The logic is very similar to [`decrypt_channel_manager_and_fix_errors`],
    /// but we should resist the urge to extract a function that abstracts over
    /// both, since there are nuanced differences between them with important
    /// security implications.
    ///
    /// [`decrypt_channel_manager_and_fix_errors`]: Self::decrypt_channel_manager_and_fix_errors
    async fn compare_channel_monitors_and_fix_errors(
        &self,
        gvfs: &GoogleVfs,
        mut google_files: Vec<VfsFile>,
        mut lexe_files: Vec<VfsFile>,
    ) -> anyhow::Result<Vec<VfsFile>> {
        // Build a hashmap (&file_id -> &file) for both
        let google_map = google_files
            .iter()
            .map(|file| (&file.id, file))
            .collect::<HashMap<&VfsFileId, &VfsFile>>();
        let lexe_map = lexe_files
            .iter()
            .map(|file| (&file.id, file))
            .collect::<HashMap<&VfsFileId, &VfsFile>>();

        // Early exit if Google and Lexe returned the same (encrypted) data.
        if google_map == lexe_map {
            return Ok(google_files);
        }
        // Uh oh! There was a discrepancy between Google's and Lexe's versions.
        // Since GDrive has rollback protection it is the primary source
        // of truth; fix any discrepancies by updating Lexe's version.

        // A hashset of all `&VfsFileId`s.
        let all_file_ids = google_map
            .keys()
            .chain(lexe_map.keys())
            .copied()
            .collect::<HashSet<&VfsFileId>>();

        // Iterate through all known file ids and fix any discrepancies.
        let mut to_append_to_google_files = Vec::<VfsFile>::new();
        let mut to_append_to_lexe_files = Vec::<VfsFile>::new();
        for file_id in all_file_ids {
            match (google_map.get(&file_id), lexe_map.get(&file_id)) {
                (Some(google_file), Some(lexe_file)) => {
                    // Both Google and Lexe contained the file.
                    if google_file != lexe_file {
                        warn!("Found a monitor discrepancy: {file_id}");
                        // Update Lexe's version.
                        let token = self.get_token().await?;
                        self.backend_api
                            .upsert_file(google_file, token)
                            .await
                            .with_context(|| format!("{file_id}"))
                            .context("Failed to fix Lexe's version")?;
                    }
                }
                (Some(google_file), None) => {
                    // Lexe didn't have the file. Copy it to Lexe.
                    let token = self.get_token().await?;
                    error!("Lexe DB is missing a monitor: {file_id}");
                    self.backend_api
                        .create_file(google_file, token)
                        .await
                        .with_context(|| format!("{file_id}"))
                        .context("Failed to fix Lexe's version")?;
                    to_append_to_lexe_files.push((*google_file).clone());
                }
                (None, Some(lexe_file)) => {
                    // Google didn't have the file; restore from Lexe's version.
                    // This case could be triggered by a deletion race where
                    // deleting from Google succeeds but the node crashes before
                    // it could be deleted from Lexe's DB. Since this is rare,
                    // we should restore the monitor from Lexe's version in case
                    // the deletion from Google was done on accident.
                    // NOTE: if a malicious Lexe is intentionally trying to
                    // resurface a previously-deleted monitor, that 'attack'
                    // doesn't actually accomplish anything.
                    error!("Restoring monitor from Lexe: {file_id}");
                    gvfs.create_file((*lexe_file).clone())
                        .await
                        .with_context(|| format!("{file_id}"))
                        .context("Failed to restore to Google")?;
                    to_append_to_google_files.push((*lexe_file).clone());
                }
                (None, None) => unreachable!("HashSet was wrong"),
            }
        }

        google_files.append(&mut to_append_to_google_files);
        lexe_files.append(&mut to_append_to_lexe_files);

        // All discrepancies have been fixed. Return the files.
        Ok(google_files)
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
            self.backend_api.clone(),
            self.authenticator.clone(),
            self.google_vfs.clone(),
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
            CHANNEL_MONITORS_DIRECTORY,
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
        let api_call_fut = upsert_to_gdrive_and_lexe(
            self.backend_api.clone(),
            self.authenticator.clone(),
            self.google_vfs.clone(),
            file,
        )
        .map_err(|e| e.context("Failed to persist new channel monitor"))
        .apply(Box::pin);

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
            CHANNEL_MONITORS_DIRECTORY,
            funding_txo.to_string(),
            monitor,
        );

        // Generate a future for making a few attempts to persist the channel
        // monitor. It will be executed by the channel monitor persistence task.
        let api_call_fut = upsert_to_gdrive_and_lexe(
            self.backend_api.clone(),
            self.authenticator.clone(),
            self.google_vfs.clone(),
            file,
        )
        .map_err(|e| e.context("Failed to persist updated channel monitor"))
        .apply(Box::pin);

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

/// Helper to upsert an important VFS file to both Google Drive and Lexe's DB.
///
/// - The upsert to GDrive is skipped if `maybe_google_vfs` is [`None`].
/// - Up to [`IMPORTANT_PERSIST_RETRIES`] additional attempts will be made if
///   the first attempt fails.
async fn upsert_to_gdrive_and_lexe(
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    authenticator: Arc<BearerAuthenticator>,
    maybe_google_vfs: Option<Arc<GoogleVfs>>,
    file: VfsFile,
) -> anyhow::Result<()> {
    let do_google_upsert = async {
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
    let do_lexe_upsert = async {
        let token = authenticator
            .get_token(backend_api.as_ref(), SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .upsert_file_with_retries(&file, token, IMPORTANT_PERSIST_RETRIES)
            .await
            .map(|_| ())
            .context("Failed to upsert to Lexe DB")
    };

    let (try_google_upsert, try_lexe_upsert) =
        tokio::join!(do_google_upsert, do_lexe_upsert,);
    try_google_upsert?;
    try_lexe_upsert?;

    Ok(())
}
