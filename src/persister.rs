use std::convert::TryInto;
use std::io::{self, Cursor, ErrorKind};
use std::ops::Deref;
use std::sync::Arc;

use bitcoin::hash_types::{BlockHash, Txid};
use bitcoin::hashes::hex::{FromHex, ToHex};
use bitcoin::secp256k1::key::PublicKey;
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
use lightning::routing::network_graph::NetworkGraph;
use lightning::routing::scoring::{
    ProbabilisticScorer as LdkProbabilisticScorer,
    ProbabilisticScoringParameters,
};
use lightning::util::config::UserConfig;
use lightning::util::ser::{ReadableArgs, Writeable};
use lightning_background_processor::Persister;

use anyhow::{anyhow, ensure, Context};
use once_cell::sync::{Lazy, OnceCell};
use reqwest::Client;
use tokio::runtime::{Builder, Handle, Runtime};

use crate::api::{self, ChannelManager, ChannelMonitor, ProbabilisticScorer};
use crate::bitcoind_client::BitcoindClient;
use crate::disk::FilesystemLogger; // TODO replace with db logger
use crate::{ChainMonitorType, ChannelManagerType};

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
        logger: Arc<FilesystemLogger>,
        user_config: UserConfig,
    ) -> anyhow::Result<Option<(BlockHash, ChannelManagerType)>> {
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
        graph: Arc<NetworkGraph>,
    ) -> anyhow::Result<LdkProbabilisticScorer<Arc<NetworkGraph>>> {
        let params = ProbabilisticScoringParameters::default();
        let ps_opt =
            api::get_probabilistic_scorer(&self.client, self.pubkey.clone())
                .await
                .map_err(|e| {
                    println!("{:#}", e);
                    e
                })
                .context("Could not fetch probabilistic scorer from DB")?;

        let ps = match ps_opt {
            Some(ps) => {
                let mut state_buf = Cursor::new(&ps.state);
                LdkProbabilisticScorer::read(
                    &mut state_buf,
                    (params, Arc::clone(&graph)),
                )
                // LDK DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize ProbabilisticScorer")?
            }
            None => LdkProbabilisticScorer::new(params, graph),
        };

        Ok(ps)
    }

    /// This function does not borrow a `LdkProbabilisticScorer` like the others
    /// would because the LdkProbabilisticScorer is wrapped in an
    /// `Arc<Mutex<T>>` in the calling function, which requires that the
    /// `MutexGuard` to the LdkProbabilisticScorer is held across
    /// `create_or_update_probabilistic_scorer().await`. However, this cannot be
    /// done since MutexGuard is not `Send`.
    ///
    /// Taking in the api::ProbabilisticScorer struct directly avoids this
    /// problem but necessitates a bit more code in the caller.
    pub async fn persist_probabilistic_scorer(
        &self,
        ps: ProbabilisticScorer,
    ) -> anyhow::Result<()> {
        api::create_or_update_probabilistic_scorer(&self.client, ps)
            .await
            .map(|_| ())
            .context("Could not persist probabilistic scorer to DB")
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
impl
    Persister<
        InMemorySigner,
        Arc<ChainMonitorType>,
        Arc<BitcoindClient>,
        Arc<KeysManager>,
        Arc<BitcoindClient>,
        Arc<FilesystemLogger>,
    > for PostgresPersister
{
    fn persist_manager(
        &self,
        channel_manager: &SimpleArcChannelManager<
            ChainMonitorType,
            BitcoindClient,
            BitcoindClient,
            FilesystemLogger,
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
        network_graph: &NetworkGraph,
    ) -> Result<(), io::Error> {
        // Original FilesystemPersister filename: "network_graph"
        // FIXME(encrypt): Encrypt under key derived from seed
        let _plaintext_bytes = network_graph.encode();
        // println!("Network graph: {:?}", plaintext_bytes);
        // Run an async fn inside a sync fn downstream of thread::spawn()

        // NOTE: We don't really need to implement this for now
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
