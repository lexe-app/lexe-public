use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use lexe_api::def::NodeLspApi;
use lexe_ln::scorer::{LexeScorer, LspLiquidity};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::time::{self, Instant};
use tracing::{debug, info, info_span, warn};

use crate::client::NodeLspClient;

/// An actor which keeps the meganode scorer's LSP liquidity fresh.
pub(crate) struct LspLiquidityUpdater {
    lsp_api: Arc<NodeLspClient>,
    scorer: Arc<Mutex<LexeScorer>>,
}

impl LspLiquidityUpdater {
    /// How often to fetch the LSP's external channels.
    const UPDATE_INTERVAL: Duration = Duration::from_secs(30);

    /// `scorer` should have just been built from the LSP's external channels.
    pub fn new(
        lsp_api: Arc<NodeLspClient>,
        scorer: Arc<Mutex<LexeScorer>>,
    ) -> Self {
        Self { lsp_api, scorer }
    }

    pub fn spawn(self, shutdown: NotifyOnce) -> LxTask<()> {
        const SPAN_NAME: &str = "(lsp-liquidity-updater)";
        LxTask::spawn_with_span(
            SPAN_NAME,
            info_span!(SPAN_NAME),
            self.run(shutdown),
        )
    }

    async fn run(self, mut shutdown: NotifyOnce) {
        // The scorer is fresh, so skip the immediate first tick.
        let start = Instant::now() + Self::UPDATE_INTERVAL;
        let mut interval = time::interval_at(start, Self::UPDATE_INTERVAL);

        loop {
            tokio::select! {
                _ = interval.tick() => (),
                () = shutdown.recv() => break,
            }

            tokio::select! {
                () = self.try_update() => (),
                () = shutdown.recv() => break,
            }
        }

        info!("LSP liquidity updater task complete");
    }

    async fn try_update(&self) {
        match self.lsp_api.get_external_channels().await {
            Ok(external_channels) => {
                let channels = &external_channels.channels;
                let liquidity = LspLiquidity::from_external_channels(channels);
                self.scorer.lock().unwrap().set_lsp_liquidity(liquidity);
                let num_channels = channels.len();
                debug!(num_channels, "Updated LSP liquidity");
            }
            Err(err) => warn!("Couldn't fetch LSP external channels: {err:#}"),
        }
    }
}
