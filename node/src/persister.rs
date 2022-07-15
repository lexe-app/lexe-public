use std::convert::TryInto;
use std::io::{self, Cursor, ErrorKind};
use std::net::SocketAddr;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, ensure, Context};
use bitcoin::hash_types::{BlockHash, Txid};
use bitcoin::hashes::hex::{FromHex, ToHex};
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

use crate::api::{ApiClient, ChannelMonitor, DirectoryId, File, FileId};
use crate::bitcoind_client::BitcoindClient;
use crate::convert;
use crate::logger::LdkTracingLogger;
use crate::types::{
    ChainMonitorType, ChannelManagerType, LoggerType, NetworkGraphType,
    ProbabilisticScorerType,
};

/// The "directory" used when persisting singleton objects
const SINGLETON_DIRECTORY: &str = ".";
const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
const NETWORK_GRAPH_FILENAME: &str = "network_graph";
const SCORER_FILENAME: &str = "scorer";

const CHANNEL_PEERS_DIRECTORY: &str = "channel_peers";

#[derive(Clone)]
pub struct PostgresPersister {
    api: ApiClient,
    instance_id: String,
}

impl PostgresPersister {
    pub fn new(api: ApiClient, pubkey: &PublicKey, measurement: &str) -> Self {
        Self {
            api,
            instance_id: convert::get_instance_id(pubkey, measurement),
        }
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
            .map_err(|e| {
                println!("{:#}", e);
                e
            })
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
        let cm_vec = self
            .api
            .get_channel_monitors(self.instance_id.clone())
            .await
            .map_err(|e| {
                println!("{:#}", e);
                e
            })
            .context("Could not fetch channel monitors from DB")?;

        let mut result = Vec::new();

        for cm in cm_vec {
            let tx_id = Txid::from_hex(cm.tx_id.as_str())
                .context("Invalid tx_id returned from DB")?;
            let tx_index: u16 = cm.tx_index.try_into().unwrap();

            let mut state_buf = Cursor::new(&cm.state);

            let (blockhash, channel_monitor) =
                <(BlockHash, LdkChannelMonitor<Signer>)>::read(
                    &mut state_buf,
                    &*keys_manager,
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize Channel Monitor")?;

            let (output, _script) = channel_monitor.get_funding_txo();
            ensure!(output.txid == tx_id, "Deserialized txid don' match");
            ensure!(output.index == tx_index, "Deserialized index don' match");

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
            .map_err(|e| {
                println!("{:#}", e);
                e
            })
            .context("Could not fetch channel peers from DB")?;

        let mut result = Vec::with_capacity(cp_file_vec.len());

        for cp_file in cp_file_vec {
            // <pubkey>@<addr>
            let pubkey_at_addr = cp_file.name;

            // vec![<pubkey>, <addr>]
            let mut pubkey_and_addr = pubkey_at_addr.split('@');
            let pubkey_str = pubkey_and_addr
                .next()
                .context("Missing <pubkey> in <pubkey>@<addr> peer address")?;
            let addr_str = pubkey_and_addr
                .next()
                .context("Missing <addr> in <pubkey>@<addr> peer address")?;

            let peer_pubkey = PublicKey::from_str(pubkey_str)
                .context("Could not deserialize PublicKey from LowerHex")?;
            let peer_addr = SocketAddr::from_str(addr_str)
                .context("Could not parse socket address from string")?;

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
        let pubkey_str = convert::pubkey_to_hex(&peer_pubkey);
        let addr_str = peer_address.to_string();

        // <pubkey>@<addr>
        let pubkey_at_addr = [pubkey_str, addr_str].join("@");

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
    > for PostgresPersister
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
            .block_on(
                async move { self.api.create_or_update_file(cm_file).await },
            )
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
            .block_on(async move { self.api.create_or_update_file(file).await })
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
                .create_or_update_file(scorer_file)
                .await
                .map(|_| ())
                .map_err(|api_err| io::Error::new(ErrorKind::Other, api_err))
        })
    }
}

impl<ChannelSigner: Sign> Persist<ChannelSigner> for PostgresPersister {
    fn persist_new_channel(
        &self,
        funding_txo: OutPoint,
        monitor: &LdkChannelMonitor<ChannelSigner>,
        _update_id: MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let tx_id = funding_txo.txid.to_hex();
        let tx_index = funding_txo.index.try_into().unwrap();
        println!("Persisting new channel {}_{}", tx_id, tx_index);
        let channel_monitor = ChannelMonitor {
            instance_id: self.instance_id.clone(),
            tx_id,
            tx_index,
            // FIXME(encrypt): Encrypt under key derived from seed
            state: monitor.encode(),
        };

        // Run an async fn inside a sync fn inside a Tokio runtime
        tokio::task::block_in_place(|| {
            Handle::current().block_on(async move {
                self.api.create_channel_monitor(channel_monitor).await
            })
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
        let tx_id = funding_txo.txid.to_hex();
        let tx_index = funding_txo.index.try_into().unwrap();
        println!("Updating persisted channel {}_{}", tx_id, tx_index);
        let channel_monitor = ChannelMonitor {
            instance_id: self.instance_id.clone(),
            tx_id,
            tx_index,
            // FIXME(encrypt): Encrypt under key derived from seed
            state: monitor.encode(),
        };

        // Run an async fn inside a sync fn inside a Tokio runtime
        tokio::task::block_in_place(|| {
            Handle::current().block_on(async move {
                self.api.update_channel_monitor(channel_monitor).await
            })
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
