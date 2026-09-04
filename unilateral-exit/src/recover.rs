//! The recovery flow: close every channel and drive every satoshi of channel
//! funds into the BDK wallet.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use lexe_api::models::command::CloseChannelRequest;
use lexe_common::{api::user::NodePk, ln::channel::ChannelId};
use lexe_ln::{
    background_processor::QuiescenceRequest, channel::ChannelEvent, command,
    esplora::LexeEsplora, p2p, sync::BdkSyncRequest, wallet::OnchainWallet,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

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
    /// Closes all channels per [`CloseMode`], returning how many were coop
    /// closed and force closed.
    async fn close_all_channels(&mut self) -> anyhow::Result<(usize, usize)> {
        let channels = self.channel_manager.list_channels();
        let num_channels = channels.len();
        info!(%num_channels, "Closing all channels");
        if channels.is_empty() {
            return Ok((0, 0));
        }

        let can_force_close = self.close_mode.can_force_close();
        let lsp_connected = match self.close_mode.lsp() {
            None => {
                info!("Force closing all channels");
                false
            }
            Some(lsp) => {
                let try_connect = p2p::connect_peer_if_necessary(
                    &self.peer_manager,
                    &lsp.node_pk,
                    &lsp.addrs,
                )
                .await;
                match try_connect {
                    Ok(maybe_task) => {
                        if let Some(task) = maybe_task.0 {
                            let _ = self.eph_tasks_tx.try_send(task);
                        }
                        info!("Connected to LSP; will try cooperative closes");
                        true
                    }
                    Err(e) if !can_force_close =>
                        return Err(e).context("Couldn't connect to the LSP"),
                    Err(e) => {
                        warn!(
                            "Couldn't connect to LSP; force closing all \
                             channels: {e:#}"
                        );
                        false
                    }
                }
            }
        };

        let mut coop_closed = 0;
        let mut force_closed = 0;
        for channel in channels {
            // Each close can block until `close_channel`'s event timeout, so
            // check for shutdown between channels.
            if self.shutdown.try_recv() {
                return Err(anyhow!("Interrupted; re-run to resume"));
            }

            let channel_id = ChannelId::from(channel.channel_id);
            let counterparty = NodePk(channel.counterparty.node_id);
            // We connected to the LSP above, and force closes don't need the
            // peer, so `ensure_counterparty_connected` is a no-op.
            let close = |force_close: bool| {
                command::close_channel(
                    &self.channel_manager,
                    &self.channel_events_bus,
                    |_cp: NodePk| async { Ok(()) },
                    CloseChannelRequest {
                        channel_id,
                        force_close,
                        maybe_counterparty: Some(counterparty),
                    },
                )
            };

            if lsp_connected {
                let force_close = false;
                match close(force_close).await {
                    Ok(()) => {
                        // TODO(max): LDK's `convert_channel_err_internal`
                        // logs coop closes at ERROR as "Closed channel due to
                        // close-required error: Coop Closed". Upstream a fix
                        // which logs them at a lower level.
                        info!(%channel_id, "Cooperatively closed channel");
                        coop_closed += 1;
                        continue;
                    }
                    Err(e) if !can_force_close =>
                        return Err(e).with_context(|| {
                            format!("Couldn't coop close {channel_id}")
                        }),
                    Err(e) => warn!(
                        %channel_id,
                        "Coop close failed; falling back to force close: {e:#}"
                    ),
                }
            }

            let force_close = true;
            match close(force_close).await {
                Ok(()) => {
                    info!(%channel_id, "Force closed channel");
                    force_closed += 1;
                }
                // The channel may already be closing, and the monitor
                // recovers the funds regardless, so keep going.
                Err(e) => warn!(%channel_id, "Force close failed: {e:#}"),
            }
        }

        Ok((coop_closed, force_closed))
    }

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
