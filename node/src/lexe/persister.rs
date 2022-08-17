use std::io::Cursor;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, ensure, Context};
use bitcoin::hash_types::BlockHash;
use bitcoin::secp256k1::PublicKey;
use common::api::vfs::{Directory, File, FileId};
use common::enclave::Measurement;
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

use crate::lexe::channel_manager::{LxChannelMonitorUpdate, USER_CONFIG};
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::peer_manager::ChannelPeer;
use crate::lexe::types::LxOutPoint;
use crate::types::{
    ApiClientType, BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorType, FeeEstimatorType, LoggerType, NetworkGraphType,
    ProbabilisticScorerType, SignerType,
};

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
pub const SINGLETON_DIRECTORY: &str = ".";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const SCORER_FILENAME: &str = "scorer";

// Non-singleton objects use a fixed directory with dynamic filenames
pub const CHANNEL_PEERS_DIRECTORY: &str = "channel_peers";
pub const CHANNEL_MONITORS_DIRECTORY: &str = "channel_monitors";

/// The default number of retries for important persisted state
const DEFAULT_RETRIES: usize = 3;

/// An Arc is held internally, so it is fine to clone and use directly.
#[derive(Clone)] // TODO Try removing this
pub struct LexePersister {
    inner: InnerPersister,
}

impl LexePersister {
    pub fn new(
        api: ApiClientType,
        node_pk: PublicKey,
        measurement: Measurement,
        channel_monitor_updated_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    ) -> Self {
        let inner = InnerPersister::new(
            api,
            node_pk,
            measurement,
            channel_monitor_updated_tx,
        );
        Self { inner }
    }
}

impl Deref for LexePersister {
    type Target = InnerPersister;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// The thing that actually impls the Persist trait. LDK requires that
/// LexePersister Derefs to it.
#[derive(Clone)]
pub struct InnerPersister {
    api: ApiClientType,
    node_pk: PublicKey,
    measurement: Measurement,
    channel_monitor_updated_tx: mpsc::Sender<LxChannelMonitorUpdate>,
}

impl InnerPersister {
    fn new(
        api: ApiClientType,
        node_pk: PublicKey,
        measurement: Measurement,
        channel_monitor_updated_tx: mpsc::Sender<LxChannelMonitorUpdate>,
    ) -> Self {
        Self {
            api,
            node_pk,
            measurement,
            channel_monitor_updated_tx,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn read_channel_manager(
        &self,
        channel_monitors: &mut [(BlockHash, ChannelMonitorType)],
        keys_manager: LexeKeysManager,
        fee_estimator: Arc<FeeEstimatorType>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BroadcasterType>,
        logger: LexeTracingLogger,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        debug!("Reading channel manager");
        let cm_file_id = FileId::new(
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
    pub async fn read_channel_monitors(
        &self,
        keys_manager: LexeKeysManager,
    ) -> anyhow::Result<Vec<(BlockHash, ChannelMonitorType)>> {
        debug!("Reading channel monitors");
        // TODO Also attempt to read from the cloud

        let cm_dir = Directory {
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

    pub async fn read_probabilistic_scorer(
        &self,
        graph: Arc<NetworkGraphType>,
        logger: LoggerType,
    ) -> anyhow::Result<ProbabilisticScorerType> {
        debug!("Reading probabilistic scorer");
        let params = ProbabilisticScoringParameters::default();

        let scorer_file_id = FileId::new(
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

    pub async fn read_network_graph(
        &self,
        genesis_hash: BlockHash,
        logger: LoggerType,
    ) -> anyhow::Result<NetworkGraphType> {
        debug!("Reading network graph");
        let ng_file_id = FileId::new(
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

    pub async fn read_channel_peers(&self) -> anyhow::Result<Vec<ChannelPeer>> {
        debug!("Reading channel peers");
        let cp_dir = Directory {
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

    pub async fn persist_channel_peer(
        &self,
        channel_peer: ChannelPeer,
    ) -> anyhow::Result<()> {
        debug!("Persisting new channel peer");
        let pk_at_addr = channel_peer.to_string();

        let cp_file = File::new(
            self.node_pk,
            self.measurement,
            CHANNEL_PEERS_DIRECTORY.to_owned(),
            pk_at_addr,
            // There is no 'data' associated with a channel peer
            Vec::new(),
        );

        // Retry up to 3 times
        self.api
            .create_file_with_retries(&cp_file, DEFAULT_RETRIES)
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }

    pub async fn persist_manager(
        &self,
        channel_manager: &ChannelManagerType,
    ) -> anyhow::Result<()> {
        debug!("Persisting channel manager");

        // FIXME(encrypt): Encrypt under key derived from seed
        let data = channel_manager.encode();

        let cm_file = File::new(
            self.node_pk,
            self.measurement,
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
            data,
        );

        // Channel manager is more important so let's retry up to three times
        self.api
            .upsert_file_with_retries(&cm_file, DEFAULT_RETRIES)
            .await
            .map(|_| ())
            .context("Could not persist channel manager")
    }

    pub async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()> {
        debug!("Persisting network graph");
        // FIXME(encrypt): Encrypt under key derived from seed
        let data = network_graph.encode();

        let file = File::new(
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

    pub async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()> {
        debug!("Persisting probabilistic scorer");

        let scorer_file = {
            let scorer = scorer_mutex.lock().unwrap();

            // FIXME(encrypt): Encrypt under key derived from seed
            let data = scorer.encode();

            File::new(
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

        let cm_file = File::new(
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
        let api_clone = self.api.clone();
        let channel_monitor_updated_tx =
            self.channel_monitor_updated_tx.clone();
        tokio::spawn(async move {
            // Retry indefinitely until it succeeds
            // TODO Also attempt to persist to cloud backup
            api_clone
                .create_file_with_retries(&cm_file, usize::MAX)
                .await
                .expect("Unlimited retries always return Ok");

            if let Err(e) = channel_monitor_updated_tx.try_send(update) {
                error!("Couldn't notify chain monitor: {:#}", e);
            }
        });

        // As documented in the `Persist` trait docs, return `TemporaryFailure`,
        // which freezes the channel until persistence succeeds.
        Err(ChannelMonitorUpdateErr::TemporaryFailure)
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        // TODO: We probably want to use this for rollback protection
        _update: &Option<ChannelMonitorUpdate>,
        monitor: &ChannelMonitorType,
        update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let funding_txo = LxOutPoint::from(funding_txo);
        debug!("Updating persisted channel {}", funding_txo);

        // FIXME(encrypt): Encrypt under key derived from seed
        let data = monitor.encode();

        let cm_file = File::new(
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
        let api_clone = self.api.clone();
        let channel_monitor_updated_tx =
            self.channel_monitor_updated_tx.clone();
        tokio::spawn(async move {
            // Retry indefinitely until it succeeds
            // TODO Also attempt to persist to cloud backup
            api_clone
                .upsert_file_with_retries(&cm_file, usize::MAX)
                .await
                .expect("Unlimited retries always return Ok");

            if let Err(e) = channel_monitor_updated_tx.try_send(update) {
                error!("Couldn't notify chain monitor: {:#}", e);
            }
        });

        // As documented in the `Persist` trait docs, return `TemporaryFailure`,
        // which freezes the channel until persistence succeeds.
        Err(ChannelMonitorUpdateErr::TemporaryFailure)
    }
}
