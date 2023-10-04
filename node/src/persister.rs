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
    api::{
        auth::{BearerAuthToken, BearerAuthenticator},
        error::{NodeApiError, NodeErrorKind},
        provision::GDriveCredentials,
        qs::{GetNewPayments, GetPaymentByIndex, GetPaymentsByIds},
        vfs::{VfsDirectory, VfsFile, VfsFileId},
        Scid, User,
    },
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
    vfs_encrypt::VfsMasterKey,
};
use gdrive::GvfsRoot;
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
use tracing::{debug, error, info};

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
    vfs_master_key: Arc<VfsMasterKey>,
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
    vfs_master_key: &VfsMasterKey,
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
    vfs_master_key: &VfsMasterKey,
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
    vfs_master_key: &VfsMasterKey,
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
    vfs_master_key: &VfsMasterKey,
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

impl NodePersister {
    pub(crate) fn new(
        backend_api: Arc<dyn BackendApiClient + Send + Sync>,
        authenticator: Arc<BearerAuthenticator>,
        vfs_master_key: Arc<VfsMasterKey>,
        user: User,
        shutdown: ShutdownChannel,
        channel_monitor_persister_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    ) -> Self {
        Self {
            backend_api,
            authenticator,
            vfs_master_key,
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

        let maybe_file = self
            .backend_api
            .get_file(&file_id, token)
            .await
            .context("Could not fetch channel manager from DB")?;

        let maybe_manager = match maybe_file {
            Some(file) => {
                let data = persister::decrypt_file(
                    &self.vfs_master_key,
                    &file_id,
                    file,
                )?;
                let mut state_buf = Cursor::new(&data);

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
        // TODO Also attempt to read from the cloud

        let dir = VfsDirectory {
            dirname: CHANNEL_MONITORS_DIRECTORY.to_owned(),
        };
        let token = self.get_token().await?;

        let all_files = self
            .backend_api
            .get_directory(&dir, token)
            .await
            .context("Could not fetch channel monitors from DB")?;

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
        let token = self.get_token().await?;

        let file = self.encrypt_ldk_writeable(
            SINGLETON_DIRECTORY,
            CHANNEL_MANAGER_FILENAME,
            channel_manager,
        );

        // Channel manager is more important so let's retry a few times
        self.backend_api
            .upsert_file_with_retries(&file, token, IMPORTANT_PERSIST_RETRIES)
            .await
            .map(|_| ())
            .context("Could not persist channel manager")
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
        let backend_api = self.backend_api.clone();
        let authenticator = self.authenticator.clone();
        let api_call_fut = Box::pin(async move {
            // TODO(max): Also attempt to persist to cloud backup
            let token = authenticator
                .get_token(backend_api.as_ref(), SystemTime::now())
                .await
                .context("Could not get token")?;
            // We upsert (instead of create) due to an occasional race where a
            // channel monitor persist succeeds and the channel is resumed but
            // the node shuts down before the channel manager is repersisted.
            backend_api
                .upsert_file_with_retries(
                    &file,
                    token,
                    IMPORTANT_PERSIST_RETRIES,
                )
                .await
                .map(|_| ())
                .context("Couldn't persist updated channel monitor")
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
            CHANNEL_MONITORS_DIRECTORY,
            funding_txo.to_string(),
            monitor,
        );

        // Generate a future for making a few attempts to persist the channel
        // monitor. It will be executed by the channel monitor persistence task.
        let backend_api = self.backend_api.clone();
        let authenticator = self.authenticator.clone();
        let api_call_fut = Box::pin(async move {
            // TODO(max): Also attempt to persist to cloud backup
            let token = authenticator
                .get_token(backend_api.as_ref(), SystemTime::now())
                .await
                .context("Could not get token")?;
            backend_api
                .upsert_file_with_retries(
                    &file,
                    token,
                    IMPORTANT_PERSIST_RETRIES,
                )
                .await
                .map(|_| ())
                .context("Couldn't persist updated channel monitor")
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
