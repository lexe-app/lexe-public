use std::convert::TryInto;
use std::io::{self, Cursor};
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
use lightning::ln::channelmanager::SimpleArcChannelManager;
use lightning::routing::network_graph::NetworkGraph;
use lightning::util::ser::{ReadableArgs, Writeable};
use lightning_background_processor::Persister;

use anyhow::{anyhow, ensure, Context};
use reqwest::Client;
use tokio::runtime::Handle;

use crate::api::{self, ChannelMonitor};
use crate::bitcoind_client::BitcoindClient;
use crate::disk::FilesystemLogger; // TODO replace with db logger
use crate::{ChainMonitorType, ChannelManagerType};

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
    pub fn read_channelmanager(
        &self,
    ) -> anyhow::Result<(BlockHash, ChannelManagerType)> {
        // FIXME(decrypt): Decrypt first
        todo!(); // TODO implement
    }

    // Replaces equivalent method in lightning_persister::FilesystemPersister
    pub fn read_channelmonitors<Signer: Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> anyhow::Result<Vec<(BlockHash, LdkChannelMonitor<Signer>)>>
    where
        K::Target: KeysInterface<Signer = Signer> + Sized,
    {
        let cm_vec = tokio::task::block_in_place(|| {
            Handle::current().block_on(async move {
                api::get_channel_monitors(&self.client, self.pubkey.clone())
                    .await
            })
        })
        .map_err(|e| {
            println!("{:?}", e);
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
                // LDK's DecodeError is Debug but doesn't impl std::error::Error
                .map_err(|e| anyhow!("{:?}", e))
                .context("Failed to deserialize Channel Monitor")?;

            let (output, _script) = channel_monitor.get_funding_txo();
            ensure!(output.txid == tx_id, "Deserialized txid don' match");
            ensure!(output.index == tx_index, "Deserialized index don' match");

            result.push((blockhash, channel_monitor));
        }

        Ok(result)
    }
}

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
        // Original FilesystemPersister filename: "manager"
        let plaintext_bytes = channel_manager.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Channel manager: {:?}", plaintext_bytes);

        Ok(()) // TODO implement
    }

    fn persist_graph(
        &self,
        network_graph: &NetworkGraph,
    ) -> Result<(), io::Error> {
        // Original FilesystemPersister filename: "network_graph"
        let plaintext_bytes = network_graph.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Network graph: {:?}", plaintext_bytes);

        Ok(()) // TODO implement
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
