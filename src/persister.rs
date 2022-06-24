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
use reqwest::Client;
use tokio::runtime::{Builder, Handle, Runtime};

use crate::api::{
    self, ChannelManager, ChannelMonitor, ChannelPeer, NetworkGraph,
    ProbabilisticScorer as ApiProbabilisticScorer,
};
use crate::bitcoind_client::BitcoindClient;
use crate::convert;
use crate::logger::StdOutLogger;
use crate::types::{
    ChainMonitorType, ChannelManagerType, LoggerType, NetworkGraphType,
    ProbabilisticScorerType,
};

#[derive(Clone)]
pub struct PostgresPersister {
    client: Client,
    instance_id: String,
}

impl PostgresPersister {
    pub fn new(client: &Client, pubkey: &PublicKey, measurement: &str) -> Self {
        Self {
            client: client.clone(),
            instance_id: convert::get_instance_id(pubkey, measurement),
        }
    }

    // Replaces `ldk-sample/main::start_ldk` "Step 8: Init ChannelManager"
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
        logger: Arc<StdOutLogger>,
        user_config: UserConfig,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
        println!("Reading channel manager");
        let cm_opt =
            api::get_channel_manager(&self.client, self.instance_id.clone())
                .await
                .map_err(|e| {
                    println!("{:#}", e);
                    e
                })
                .context("Could not fetch channel manager from DB")?;

        let cm_opt = match cm_opt {
            Some(cm) => {
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

                let mut state_buf = Cursor::new(&cm.state);

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
        let cm_vec =
            api::get_channel_monitors(&self.client, self.instance_id.clone())
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
        let ps_opt = api::get_probabilistic_scorer(
            &self.client,
            self.instance_id.clone(),
        )
        .await
        .context("Could not fetch probabilistic scorer from DB")?;

        let ps = match ps_opt {
            Some(ps) => {
                let mut state_buf = Cursor::new(&ps.state);
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

        Ok(ps)
    }

    pub async fn read_network_graph(
        &self,
        genesis_hash: BlockHash,
        logger: LoggerType,
    ) -> anyhow::Result<NetworkGraphType> {
        println!("Reading network graph");
        let ng_opt =
            api::get_network_graph(&self.client, self.instance_id.clone())
                .await
                .context("Could not fetch network graph from DB")?;

        let ng = match ng_opt {
            Some(ng) => {
                let mut state_buf = Cursor::new(&ng.state);
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
        let cp_vec =
            api::get_channel_peers(&self.client, self.instance_id.clone())
                .await
                .map_err(|e| {
                    println!("{:#}", e);
                    e
                })
                .context("Could not fetch channel peers from DB")?;

        let mut result = Vec::new();

        for cp in cp_vec {
            let peer_pubkey = PublicKey::from_str(&cp.peer_public_key)
                .context("Could not deserialize PublicKey from LowerHex")?;
            let peer_addr = SocketAddr::from_str(&cp.peer_address)
                .context("Could not parse socket address from string")?;

            result.push((peer_pubkey, peer_addr));
        }

        Ok(result)
    }

    pub async fn persist_channel_peer(
        &self,
        peer_pubkey: PublicKey,
        peer_address: SocketAddr,
    ) -> anyhow::Result<()> {
        println!("Persisting new channel peer");
        let cp = ChannelPeer {
            instance_id: self.instance_id.clone(),
            peer_public_key: convert::pubkey_to_hex(&peer_pubkey),
            peer_address: peer_address.to_string(),
        };

        api::create_channel_peer(&self.client, cp)
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
        Arc<StdOutLogger>,
        Mutex<ProbabilisticScorerType>,
    > for PostgresPersister
{
    fn persist_manager(
        &self,
        channel_manager: &SimpleArcChannelManager<
            ChainMonitorType,
            BitcoindClient,
            BitcoindClient,
            StdOutLogger,
        >,
    ) -> Result<(), io::Error> {
        println!("Persisting channel manager");
        let channel_manager = ChannelManager {
            instance_id: self.instance_id.clone(),
            // FIXME(encrypt): Encrypt under key derived from seed
            state: channel_manager.encode(),
        };

        // Run an async fn inside a sync fn downstream of thread::spawn()
        PERSISTER_RUNTIME
            .get()
            .unwrap()
            .block_on(async move {
                api::create_or_update_channel_manager(
                    &self.client,
                    channel_manager,
                )
                .await
            })
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
        let network_graph = NetworkGraph {
            instance_id: self.instance_id.clone(),
            // FIXME(encrypt): Encrypt under key derived from seed
            state: network_graph.encode(),
        };

        // Run an async fn inside a sync fn downstream of thread::spawn()
        PERSISTER_RUNTIME
            .get()
            .unwrap()
            .block_on(async move {
                api::create_or_update_network_graph(&self.client, network_graph)
                    .await
            })
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

        let ps = {
            let scorer = scorer_mutex.lock().unwrap();
            ApiProbabilisticScorer {
                instance_id: self.instance_id.clone(),
                state: scorer.encode(),
            }
        };

        PERSISTER_RUNTIME.get().unwrap().block_on(async move {
            api::create_or_update_probabilistic_scorer(&self.client, ps)
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
                api::create_channel_monitor(&self.client, channel_monitor).await
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
                api::update_channel_monitor(&self.client, channel_monitor).await
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
