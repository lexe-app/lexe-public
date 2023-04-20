use std::cmp;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use bdk::FeeRate;
use bitcoin::blockdata::transaction::Transaction;
use common::constants::GOOGLE_CA_CERT_DER;
use common::reqwest;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use esplora_client::AsyncClient;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
    FEERATE_FLOOR_SATS_PER_KW,
};
use tokio::time;
use tracing::{debug, error, info, instrument, warn};

use crate::test_event::{TestEvent, TestEventSender};

/// The interval at which we refresh estimated fee rates.
// Since we want to reduce the number of API calls made to our (external)
// Esplora backend, we set this to a fairly high value of refreshing just once
// an hour. There is a guaranteed refresh at init.
const REFRESH_FEE_ESTIMATES_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The duration after which requests to the Esplora API will time out.
const ESPLORA_CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// Enumerates all [`ConfirmationTarget`]s.
const ALL_CONF_TARGETS: [ConfirmationTarget; 3] = [
    ConfirmationTarget::HighPriority,
    ConfirmationTarget::Normal,
    ConfirmationTarget::Background,
];

/// A version of [`esplora_client::convert_fee_rate`] which avoids cloning the
/// entire HashMap when computing the feerate in sats/vbytes.
fn convert_fee_rate(
    target: usize,
    esplora_estimates: &HashMap<String, f64>,
) -> anyhow::Result<f32> {
    let fee_val = {
        let mut pairs = esplora_estimates
            .iter()
            .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v)))
            .collect::<Vec<_>>();
        pairs.sort_unstable_by_key(|(k, _)| std::cmp::Reverse(*k));
        pairs
            .into_iter()
            .find(|(k, _)| k <= &target)
            .map(|(_, v)| v)
            .unwrap_or(&1.0)
    };

    Ok(*fee_val as f32)
}

/// Convert a [`ConfirmationTarget`] to a human-readable &str.
// TODO(max): Remove once LDK#1963 is merged and released
fn conf_to_str(conf_target: ConfirmationTarget) -> &'static str {
    match conf_target {
        ConfirmationTarget::HighPriority => "high priority",
        ConfirmationTarget::Normal => "normal",
        ConfirmationTarget::Background => "background",
    }
}

/// Spawns a task that periodically calls the `refresh_all_fee_estimates` fn.
fn spawn_refresh_fees_task(
    esplora: Arc<LexeEsplora>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("refresh fees", async move {
        let mut interval = time::interval(REFRESH_FEE_ESTIMATES_INTERVAL);
        // Consume the first tick since fees were refreshed during init
        interval.tick().await;

        loop {
            tokio::select! {
                _ = interval.tick() => {}
                () = shutdown.recv() => break,
            }

            let try_refresh = tokio::select! {
                res = esplora.refresh_all_fee_estimates() => res,
                () = shutdown.recv() => break,
            };

            match try_refresh {
                Ok(()) => debug!("Successfull refreshed feerates."),
                Err(e) => warn!("Could not refresh feerates: {e:#}"),
            }
        }

        info!("refresh fees task shutting down");
    })
}

pub struct LexeEsplora {
    client: AsyncClient,
    test_event_tx: TestEventSender,

    // --- Cached fee estimations --- //
    high_prio_fees: AtomicU32,
    normal_fees: AtomicU32,
    background_fees: AtomicU32,
}

impl LexeEsplora {
    pub async fn init(
        esplora_url: String,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<(Arc<Self>, LxTask<()>)> {
        // We need to manually trust Blockstream's CA (i.e. Google Trust
        // Services) since we don't trust any roots by default.
        let google_ca_cert =
            reqwest::tls::Certificate::from_der(GOOGLE_CA_CERT_DER)
                .context("Invalid Google CA der cert")?;
        let reqwest_client = reqwest::ClientBuilder::new()
            .add_root_certificate(google_ca_cert)
            .timeout(ESPLORA_CLIENT_TIMEOUT)
            .build()
            .context("Failed to build reqwest client")?;

        // Initialize inner esplora client
        let client = AsyncClient::from_client(esplora_url, reqwest_client);

        // Initialize the fee rate estimates to some sane default values
        let high_prio_fees = AtomicU32::new(5000);
        let normal_fees = AtomicU32::new(2000);
        let background_fees = AtomicU32::new(FEERATE_FLOOR_SATS_PER_KW);

        // Instantiate
        let esplora = Arc::new(Self {
            client,
            test_event_tx,
            background_fees,
            normal_fees,
            high_prio_fees,
        });

        // Do initial refresh of all fee estimates
        esplora
            .refresh_all_fee_estimates()
            .await
            .context("Could not initial fee estimates")?;

        // Spawn refresh fees task
        let task = spawn_refresh_fees_task(esplora.clone(), shutdown);

        Ok((esplora, task))
    }

    /// Returns a reference to the underlying [`AsyncClient`].
    pub fn client(&self) -> &AsyncClient {
        &self.client
    }

    /// Refreshes all current fee estimates.
    async fn refresh_all_fee_estimates(&self) -> anyhow::Result<()> {
        // Why does this return `HashMap<String, _>`???
        let esplora_estimates = self
            .client
            .get_fee_estimates()
            .await
            .context("Could not fetch esplora's fee estimates")?;

        for conf_target in ALL_CONF_TARGETS {
            self.refresh_single_fee_estimate(conf_target, &esplora_estimates)
                .with_context(|| {
                    let conf_str = conf_to_str(conf_target);
                    format!("Could not refresh fees for {conf_str}")
                })?;
        }

        Ok(())
    }

    /// Refreshes the current fee estimate for a [`ConfirmationTarget`] given a
    /// `HashMap<String, f64>` returned by [`AsyncClient::get_fee_estimates`].
    /// Returns the `u32` sats per 1000 weight that was stored in the cache.
    fn refresh_single_fee_estimate(
        &self,
        conf_target: ConfirmationTarget,
        esplora_estimates: &HashMap<String, f64>,
    ) -> anyhow::Result<u32> {
        // Convert the conf target to a target number of blocks.
        let num_blocks_target = match conf_target {
            ConfirmationTarget::HighPriority => 3,
            ConfirmationTarget::Normal => 12,
            ConfirmationTarget::Background => 144,
        };

        // Munge with units to get to sats per 1000 weight unit required by LDK
        let feerate_satsvbyte =
            convert_fee_rate(num_blocks_target, esplora_estimates)
                .context("Could not convert feerate to sats/vbytes")?;
        let bdk_feerate = FeeRate::from_sat_per_vb(feerate_satsvbyte);
        let feerate_sats_per_1000_weight = bdk_feerate.fee_wu(1000) as u32;

        // Ensure we don't fall below the minimum feerate required by LDK.
        let feerate_sats_per_1000_weight =
            cmp::max(feerate_sats_per_1000_weight, FEERATE_FLOOR_SATS_PER_KW);

        // Get a reference to the AtomicU32 we need to store the result in
        let ref_atomic_u32 = match conf_target {
            ConfirmationTarget::HighPriority => &self.high_prio_fees,
            ConfirmationTarget::Normal => &self.normal_fees,
            ConfirmationTarget::Background => &self.background_fees,
        };

        // Store the result and return
        ref_atomic_u32.store(feerate_sats_per_1000_weight, Ordering::Release);

        Ok(feerate_sats_per_1000_weight)
    }

    pub fn get_bdk_feerate(&self, conf_target: ConfirmationTarget) -> FeeRate {
        let feerate_sats_per_1000_weight =
            self.get_est_sat_per_1000_weight(conf_target);
        FeeRate::from_wu(feerate_sats_per_1000_weight as u64, 1000)
    }

    /// Broadcast a [`Transaction`].
    ///
    /// - Logs a debug message if successful.
    /// - Logs an error message if the broadcast failed.
    /// - Sends a [`TestEvent::TxBroadcasted`] if successful.
    pub async fn broadcast_tx(&self, tx: &Transaction) -> anyhow::Result<()> {
        Self::broadcast_tx_inner(&self.client, &self.test_event_tx, tx).await
    }

    #[instrument(skip_all, name = "(broadcast-tx)")]
    async fn broadcast_tx_inner(
        client: &AsyncClient,
        test_event_tx: &TestEventSender,
        tx: &Transaction,
    ) -> anyhow::Result<()> {
        let txid = tx.txid();
        client
            .broadcast(tx)
            .await
            .context("esplora_client failed to broadcast tx")
            .inspect(|&()| debug!("Successfully broadcasted tx {txid}"))
            .inspect(|&()| test_event_tx.send(TestEvent::TxBroadcasted))
            .inspect_err(|e| error!("Could not broadcast tx {txid}: {e:#}"))
    }
}

impl BroadcasterInterface for LexeEsplora {
    fn broadcast_transaction(&self, tx: &Transaction) {
        // We can't make LexeEsplora clonable because LDK's API requires a
        // `Deref<Target: FeeEstimator>` and making LexeEsplora Deref to a inner
        // version of itself is a dumb way to accomplish that. Instead, we have
        // the `broadcast_tx_inner` static method which is good enough.
        let client = self.client.clone();
        let test_event_tx = self.test_event_tx.clone();
        let tx = tx.clone();

        // Clippy bug; we need the `async move` to make the future static
        #[allow(clippy::redundant_async_block)]
        LxTask::spawn(async move {
            LexeEsplora::broadcast_tx_inner(&client, &test_event_tx, &tx).await
        })
        .detach()
    }
}

impl FeeEstimator for LexeEsplora {
    fn get_est_sat_per_1000_weight(
        &self,
        conf_target: ConfirmationTarget,
    ) -> u32 {
        use ConfirmationTarget::*;
        match conf_target {
            HighPriority => self.high_prio_fees.load(Ordering::Acquire),
            Normal => self.normal_fees.load(Ordering::Acquire),
            Background => self.background_fees.load(Ordering::Acquire),
        }
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use std::collections::HashMap;

    use proptest::arbitrary::any;
    use proptest::{prop_assert_eq, proptest};

    /// Check that our [`convert_fee_rate`] function is equivalent to
    /// [`esplora_client`]'s.
    #[test]
    fn convert_fee_rate_equiv() {
        proptest!(|(
            parsed_estimates in any::<HashMap<usize, f64>>(),
            target in any::<usize>(),
        )| {
            let estimates = parsed_estimates
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect::<HashMap<String, f64>>();

            let our_feerate_res = super::convert_fee_rate(target, &estimates);
            let their_feerate_res =
                esplora_client::convert_fee_rate(target, estimates);

            match (our_feerate_res, their_feerate_res) {
                (Err(_), Err(_)) => {
                    // Both errored, good; don't compare the error types
                }
                (Ok(ours), Ok(theirs)) => prop_assert_eq!(ours, theirs),
                _ => panic!("Results did not match"),
            }
        })
    }
}
