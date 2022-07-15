use std::io::{self, Cursor, ErrorKind};
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, ensure, Context};
use bitcoin::hash_types::BlockHash;
use bitcoin::secp256k1::PublicKey;
use lightning::chain::chainmonitor::{MonitorUpdateId, Persist};
use lightning::chain::channelmonitor::{
    ChannelMonitor as LdkChannelMonitor, ChannelMonitorUpdate,
};
use lightning::chain::keysinterface::{
    InMemorySigner, KeysInterface, KeysManager, Sign,
};
use lightning::chain::transaction::OutPoint;
use lightning::chain::ChannelMonitorUpdateErr;
use lightning::ln::channelmanager::{
    ChannelManagerReadArgs, SimpleArcChannelManager,
};
use lightning::routing::gossip::NetworkGraph as LdkNetworkGraph;
use lightning::routing::scoring::{
    ProbabilisticScorer, ProbabilisticScoringParameters,
};
use lightning::util::config::UserConfig;
use lightning::util::persist::Persister;
use lightning::util::ser::{ReadableArgs, Writeable};
use once_cell::sync::{Lazy, OnceCell};
use tokio::runtime::{Builder, Handle, Runtime};

use crate::api::{ApiClient, DirectoryId, File, FileId};
use crate::bitcoind_client::BitcoindClient;
use crate::convert;
use crate::logger::LdkTracingLogger;
use crate::types::{
    ChainMonitorType, ChannelManagerType, InstanceId, LoggerType,
    NetworkGraphType, ProbabilisticScorerType,
};

// Singleton objects use SINGLETON_DIRECTORY with a fixed filename
const SINGLETON_DIRECTORY: &str = ".";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const SCORER_FILENAME: &str = "scorer";

// Non-singleton objects use a fixed directory with dynamic filenames
const CHANNEL_PEERS_DIRECTORY: &str = "channel_peers";
const CHANNEL_MONITORS_DIRECTORY: &str = "channel_monitors";

#[derive(Clone)]
pub struct LexePersister {
    api: ApiClient,
    instance_id: InstanceId,
}

impl LexePersister {
    pub fn new(api: ApiClient, instance_id: InstanceId) -> Self {
        Self { api, instance_id }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn read_channel_manager(
        &self,
        channel_monitors: &mut [(
            BlockHash,
            LdkChannelMonitor<InMemorySigner>,
        )],
        keys_manager: Arc<KeysManager>,
        fee_estimator: Arc<BitcoindClient>,
        chain_monitor: Arc<ChainMonitorType>,
        broadcaster: Arc<BitcoindClient>,
        logger: Arc<LdkTracingLogger>,
        user_config: UserConfig,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        println!("Reading channel manager");
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
                    user_config,
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
    pub async fn read_channel_monitors<Signer: Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> anyhow::Result<Vec<(BlockHash, LdkChannelMonitor<Signer>)>>
    where
        K::Target: KeysInterface<Signer = Signer> + Sized,
    {
        println!("Reading channel monitors");

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
            // <txid>_<txindex>
            let id = cm_file.name;
            let (txid, index) = convert::txid_and_index_from_string(id)
                .context("Invalid channel id")?;

            let mut state_buf = Cursor::new(&cm_file.data);

            let (blockhash, channel_monitor) =
                <(BlockHash, LdkChannelMonitor<Signer>)>::read(
                    &mut state_buf,
                    &*keys_manager,
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize Channel Monitor")?;

            let (output, _script) = channel_monitor.get_funding_txo();
            ensure!(output.txid == txid, "Deserialized txid don' match");
            ensure!(output.index == index, "Deserialized index don' match");

            result.push((blockhash, channel_monitor));
        }

        Ok(result)
    }

    pub async fn read_probabilistic_scorer(
        &self,
        graph: Arc<NetworkGraphType>,
        logger: LoggerType,
    ) -> anyhow::Result<ProbabilisticScorerType> {
        println!("Reading probabilistic scorer");
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
        println!("Reading network graph");
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

    pub async fn read_channel_peers(
        &self,
    ) -> anyhow::Result<Vec<(PublicKey, SocketAddr)>> {
        println!("Reading channel peers");
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
            // <pubkey>@<addr>
            let pubkey_at_addr = cp_file.name;

            let (peer_pubkey, peer_addr) =
                convert::peer_pubkey_addr_from_string(pubkey_at_addr)
                    .context("Invalid peer <pubkey>@<addr>")?;

            result.push((peer_pubkey, peer_addr));
        }

        Ok(result)
    }

    #[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
    pub async fn persist_channel_peer(
        &self,
        peer_pubkey: PublicKey,
        peer_address: SocketAddr,
    ) -> anyhow::Result<()> {
        println!("Persisting new channel peer");
        let pubkey_at_addr =
            convert::peer_pubkey_addr_to_string(peer_pubkey, peer_address);

        let cp_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_PEERS_DIRECTORY.to_owned(),
            name: pubkey_at_addr.to_owned(),
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
        InMemorySigner,
        Arc<ChainMonitorType>,
        Arc<BitcoindClient>,
        Arc<KeysManager>,
        Arc<BitcoindClient>,
        Arc<LdkTracingLogger>,
        Mutex<ProbabilisticScorerType>,
    > for LexePersister
{
    fn persist_manager(
        &self,
        channel_manager: &SimpleArcChannelManager<
            ChainMonitorType,
            BitcoindClient,
            BitcoindClient,
            LdkTracingLogger,
        >,
    ) -> Result<(), io::Error> {
        println!("Persisting channel manager");
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
                println!("Could not persist channel manager: {:#}", api_err);
                io::Error::new(ErrorKind::Other, api_err)
            })
    }

    fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> Result<(), io::Error> {
        println!("Persisting network graph");
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
                println!("Could not persist network graph: {:#}", api_err);
                io::Error::new(ErrorKind::Other, api_err)
            })
    }

    fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> Result<(), io::Error> {
        println!("Persisting probabilistic scorer");

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

impl<ChannelSigner: Sign> Persist<ChannelSigner> for LexePersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &LdkChannelMonitor<ChannelSigner>,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let id = convert::txid_and_index_to_string(
            funding_txo.txid,
            funding_txo.index,
        );
        println!("Persisting new channel {}", id);

        let cm_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_MONITORS_DIRECTORY.to_owned(),
            name: id,
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
            println!("Could not persist new channel monitor: {:#}", e);
            ChannelMonitorUpdateErr::TemporaryFailure
        })
    }

    fn update_persisted_channel(
        &self,
        funding_txo: OutPoint,
        _update: &Option<ChannelMonitorUpdate>,
        monitor: &LdkChannelMonitor<ChannelSigner>,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let id = convert::txid_and_index_to_string(
            funding_txo.txid,
            funding_txo.index,
        );
        println!("Updating persisted channel {}", id);

        let cm_file = File {
            instance_id: self.instance_id.clone(),
            directory: CHANNEL_MONITORS_DIRECTORY.to_owned(),
            name: id,
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
            println!("Could not update persisted channel monitor: {:#}", e);
            ChannelMonitorUpdateErr::TemporaryFailure
        })
    }
}
