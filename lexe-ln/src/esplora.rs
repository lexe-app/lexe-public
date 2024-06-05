use std::{
    cmp,
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, Context};
use bdk::FeeRate;
use bitcoin::{blockdata::transaction::Transaction, OutPoint};
use common::{
    constants, ln::hashes::LxTxid, shutdown::ShutdownChannel, task::LxTask,
    test_event::TestEvent, Apply,
};
use esplora_client::{api::OutputStatus, AsyncClient};
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
    FEERATE_FLOOR_SATS_PER_KW,
};
use tokio::time;
use tracing::{debug, error, info, instrument, warn};

use crate::test_event::TestEventSender;

/// The interval at which we refresh estimated fee rates.
// Since we want to reduce the number of API calls made to our (external)
// Esplora backend, we set this to a fairly high value of refreshing just once
// an hour. There is a guaranteed refresh at init.
const REFRESH_FEE_ESTIMATES_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The duration after which requests to the Esplora API will time out.
const ESPLORA_CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

/// The default `-mempoolexpiry` value in Bitcoin Core (14 days). If a
/// [`Transaction`] is older than this and still hasn't been confirmed, it is
/// likely that most nodes will have evicted this tx from their mempool. Txs
/// which have reached this age should be considered to be dropped.
const BITCOIN_CORE_MEMPOOL_EXPIRY: Duration =
    Duration::from_secs(60 * 60 * 24 * 14);

/// Enumerates all [`ConfirmationTarget`]s.
const ALL_CONF_TARGETS: [ConfirmationTarget; 4] = [
    ConfirmationTarget::HighPriority,
    ConfirmationTarget::Normal,
    ConfirmationTarget::Background,
    ConfirmationTarget::MempoolMinimum,
];

/// The minimum information about a [`bitcoin::Transaction`] required to query
/// Esplora for if the transaction has been confirmed or replaced.
pub struct TxConfQuery {
    pub txid: LxTxid,
    pub inputs: Vec<OutPoint>,
    pub created_at: SystemTime,
}

/// Enumerates the possible confirmation statuses of a given [`Transaction`].
pub enum TxConfStatus {
    /// The tx has not been included in a block, or the containing block has
    /// been orphaned.
    ZeroConf,
    /// The tx has been included in a block, and the containing block is in the
    /// best chain.
    InBestChain {
        /// The number of confirmations this tx has, e.g.:
        /// - Included in chain tip => 1 confirmation
        /// - Included in block with 5 more built on top => 6 confirmations
        confs: u32,
    },
    /// The tx is being replaced; i.e. at least one of its inputs is being
    /// spent by another tx which has at least 1 confirmation.
    HasReplacement {
        /// The number of confirmations that the replacement tx has.
        confs: u32,
        /// The txid of the replacement transaction.
        rp_txid: LxTxid,
    },
    /// All of the following are true:
    /// (1) The tx was not included in a block in the best chain.
    /// (2) We have not found a replacement tx with >0 confirmations.
    /// (3) The tx has reached the default `-mempoolexpiry` age, and is thus
    ///     likely to have been dropped from most nodes' mempools.
    Dropped,
}

pub struct LexeEsplora {
    client: AsyncClient,
    test_event_tx: TestEventSender,

    // --- Cached fee estimations --- //
    high_prio_fees: AtomicU32,
    normal_fees: AtomicU32,
    background_fees: AtomicU32,
    mempool_minimum_fees: AtomicU32,
}

impl LexeEsplora {
    pub async fn init(
        esplora_url: String,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<(Arc<Self>, LxTask<()>)> {
        let google_ca_cert = reqwest11::Certificate::from_der(
            constants::GTS_ROOT_R1_CA_CERT_DER,
        )
        .context("Invalid Google CA der cert")?;
        let letsencrypt_ca_cert = reqwest11::Certificate::from_der(
            constants::LETSENCRYPT_ROOT_CA_CERT_DER,
        )
        .context("Invalid Google CA der cert")?;
        let reqwest_client = reqwest11::ClientBuilder::new()
            .add_root_certificate(google_ca_cert)
            .add_root_certificate(letsencrypt_ca_cert)
            .timeout(ESPLORA_CLIENT_TIMEOUT)
            .build()
            .context("Failed to build reqwest client")?;

        // Initialize inner esplora client
        let client = AsyncClient::from_client(esplora_url, reqwest_client);

        // Initialize the fee rate estimates to some sane default values
        let high_prio_fees = AtomicU32::new(13_000); // 13 sat/vB
        let normal_fees = AtomicU32::new(6_000); // 6 sat/vB
        let background_fees = AtomicU32::new(1_000); // 1 sat/vB
        let mempool_minimum_fees = AtomicU32::new(FEERATE_FLOOR_SATS_PER_KW);

        // Instantiate
        let esplora = Arc::new(Self {
            client,
            test_event_tx,
            high_prio_fees,
            normal_fees,
            background_fees,
            mempool_minimum_fees,
        });

        // Do initial refresh of all fee estimates
        esplora
            .refresh_all_fee_estimates()
            .await
            .context("Could not initial fee estimates")?;

        // Spawn refresh fees task
        let task = Self::spawn_refresh_fees_task(esplora.clone(), shutdown);

        Ok((esplora, task))
    }

    /// Spawns a task that periodically calls `refresh_all_fee_estimates`.
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
                    format!("Could not refresh fees for {conf_target:?}")
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
            ConfirmationTarget::HighPriority => 1,
            ConfirmationTarget::Normal => 3,
            ConfirmationTarget::Background => 72,
            ConfirmationTarget::MempoolMinimum => 1008,
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
            ConfirmationTarget::MempoolMinimum => &self.mempool_minimum_fees,
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
        Self::broadcast_txs_inner(&self.client, &self.test_event_tx, &[tx])
            .await
    }

    #[instrument(skip_all, name = "(broadcast-tx)")]
    async fn broadcast_txs_inner(
        client: &AsyncClient,
        test_event_tx: &TestEventSender,
        txs: &[&Transaction],
    ) -> anyhow::Result<()> {
        if txs.is_empty() {
            return Err(anyhow!("We were given no transactions to broadcast"));
        }

        let num_txs = txs.len();
        info!("Broadcasting batch of {num_txs} txs");

        let results = txs
            .iter()
            .map(|tx| async {
                let txid = tx.txid();
                debug!("Broadcasting tx {txid}");
                let res = client.broadcast(tx).await.map_err(|e| {
                    anyhow!("Error broadcasting tx {txid}: {e:#}")
                });
                (txid, res)
            })
            .apply(futures::future::join_all)
            .await;

        let mut err_msgs = Vec::new();
        for (txid, res) in results {
            match res {
                Ok(()) => debug!("Successfully broadcasted {txid}"),
                Err(e) => err_msgs.push(format!("{e:#}")),
            }
        }

        if !err_msgs.is_empty() {
            let joined_msgs = err_msgs.join("; ");
            error!("Batch broadcast failed: {joined_msgs}");
            return Err(anyhow!("Batch broadcast failed: {joined_msgs}"));
        }

        test_event_tx.send(TestEvent::TxBroadcasted);
        info!("Batch broadcast of {num_txs} txs succeeded");
        Ok(())
    }

    /// Returns the [`TxConfStatus`]es for a list of [`TxConfQuery`]s.
    #[instrument(skip_all, name = "(get-tx-conf-statuses)")]
    pub async fn get_tx_conf_statuses<'query>(
        &self,
        queries: impl Iterator<Item = &'query TxConfQuery>,
    ) -> anyhow::Result<Vec<TxConfStatus>> {
        let now = SystemTime::now();

        // Get the block height of our best-known chain tip.
        let best_height = self
            .client
            .get_height()
            .await
            .context("Could not fetch block height")?;

        // Concurrently get the tx conf status for all input `TxConfQuery`s,
        // quitting early if any return an error.
        let conf_status_futs = queries
            .map(|query| self.get_tx_conf_status(best_height, now, query));
        let conf_statuses = futures::future::try_join_all(conf_status_futs)
            .await
            .context("Error computing conf statuses")?;

        Ok(conf_statuses)
    }

    /// Given our best block height, determine the confirmation status for a
    /// single [`TxConfQuery`].
    async fn get_tx_conf_status<'query>(
        &self,
        best_height: u32,
        now: SystemTime,
        query: &'query TxConfQuery,
    ) -> anyhow::Result<TxConfStatus> {
        // Fetch the tx status.
        let tx_status = self
            .client
            .get_tx_status(&query.txid.0)
            .await
            .context("Could not fetch tx status")?
            // The extra Option<_> is an esplora_client bug; Esplora always
            // returns Some for this endpoint.
            // https://github.com/bitcoindevkit/rust-esplora-client/pull/46
            .context("Txid somehow not found")?;

        // This is poorly documented, but the `GET /tx/:txid/status` handler in
        // Blockstream/electrs returns `Some(_)` if and only if (1) the tx has
        // been included in a block, and (2) the block is in the best chain.
        // https://github.com/Blockstream/electrs/blob/adedee15f1fe460398a7045b292604df2161adc0/src/rest.rs#L941
        if let Some(height) = tx_status.block_height {
            // Compute the # of confirmations by subtracting the containing
            // block height from the tip height. An occasional race is ok
            // because the confs checker task will try again later.
            let height_diff = best_height.checked_sub(height).context(
                "Best height wasn't actually the best height, OR \
                we hit a rare (but acceptable) TOCTTOU race",
            )?;

            // Don't forget to count the including block!
            let confs = height_diff + 1;

            return Ok(TxConfStatus::InBestChain { confs });
        }
        // By now, we know that this tx is not in the best chain.
        // Let's see if any of its inputs have been spent by another tx.

        // Fetch the output status for every input.
        let output_status_futs = query.inputs.iter().map(|outpoint| async {
            let output_status = self
                .client
                .get_output_status(&outpoint.txid, outpoint.vout.into())
                .await
                .context("Could not fetch output status")?
                .context("Input tx was not found")?;
            Ok(output_status)
        });
        let output_statuses = futures::future::join_all(output_status_futs)
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<OutputStatus>>>()?;

        // Map each output to its replacement (`rp_`) txid and # of confs,
        // then find and return the most confirmed of these if one exists.
        let maybe_replacement = output_statuses
            .into_iter()
            .filter_map(|output_status| {
                // Aborts if there was no spending txid.
                let rp_txid = LxTxid(output_status.txid?);
                // Aborts if there was no tx status for the spending tx.
                let rp_tx_status = output_status.status?;
                // Aborts if the spending tx status had no block height.
                let rp_height = rp_tx_status.block_height?;
                // This underflow is a rare but acceptable race; try again later
                let rp_height_diff = best_height.checked_sub(rp_height)?;
                let rp_confs = rp_height_diff + 1;

                Some((rp_txid, rp_confs))
            })
            .max_by_key(|(_txid, confs)| *confs);
        if let Some((rp_txid, confs)) = maybe_replacement {
            let conf_status = TxConfStatus::HasReplacement { rp_txid, confs };
            return Ok(conf_status);
        }

        // By now, we know (1) the tx is not in the best chain and (2) there is
        // no confirmed replacement for it. Check if it has likely been dropped.
        let tx_age = now
            .duration_since(query.created_at)
            .unwrap_or(Duration::ZERO);
        if tx_age > BITCOIN_CORE_MEMPOOL_EXPIRY {
            return Ok(TxConfStatus::Dropped);
        }

        // The tx is fresh, with no confs or replacements. It is simply 0-conf.
        Ok(TxConfStatus::ZeroConf)
    }
}

impl BroadcasterInterface for LexeEsplora {
    fn broadcast_transactions(&self, txs: &[&Transaction]) {
        // We can't make LexeEsplora clonable because LDK's API requires a
        // `Deref<Target: FeeEstimator>` and making LexeEsplora Deref to a inner
        // version of itself is a dumb way to accomplish that. Instead, we have
        // the `broadcast_txs_inner` static method which is good enough.
        let client = self.client.clone();
        let test_event_tx = self.test_event_tx.clone();
        let txs = txs.iter().copied().cloned().collect::<Vec<Transaction>>();

        // Clippy bug; we need the `async move` to make the future static
        #[allow(clippy::redundant_async_block)]
        LxTask::spawn(async move {
            let tx_refs = txs.iter().collect::<Vec<&Transaction>>();
            LexeEsplora::broadcast_txs_inner(
                &client,
                &test_event_tx,
                tx_refs.as_slice(),
            )
            .await
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
            MempoolMinimum => self.mempool_minimum_fees.load(Ordering::Acquire),
        }
    }
}

/// A version of [`esplora_client::convert_fee_rate`] which avoids cloning the
/// entire HashMap when computing the feerate in sats/vbytes.
///
/// Functionality: Given a desired target number of blocks by which a tx is
/// confirmed, and the return value of [`AsyncClient::get_fee_estimates`] which
/// maps string-encoded (why?) [`usize`] conf targets (in number of blocks) to
/// the [`f64`] estimated fee rates (in sats per vbyte), extracts the estimated
/// feerate whose corresponding target is the largest of all targets less than
/// or equal to our desired target, or defaults to 1 sat per vbyte if our
/// desired target was lower than the smallest target with a fee estimate.
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

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use std::collections::HashMap;

    use proptest::{arbitrary::any, prop_assert_eq, proptest};

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
