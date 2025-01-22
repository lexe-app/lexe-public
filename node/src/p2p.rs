use std::time::Duration;

use anyhow::Context;
use common::{cli::LspInfo, notify_once::NotifyOnce, task::LxTask};
use lexe_ln::p2p;
use tracing::{info, info_span, warn};

use crate::peer_manager::NodePeerManager;

const LSP_RECONNECT_INTERVAL: Duration = Duration::from_secs(60);

/// A task which makes the initial p2p connection to Lexe's LSP then spawns a
/// task to periodically reconnect to the LSP if we somehow disconnect after.
/// Returns only *after* the initial connect completes (or errors).
///
/// The task also disconnects from the LSP at shutdown to ensure we don't
/// continue updating our channel data after the BGP has stopped.
pub(crate) async fn connect_to_lsp_then_spawn_connector_task(
    peer_manager: NodePeerManager,
    lsp: &LspInfo,
    mut shutdown: NotifyOnce,
) -> anyhow::Result<LxTask<()>> {
    let lsp_node_pk = lsp.node_pk;
    let lsp_addrs = [lsp.private_p2p_addr.clone()];

    // Do the initial connection to the LSP.
    p2p::connect_peer_if_necessary(&peer_manager, &lsp_node_pk, &lsp_addrs)
        .await
        .context("Failed initial connection to LSP")?;

    const SPAN_NAME: &str = "(lsp-connector)";
    Ok(LxTask::spawn_named_with_span(
        SPAN_NAME,
        info_span!(SPAN_NAME),
        async move {
            let mut timer = tokio::time::interval(LSP_RECONNECT_INTERVAL);

            // Consume the first tick since we just reconnected above
            timer.tick().await;

            loop {
                tokio::select! {
                    _ = timer.tick() => (),
                    () = shutdown.recv() => break,
                }

                let reconnect_fut = async {
                    let result = p2p::connect_peer_if_necessary(
                        &peer_manager,
                        &lsp_node_pk,
                        &lsp_addrs,
                    )
                    .await;

                    match result {
                        Ok(()) => info!("(Re)connected to LSP"),
                        Err(e) => warn!("Couldn't (re)connect to LSP: {e:#}"),
                    }
                };

                tokio::select! {
                    () = reconnect_fut => (),
                    () = shutdown.recv() => break,
                }
            }

            info!("Received shutdown; disconnecting from LSP");
            peer_manager.disconnect_all_peers();

            info!("LSP connector task complete");
        },
    ))
}
