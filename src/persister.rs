use std::convert::TryInto;
use std::io::{self, Cursor, ErrorKind};
use std::net::SocketAddr;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

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

use anyhow::{anyhow, ensure, Context};
use once_cell::sync::{Lazy, OnceCell};
use reqwest::Client;
use tokio::runtime::{Builder, Handle, Runtime};

use crate::api::{
    self, ChannelManager, ChannelMonitor, ChannelPeer, NetworkGraph,
    ProbabilisticScorer as ApiProbabilisticScorer,
};
use crate::bitcoind_client::BitcoindClient;
use crate::cli;
use crate::logger::StdOutLogger;
use crate::{
    ChainMonitorType, ChannelManagerType, LoggerType, NetworkGraphType,
    ProbabilisticScorerType,
};

#[derive(Clone)]
pub struct PostgresPersister {
    client: Client,
    pubkey: String,
}

impl PostgresPersister {
    pub fn new(client: &Client, pubkey: PublicKey) -> Self {
        Self {
            client: client.clone(),
            pubkey: format!("{:x}", pubkey),
        }
    }

    // Replaces `ldk-sample/main::start_ldk` "Step 8: Init ChannelManager"
    #[allow(clippy::too_many_arguments)]
    pub async fn read_channel_manager(
        &self,
        channelmonitors: &mut [(
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
            api::get_channel_manager(&self.client, self.pubkey.clone())
                .await
                .map_err(|e| {
                    println!("{:#}", e);
                    e
                })
                .context("Could not fetch channel manager from DB")?;

        let cm_opt = match cm_opt {
            Some(cm) => {
                let mut channel_monitor_mut_refs = Vec::new();
                for (_, channel_monitor) in channelmonitors.iter_mut() {
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
            api::get_channel_monitors(&self.client, self.pubkey.clone())
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
        let ps_opt =
            api::get_probabilistic_scorer(&self.client, self.pubkey.clone())
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

    // TODO update this description
    /// This function does not borrow a `ProbabilisticScorer` like the others
    /// would because the ProbabilisticScorer is wrapped in an
    /// `Arc<Mutex<T>>` in the calling function, which requires that the
    /// `MutexGuard` to the ProbabilisticScorer is held across
    /// `create_or_update_probabilistic_scorer().await`. However, this cannot be
    /// done since MutexGuard is not `Send`.
    ///
    /// Taking in the api::ProbabilisticScorer struct directly avoids this
    /// problem but necessitates a bit more code in the caller.
    pub async fn persist_probabilistic_scorer(
        &self,
        ps: ApiProbabilisticScorer,
    ) -> anyhow::Result<()> {
        println!("Persisting probabilistic scorer");
        api::create_or_update_probabilistic_scorer(&self.client, ps)
            .await
            .map(|_| ())
            .context("Could not persist probabilistic scorer to DB")
    }

    pub async fn read_network_graph(
        &self,
        genesis_hash: BlockHash,
        logger: LoggerType,
    ) -> anyhow::Result<NetworkGraphType> {
        println!("Reading network graph");
        let ng_opt = api::get_network_graph(&self.client, self.pubkey.clone())
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
        let cp_vec = api::get_channel_peers(&self.client, self.pubkey.clone())
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
        peer_info_str: String,
    ) -> anyhow::Result<()> {
        let (peer_pubkey, peer_addr) = cli::parse_peer_info(peer_info_str)
            .context("Could not parse peer info from string")?;

        println!("Persisting new channel peer");
        let cp = ChannelPeer {
            node_public_key: self.pubkey.clone(),
            peer_public_key: format!("{:x}", peer_pubkey),
            peer_address: peer_addr.to_string(),
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

/// This trait is defined in the `lightning-background-processor` crate.
///
/// The methods in this trait are called downstream of
/// `BackgroundProcessor::start()` which calls `thread::spawn()` internally,
/// meaning that the thread-local context for these fns do not already contain
/// an async (Tokio) runtime. Thus, we offer a lazily-initialized
/// `PERSISTER_RUNTIME` above which `persist_manager` and `persist_graph` hook
/// into to run async closures.
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
            node_public_key: self.pubkey.clone(),
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
            node_public_key: self.pubkey.clone(),
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
        _scorer: &Mutex<ProbabilisticScorerType>,
    ) -> Result<(), io::Error> {
        // TODO
        Ok(())
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
            node_public_key: self.pubkey.clone(),
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
            node_public_key: self.pubkey.clone(),
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
