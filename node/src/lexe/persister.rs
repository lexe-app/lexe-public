use std::io::Cursor;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, ensure, Context};
use bitcoin::hash_types::BlockHash;
use bitcoin::secp256k1::PublicKey;
use common::api::vfs::{NodeDirectory, NodeFile, NodeFileId};
use common::enclave::Measurement;
use common::ln::channel::LxOutPoint;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lexe_ln::alias::{
    BroadcasterType, ChannelMonitorType, FeeEstimatorType, SignerType,
};
use lexe_ln::channel_monitor::LxChannelMonitorUpdate;
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::logger::LexeTracingLogger;
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::ChannelMonitorUpdate;
use lightning::chain::transaction::OutPoint;
use lightning::chain::ChannelMonitorUpdateErr;
use lightning::ln::channelmanager::ChannelManagerReadArgs;
use lightning::routing::gossip::NetworkGraph as LdkNetworkGraph;
use lightning::routing::scoring::{
    ProbabilisticScorer, ProbabilisticScoringParameters,
};
use lightning::util::ser::{ReadableArgs, Writeable};
use tokio::sync::mpsc;
use tracing::{debug, error};

use crate::lexe::channel_manager::USER_CONFIG;
use crate::lexe::peer_manager::ChannelPeer;
use crate::types::{
    ApiClientType, ChainMonitorType, ChannelManagerType, NetworkGraphType,
    ProbabilisticScorerType,
};

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
pub(crate) const SINGLETON_DIRECTORY: &str = ".";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const SCORER_FILENAME: &str = "scorer";

// Non-singleton objects use a fixed directory with dynamic filenames
pub(crate) const CHANNEL_PEERS_DIRECTORY: &str = "channel_peers";
pub(crate) const CHANNEL_MONITORS_DIRECTORY: &str = "channel_monitors";

/// The default number of persist retries for important objects
const IMPORTANT_RETRIES: usize = 3;

/// An Arc is held internally, so it is fine to clone and use directly.
#[derive(Clone)] // TODO Try removing this
pub(crate) struct NodePersister {
    inner: InnerPersister,
}

impl NodePersister {
    pub(crate) fn new(
        api: ApiClientType,
        node_pk: PublicKey,
        measurement: Measurement,
        shutdown: ShutdownChannel,
        channel_monitor_updated_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    ) -> Self {
        let inner = InnerPersister {
            api,
            node_pk,
            measurement,
            shutdown,
            channel_monitor_updated_tx,
        };

        Self { inner }
    }
}

impl Deref for NodePersister {
    type Target = InnerPersister;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// The thing that actually impls the Persist trait. LDK requires that
/// NodePersister Derefs to it.
#[derive(Clone)]
pub(crate) struct InnerPersister {
    api: ApiClientType,
    node_pk: PublicKey,
    measurement: Measurement,
    shutdown: ShutdownChannel,
    channel_monitor_updated_tx: mpsc::Sender<LxChannelMonitorUpdate>,
}

impl InnerPersister {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn read_channel_manager(
        &self,
        channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
        keys_manager: LexeKeysManager,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        debug!("Reading channel manager");
        let cm_file_id = NodeFileId::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
        );
        let cm_file_opt = self
            .api
            .get_file(&cm_file_id)
            .await
            .context("Could not fetch channel manager from DB")?;

        let cm_opt = match cm_file_opt {
            Some(cm_file) => {
                let mut channel_monitor_mut_refs = Vec::new();
                for (_, channel_monitor) in channel_monitors.iter_mut() {
                    channel_monitor_mut_refs.push(channel_monitor);
                }
                let read_args = ChannelManagerReadArgs::new(
                    keys_manager,
                    fee_estimator,
                    chain_monitor,
                    broadcaster,
                    logger,
                    USER_CONFIG,
                    channel_monitor_mut_refs,
                );

                let mut state_buf = Cursor::new(&cm_file.data);

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

        Ok(cm_opt)
    }

    // Replaces equivalent method in lightning_persister::FilesystemPersister
    pub(crate) async fn read_channel_monitors(
        &self,
        keys_manager: LexeKeysManager,
    ) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
        debug!("Reading channel monitors");
        // TODO Also attempt to read from the cloud

        let cm_dir = NodeDirectory {
            node_pk: self.node_pk,
            measurement: self.measurement,
            dirname: CHANNEL_MONITORS_DIRECTORY.to_owned(),
        };

        let cm_file_vec = self
            .api
            .get_directory(&cm_dir)
            .await
            .context("Could not fetch channel monitors from DB")?;

        let mut result = Vec::new();

        for cm_file in cm_file_vec {
            let given = LxOutPoint::from_str(&cm_file.id.filename)
                .context("Invalid funding txo string")?;

            let mut state_buf = Cursor::new(&cm_file.data);

            let (blockhash, channel_monitor) =
                <(BlockHash, ChannelMonitorType)>::read(
                    &mut state_buf,
                    &*keys_manager,
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize Channel Monitor")?;

            let (derived, _script) = channel_monitor.get_funding_txo();
            ensure!(derived.txid == given.txid, "outpoint txid don' match");
            ensure!(derived.index == given.index, "outpoint index don' match");

            result.push((blockhash, channel_monitor));
        }

        Ok(result)
    }

    pub(crate) async fn read_probabilistic_scorer(
        &self,
        graph: Arc<NetworkGraphType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<ProbabilisticScorerType> {
        debug!("Reading probabilistic scorer");
        let params = ProbabilisticScoringParameters::default();

        let scorer_file_id = NodeFileId::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            SCORER_FILENAME.to_owned(),
        );
        let scorer_file_opt = self
            .api
            .get_file(&scorer_file_id)
            .await
            .context("Could not fetch probabilistic scorer from DB")?;

        let scorer = match scorer_file_opt {
            Some(scorer_file) => {
                let mut state_buf = Cursor::new(&scorer_file.data);
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
        genesis_hash: BlockHash,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<NetworkGraphType> {
        debug!("Reading network graph");
        let ng_file_id = NodeFileId::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            NETWORK_GRAPH_FILENAME.to_owned(),
        );
        let ng_file_opt = self
            .api
            .get_file(&ng_file_id)
            .await
            .context("Could not fetch network graph from DB")?;

        let ng = match ng_file_opt {
            Some(ng_file) => {
                let mut state_buf = Cursor::new(&ng_file.data);
                LdkNetworkGraph::read(&mut state_buf, logger.clone())
                    // LDK DecodeError is Debug but doesn't impl
                    // std::error::Error
                    .map_err(|e| anyhow!("{:?}", e))
                    .context("Failed to deserialize NetworkGraph")?
            }
            None => LdkNetworkGraph::new(genesis_hash, logger),
        };

        Ok(ng)
    }

    pub(crate) async fn read_channel_peers(
        &self,
    ) -> anyhow::Result<Vec<ChannelPeer>> {
        debug!("Reading channel peers");
        let cp_dir = NodeDirectory {
            node_pk: self.node_pk,
            measurement: self.measurement,
            dirname: CHANNEL_PEERS_DIRECTORY.to_owned(),
        };

        let cp_file_vec = self
            .api
            .get_directory(&cp_dir)
            .await
            .context("Could not fetch channel peers from DB")?;

        let mut result = Vec::with_capacity(cp_file_vec.len());

        for cp_file in cp_file_vec {
            // <pk>@<addr>
            let pk_at_addr = cp_file.id.filename;

            let channel_peer = ChannelPeer::from_str(&pk_at_addr)
                .context("Could not deserialize channel peer")?;

            result.push(channel_peer);
        }

        Ok(result)
    }

    pub(crate) async fn persist_channel_peer(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()> {
        debug!("Persisting new channel peer");
        let pk_at_addr = channel_peer.to_string();

        let cp_file = NodeFile::new(
            self.node_pk,
            self.measurement,
            CHANNEL_PEERS_DIRECTORY.to_owned(),
            pk_at_addr,
            // There is no 'data' associated with a channel peer
            Vec::new(),
        );

        // Retry up to 3 times
        self.api
            .create_file_with_retries(&cp_file, IMPORTANT_RETRIES)
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }

    pub(crate) async fn persist_manager(
        &self,
        channel_manager: &ChannelManagerType,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");

        // FIXME(encrypt): Encrypt under key derived from seed
        let data = channel_manager.encode();

        let cm_file = NodeFile::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
            data,
        );

        // Channel manager is more important so let's retry up to three times
        self.api
            .upsert_file_with_retries(&cm_file, IMPORTANT_RETRIES)
            .await
            .map(|_| ())
            .context("Could not persist channel manager")
    }

    pub(crate) async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()> {
        debug!("Persisting network graph");
        // FIXME(encrypt): Encrypt under key derived from seed
        let data = network_graph.encode();

        let file = NodeFile::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            NETWORK_GRAPH_FILENAME.to_owned(),
            data,
        );

        self.api
            .upsert_file(&file)
            .await
            .map(|_| ())
            .context("Could not persist network graph")
    }

    pub(crate) async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()> {
        debug!("Persisting probabilistic scorer");

        let scorer_file = {
            let scorer = scorer_mutex.lock().unwrap();

            // FIXME(encrypt): Encrypt under key derived from seed
            let data = scorer.encode();

            NodeFile::new(
                self.node_pk,
                self.measurement,
                SINGLETON_DIRECTORY.to_owned(),
                SCORER_FILENAME.to_owned(),
                data,
            )
        };

        self.api
            .upsert_file(&scorer_file)
            .await
            .map(|_| ())
            .context("Could not persist scorer")
    }
}

impl Persist<SignerType> for InnerPersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitorType,
        update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let funding_txo = LxOutPoint::from(funding_txo);
        debug!("Persisting new channel {}", funding_txo);

        // FIXME(encrypt): Encrypt under key derived from seed
        let data = monitor.encode();

        let cm_file = NodeFile::new(
            self.node_pk,
            self.measurement,
            CHANNEL_MONITORS_DIRECTORY.to_owned(),
            funding_txo.to_string(),
            data,
        );
        let update = LxChannelMonitorUpdate {
            funding_txo,
            update_id,
        };

        // Spawn a task for persisting the channel monitor
        let api = self.api.clone();
        let channel_monitor_updated_tx =
            self.channel_monitor_updated_tx.clone();
        let shutdown = self.shutdown.clone();
        let _ = LxTask::spawn(async move {
            // Retry a few times and shut down if persist fails
            // TODO Also attempt to persist to cloud backup
            let persist_res = api
                .create_file_with_retries(&cm_file, IMPORTANT_RETRIES)
                .await;
            match persist_res {
                Ok(_) => {
                    debug!("Persisting new channel succeeded");
                    // Notify the chain monitor
                    if let Err(e) = channel_monitor_updated_tx.try_send(update)
                    {
                        error!("Couldn't notify chain monitor: {e:#}");
                    }
                }
                Err(e) => {
                    error!("Fatal error: Couldn't persist new channel: {e:#}");
                    shutdown.send();
                }
            }
        });

        // As documented in the `Persist` trait docs, return `TemporaryFailure`,
        // which freezes the channel until persistence succeeds.
        Err(ChannelMonitorUpdateErr::TemporaryFailure)
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        // TODO: We probably want to use the id inside for rollback protection
        _update: &Option<ChannelMonitorUpdate>,
        monitor: &ChannelMonitorType,
        update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let funding_txo = LxOutPoint::from(funding_txo);
        debug!("Updating persisted channel {}", funding_txo);

        // FIXME(encrypt): Encrypt under key derived from seed
        let data = monitor.encode();

        let cm_file = NodeFile::new(
            self.node_pk,
            self.measurement,
            CHANNEL_MONITORS_DIRECTORY.to_owned(),
            funding_txo.to_string(),
            data,
        );
        let update = LxChannelMonitorUpdate {
            funding_txo,
            update_id,
        };

        // Spawn a task for persisting the channel monitor
        let api = self.api.clone();
        let channel_monitor_updated_tx =
            self.channel_monitor_updated_tx.clone();
        let shutdown = self.shutdown.clone();
        let _ = LxTask::spawn(async move {
            // Retry a few times and shut down if persist fails
            // TODO Also attempt to persist to cloud backup
            let persist_res = api
                .upsert_file_with_retries(&cm_file, IMPORTANT_RETRIES)
                .await;
            match persist_res {
                Ok(_) => {
                    debug!("Persisting updated channel succeeded");
                    // Notify the chain monitor
                    if let Err(e) = channel_monitor_updated_tx.try_send(update)
                    {
                        error!("Couldn't notify chain monitor: {e:#}");
                    }
                }
                Err(e) => {
                    error!(
                        "Fatal error: Couldn't persist updated channel: {e:#}"
                    );
                    shutdown.send();
                }
            }
        });

        // As documented in the `Persist` trait docs, return `TemporaryFailure`,
        // which freezes the channel until persistence succeeds.
        Err(ChannelMonitorUpdateErr::TemporaryFailure)
    }
}
