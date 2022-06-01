use std::ops::Deref;
use std::sync::Arc;

use bitcoin::hash_types;
use lightning::chain::{
    self, chainmonitor, channelmonitor, keysinterface, transaction,
};
use lightning::ln::channelmanager;
use lightning::routing::network_graph;
use lightning_background_processor::Persister;

use crate::bitcoind_client::BitcoindClient;
use crate::disk::FilesystemLogger; // TODO replace with db logger
use crate::ChainMonitorType;

pub struct PostgresPersister {}

impl PostgresPersister {
    pub fn new() -> Self {
        Self {}
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
        unimplemented!(); // TODO implement
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
        Ok(()) // TODO implement
    }

    fn persist_graph(
        &self,
        network_graph: &network_graph::NetworkGraph,
    ) -> Result<(), std::io::Error> {
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
        Ok(()) // TODO implement
    }

    fn update_persisted_channel(
        &self,
        funding_txo: transaction::OutPoint,
        _update: &Option<channelmonitor::ChannelMonitorUpdate>,
        monitor: &channelmonitor::ChannelMonitor<ChannelSigner>,
        _update_id: chainmonitor::MonitorUpdateId,
    ) -> Result<(), chain::ChannelMonitorUpdateErr> {
        Ok(()) // TODO implement
    }
}
