//! The recovery flow: close every channel and drive every satoshi of channel
//! funds into the BDK wallet.

use std::{sync::Arc, time::Duration};

use anyhow::Context;
use lexe_ln::{
    background_processor::QuiescenceRequest, channel::ChannelEvent,
    esplora::LexeEsplora, sync::BdkSyncRequest, wallet::OnchainWallet,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use tokio::sync::{mpsc, oneshot};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    peer_manager::RecoveryPeerManager,
    persister::RecoveryPersister,
    types::CloseMode,
};

/// Everything the recovery loop operates on. Assembled by
/// [`RecoveryNode::init`] and owned by the recovery task.
///
/// [`RecoveryNode::init`]: crate::node::RecoveryNode::init
pub(crate) struct RecoveryCtx {
    pub bdk_resync_tx: mpsc::Sender<BdkSyncRequest>,
    pub bgp_control_tx: mpsc::Sender<QuiescenceRequest>,
    pub chain_monitor: Arc<ChainMonitorType>,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub channel_manager: Arc<ChannelManagerType>,
    pub close_mode: CloseMode,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub esplora: Arc<LexeEsplora>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub peer_manager: RecoveryPeerManager,
    pub persister: Arc<RecoveryPersister>,
    pub shutdown: NotifyOnce,
    pub sync_interval: Duration,
    pub wallet: OnchainWallet,
}

impl RecoveryCtx {
    /// Triggers a BDK + LDK resync and waits for both to complete.
    async fn resync(&self) -> anyhow::Result<()> {
        let (bdk_tx, bdk_rx) = oneshot::channel();
        let (ldk_tx, ldk_rx) = oneshot::channel();
        self.bdk_resync_tx
            .send(BdkSyncRequest {
                full_sync: false,
                tx: bdk_tx,
            })
            .await
            .context("BDK sync task died")?;
        self.ldk_resync_tx
            .send(ldk_tx)
            .await
            .context("LDK sync task died")?;
        bdk_rx.await.context("BDK sync task dropped request")?;
        ldk_rx.await.context("LDK sync task dropped request")?;
        Ok(())
    }
}
