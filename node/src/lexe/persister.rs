use std::io::{self, Cursor, ErrorKind};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, ensure, Context};
use bitcoin::hash_types::BlockHash;
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::ChannelMonitorUpdate;
use lightning::chain::transaction::OutPoint;
use lightning::chain::ChannelMonitorUpdateErr;
use lightning::ln::channelmanager::ChannelManagerReadArgs;
use lightning::routing::gossip::NetworkGraph as LdkNetworkGraph;
use lightning::routing::scoring::{
    ProbabilisticScorer, ProbabilisticScoringParameters,
};
use lightning::util::persist::Persister;
use lightning::util::ser::{ReadableArgs, Writeable};
use once_cell::sync::{Lazy, OnceCell};
use tokio::runtime::{Builder, Handle, Runtime};
use tracing::{debug, error};

use crate::api::{DirectoryId, File, FileId};
use crate::lexe::bitcoind::LexeBitcoind;
use crate::lexe::channel_manager::USER_CONFIG;
use crate::lexe::keys_manager::LexeKeysManager;
use crate::lexe::logger::LexeTracingLogger;
use crate::lexe::peer_manager::ChannelPeer;
use crate::lexe::types::LxOutPoint;
use crate::types::{
    ApiClientType, BroadcasterType, ChainMonitorType, ChannelManagerType,
    ChannelMonitorType, FeeEstimatorType, InstanceId, LoggerType,
    NetworkGraphType, ProbabilisticScorerType, SignerType,
};

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
pub const SINGLETON_DIRECTORY: &str = ".";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const SCORER_FILENAME: &str = "scorer";

// Non-singleton objects use a fixed directory with dynamic filenames
pub const CHANNEL_PEERS_DIRECTORY: &str = "channel_peers";
pub const CHANNEL_MONITORS_DIRECTORY: &str = "channel_monitors";

/// An Arc is held internally, so it is fine to clone and use directly.
#[derive(Clone)] // TODO Try removing this
pub struct LexePersister {
    inner: InnerPersister,
}

impl LexePersister {
    pub fn new(api: ApiClientType, instance_id: InstanceId) -> Self {
        let inner = InnerPersister::new(api, instance_id);
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
    instance_id: InstanceId,
}

impl InnerPersister {
    fn new(api: ApiClientType, instance_id: InstanceId) -> Self {
        Self { api, instance_id }
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
        let cm_file_id = FileId {
            instance_id: self.instance_id.clone(),
            directory: SINGLETON_DIRECTORY.to_owned(),
            name: CHANNEL_MANAGER_FILENAME.to_owned(),
        };
        let cm_file_opt = self
            .api
            .get_file(cm_file_id)
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

        let cm_dir_id = DirectoryId {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_MONITORS_DIRECTORY.to_owned(),
        };

        let cm_file_vec = self
            .api
            .get_directory(cm_dir_id)
            .await
            .context("Could not fetch channel monitors from DB")?;

        let mut result = Vec::new();

        for cm_file in cm_file_vec {
            let given = LxOutPoint::from_str(&cm_file.name)
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
        let scorer_file_id = FileId {
            instance_id: self.instance_id.clone(),
            directory: SINGLETON_DIRECTORY.to_owned(),
            name: SCORER_FILENAME.to_owned(),
        };
        let scorer_file_opt = self
            .api
            .get_file(scorer_file_id)
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
        let ng_file_id = FileId {
            instance_id: self.instance_id.clone(),
            directory: SINGLETON_DIRECTORY.to_owned(),
            name: NETWORK_GRAPH_FILENAME.to_owned(),
        };
        let ng_file_opt = self
            .api
            .get_file(ng_file_id)
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
        let cp_dir_id = DirectoryId {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_PEERS_DIRECTORY.to_owned(),
        };

        let cp_file_vec = self
            .api
            .get_directory(cp_dir_id)
            .await
            .context("Could not fetch channel peers from DB")?;

        let mut result = Vec::with_capacity(cp_file_vec.len());

        for cp_file in cp_file_vec {
            // <pk>@<addr>
            let pk_at_addr = cp_file.name;

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

        let cp_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_PEERS_DIRECTORY.to_owned(),
            name: pk_at_addr,
            // There is no 'data' associated with a channel peer
            data: Vec::new(),
        };

        self.api
            .create_file(cp_file)
            .await
            .map(|_| ())
            .map_err(|e| e.into())
    }
}

/// A Tokio runtime which can be used to run async closures in sync fns
/// downstream of thread::spawn()
static PERSISTER_RUNTIME: Lazy<OnceCell<Runtime>> = Lazy::new(|| {
    Builder::new_current_thread()
        .enable_io()
        // Because our reqwest::Client has a configured timeout
        .enable_time()
        .build()
        .unwrap()
        .into()
});

/// This trait is defined in lightning::util::Persist.
///
/// The methods in this trait are called inside a `thread::spawn()` within
/// `BackgroundProcessor::start()`, meaning that the thread-local context for
/// these function do not contain a Tokio (async) runtime. Thus, we offer a
/// lazily-initialized `PERSISTER_RUNTIME` above which the `Persister` methods
/// use to run async closures inside their synchronous functions.
impl<'a>
    Persister<
        'a,
        SignerType,
        Arc<ChainMonitorType>,
        Arc<LexeBitcoind>,
        LexeKeysManager,
        Arc<LexeBitcoind>,
        LexeTracingLogger,
        Mutex<ProbabilisticScorerType>,
    > for InnerPersister
{
    fn persist_manager(
        &self,
        channel_manager: &ChannelManagerType,
    ) -> Result<(), io::Error> {
        debug!("Persisting channel manager");
        let cm_file = File {
            instance_id: self.instance_id.clone(),
            directory: SINGLETON_DIRECTORY.to_owned(),
            name: CHANNEL_MANAGER_FILENAME.to_owned(),
            // FIXME(encrypt): Encrypt under key derived from seed
            data: channel_manager.encode(),
        };

        // Run an async fn inside a sync fn downstream of thread::spawn()
        PERSISTER_RUNTIME
            .get()
            .unwrap()
            .block_on(async move { self.api.upsert_file(cm_file).await })
            .map(|_| ())
            .map_err(|api_err| {
                error!("Could not persist channel manager: {:#}", api_err);
                io::Error::new(ErrorKind::Other, api_err)
            })
    }

    fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> Result<(), io::Error> {
        debug!("Persisting network graph");
        let file = File {
            instance_id: self.instance_id.clone(),
            directory: SINGLETON_DIRECTORY.to_owned(),
            name: NETWORK_GRAPH_FILENAME.to_owned(),
            // FIXME(encrypt): Encrypt under key derived from seed
            data: network_graph.encode(),
        };

        // Run an async fn inside a sync fn downstream of thread::spawn()
        PERSISTER_RUNTIME
            .get()
            .unwrap()
            .block_on(async move { self.api.upsert_file(file).await })
            .map(|_| ())
            .map_err(|api_err| {
                error!("Could not persist network graph: {:#}", api_err);
                io::Error::new(ErrorKind::Other, api_err)
            })
    }

    fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> Result<(), io::Error> {
        debug!("Persisting probabilistic scorer");

        let scorer_file = {
            let scorer = scorer_mutex.lock().unwrap();

            File {
                instance_id: self.instance_id.clone(),
                directory: SINGLETON_DIRECTORY.to_owned(),
                name: SCORER_FILENAME.to_owned(),
                data: scorer.encode(),
            }
        };

        PERSISTER_RUNTIME.get().unwrap().block_on(async move {
            self.api
                .upsert_file(scorer_file)
                .await
                .map(|_| ())
                .map_err(|api_err| io::Error::new(ErrorKind::Other, api_err))
        })
    }
}

impl Persist<SignerType> for InnerPersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &ChannelMonitorType,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let outpoint = LxOutPoint::from(funding_txo);
        let outpoint_str = outpoint.to_string();
        debug!("Persisting new channel {}", outpoint_str);

        let cm_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_MONITORS_DIRECTORY.to_owned(),
            name: outpoint_str,
            // FIXME(encrypt): Encrypt under key derived from seed
            data: monitor.encode(),
        };

        // Run an async fn inside a sync fn inside a Tokio runtime
        tokio::task::block_in_place(|| {
            Handle::current()
                .block_on(async move { self.api.create_file(cm_file).await })
        })
        .map(|_| ())
        .map_err(|e| {
            // Even though this is a temporary failure that can be retried,
            // we should still log it
            error!("Could not persist new channel monitor: {:#}", e);
            ChannelMonitorUpdateErr::TemporaryFailure
        })
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        _update: &Option<ChannelMonitorUpdate>,
        monitor: &ChannelMonitorType,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let outpoint = LxOutPoint::from(funding_txo);
        let outpoint_str = outpoint.to_string();
        debug!("Updating persisted channel {}", outpoint_str);

        let cm_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_MONITORS_DIRECTORY.to_owned(),
            name: outpoint_str,
            // FIXME(encrypt): Encrypt under key derived from seed
            data: monitor.encode(),
        };

        // Run an async fn inside a sync fn inside a Tokio runtime
        tokio::task::block_in_place(|| {
            Handle::current()
                .block_on(async move { self.api.upsert_file(cm_file).await })
        })
        .map(|_| ())
        .map_err(|e| {
            // Even though this is a temporary failure that can be retried,
            // we should still log it
            error!("Could not update persisted channel monitor: {:#}", e);
            ChannelMonitorUpdateErr::TemporaryFailure
        })
    }
}
