use std::ops::Deref;
use std::sync::Arc;

use bitcoin::hash_types;
use bitcoin::hashes::hex::ToHex;
use lightning::chain::{
    self, chainmonitor, channelmonitor, keysinterface, transaction,
};
use lightning::ln::channelmanager;
use lightning::routing::network_graph;
use lightning::util::ser::Writeable;
use lightning_background_processor::Persister;

use crate::bitcoind_client::BitcoindClient;
use crate::disk::FilesystemLogger; // TODO replace with db logger
use crate::{ChainMonitorType, ChannelManagerType};

pub struct PostgresPersister {}

impl PostgresPersister {
    pub fn new() -> Self {
        Self {}
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
    // TODO: We really need a way for the persister to inform the user that its
    // time to crash/shut down once these start returning failure.
    // A PermanentFailure implies we need to shut down since we're force-closing
    // channels without even broadcasting!

    fn persist_new_channel(
        &self,
        funding_txo: transaction::OutPoint,
        monitor: &channelmonitor::ChannelMonitor<ChannelSigner>,
        _update_id: chainmonitor::MonitorUpdateId,
    ) -> Result<(), chain::ChannelMonitorUpdateErr> {
        // Original FilesystemPersister filename: `id`, under folder "monitors"
        let id = format!("{}_{}", funding_txo.txid.to_hex(), funding_txo.index);
        let txo_plaintext_bytes = id.into_bytes();
        // FIXME(encrypt): Encrypt before send
        println!("Persisting new channel {:?}", txo_plaintext_bytes);

        let monitor_plaintext_bytes = monitor.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Channel monitor: {:?}", monitor_plaintext_bytes);

        Ok(()) // TODO implement
    }

    fn update_persisted_channel(
        &self,
        funding_txo: transaction::OutPoint,
        _update: &Option<channelmonitor::ChannelMonitorUpdate>,
        monitor: &channelmonitor::ChannelMonitor<ChannelSigner>,
        _update_id: chainmonitor::MonitorUpdateId,
    ) -> Result<(), chain::ChannelMonitorUpdateErr> {
        // Original FilesystemPersister filename: `id`, under folder "monitors"
        let id = format!("{}_{}", funding_txo.txid.to_hex(), funding_txo.index);
        let txo_plaintext_bytes = id.into_bytes();
        // FIXME(encrypt): Encrypt before send
        println!("Updating persisted channel {:?}", txo_plaintext_bytes);

        let monitor_plaintext_bytes = monitor.encode();
        // FIXME(encrypt): Encrypt before send
        println!("Channel monitor: {:?}", monitor_plaintext_bytes);

        Ok(()) // TODO implement
    }
}
