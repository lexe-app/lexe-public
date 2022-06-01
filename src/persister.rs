use std::sync::Arc;

use lightning::chain::{
    self, chainmonitor, channelmonitor, keysinterface, transaction,
};
use lightning::ln::channelmanager;
use lightning::routing::network_graph;
use lightning_background_processor::Persister;

use crate::bitcoind_client::BitcoindClient;
use crate::disk::FilesystemLogger; // TODO replace with db logger

pub struct PostgresPersister {}

impl
    Persister<
        keysinterface::InMemorySigner,
        Arc<
            chainmonitor::ChainMonitor<
                keysinterface::InMemorySigner,
                Arc<dyn chain::Filter + Send + Sync>,
                Arc<BitcoindClient>,
                Arc<BitcoindClient>,
                Arc<FilesystemLogger>,
                Arc<PostgresPersister>,
            >,
        >,
        Arc<BitcoindClient>,
        Arc<keysinterface::KeysManager>,
        Arc<BitcoindClient>,
        Arc<FilesystemLogger>,
    > for PostgresPersister
{
    fn persist_manager(
        &self,
        channel_manager: &channelmanager::SimpleArcChannelManager<
            chainmonitor::ChainMonitor<
                keysinterface::InMemorySigner,
                Arc<dyn chain::Filter + Send + Sync>,
                Arc<BitcoindClient>,
                Arc<BitcoindClient>,
                Arc<FilesystemLogger>,
                Arc<PostgresPersister>,
            >,
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
