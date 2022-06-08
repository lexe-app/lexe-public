use std::convert::TryInto;
use std::ops::Deref;
use std::sync::Arc;

use bitcoin::hash_types;
use bitcoin::hashes::hex::ToHex;
use bitcoin::secp256k1::key::PublicKey;
use lightning::chain::{
    chainmonitor, channelmonitor, keysinterface, transaction,
    ChannelMonitorUpdateErr,
};
use lightning::ln::channelmanager;
use lightning::routing::network_graph;
use lightning::util::ser::Writeable;
use lightning_background_processor::Persister;

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
    ) -> Result<(hash_types::BlockHash, ChannelManagerType), std::io::Error>
    {
        // FIXME(decrypt): Decrypt first
        unimplemented!(); // TODO implement
    }

    // Replaces equivalent method in lightning_persister::FilesystemPersister
    pub fn read_channelmonitors<Signer: keysinterface::Sign, K: Deref>(
        &self,
        keys_manager: K,
    ) -> Result<
        Vec<(
            hash_types::BlockHash,
            channelmonitor::ChannelMonitor<Signer>,
        )>,
        std::io::Error,
    >
    where
        K::Target: keysinterface::KeysInterface<Signer = Signer> + Sized,
    {
        // FIXME(decrypt): Decrypt first
        Ok(Vec::new()) // TODO implement
    }
}

impl
    Persister<
        keysinterface::InMemorySigner,
        Arc<ChainMonitorType>,
        Arc<BitcoindClient>,
        Arc<keysinterface::KeysManager>,
        Arc<BitcoindClient>,
        Arc<FilesystemLogger>,
    > for PostgresPersister
{
    fn persist_manager(
        &self,
        channel_manager: &channelmanager::SimpleArcChannelManager<
            ChainMonitorType,
            BitcoindClient,
            BitcoindClient,
            FilesystemLogger,
        >,
    ) -> Result<(), std::io::Error> {
        // Original FilesystemPersister filename: "manager"
        let plaintext_bytes = channel_manager.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Channel manager: {:?}", plaintext_bytes);

        Ok(()) // TODO implement
    }

    fn persist_graph(
        &self,
        network_graph: &network_graph::NetworkGraph,
    ) -> Result<(), std::io::Error> {
        // Original FilesystemPersister filename: "network_graph"
        let plaintext_bytes = network_graph.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Network graph: {:?}", plaintext_bytes);

        Ok(()) // TODO implement
    }
}

impl<ChannelSigner: keysinterface::Sign> chainmonitor::Persist<ChannelSigner>
    for PostgresPersister
{
    fn persist_new_channel(
        &self,
        funding_txo: transaction::OutPoint,
        monitor: &channelmonitor::ChannelMonitor<ChannelSigner>,
        _update_id: chainmonitor::MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let tx_id = funding_txo.txid.to_hex();
        let tx_index = funding_txo.index.try_into().unwrap();
        println!("Persisting new channel {}_{}", tx_id, tx_index);
        let channel_monitor = ChannelMonitor {
            node_public_key: self.pubkey.clone(),
            tx_id,
            tx_index,
            // FIXME(encrypt): Encrypt before send
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
        funding_txo: transaction::OutPoint,
        _update: &Option<channelmonitor::ChannelMonitorUpdate>,
        monitor: &channelmonitor::ChannelMonitor<ChannelSigner>,
        _update_id: chainmonitor::MonitorUpdateId,
    ) -> Result<(), ChannelMonitorUpdateErr> {
        let tx_id = funding_txo.txid.to_hex();
        let tx_index = funding_txo.index.try_into().unwrap();
        println!("Updating persisted channel {}_{}", tx_id, tx_index);
        let channel_monitor = ChannelMonitor {
            node_public_key: self.pubkey.clone(),
            tx_id,
            tx_index,
            // FIXME(encrypt): Encrypt before send
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
