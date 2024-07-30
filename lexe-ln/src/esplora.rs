use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, ensure, Context};
use arc_swap::ArcSwap;
use bitcoin::{blockdata::transaction::Transaction, OutPoint};
use common::{
    constants,
    ln::{
        hashes::LxTxid,
        network::LxNetwork,
        priority::{ConfirmationPriority, ToNumBlocks},
    },
    shutdown::ShutdownChannel,
    task::LxTask,
    test_event::TestEvent,
    Apply,
};
use esplora_client::{api::OutputStatus, AsyncClient};
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
    FEERATE_FLOOR_SATS_PER_KW,
};
use rand::{seq::SliceRandom, RngCore};
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

/// Shorthand for 1000 bitcoin weight units, i.e. one kwu.
const R1000_WU: bitcoin::Weight = bitcoin::Weight::from_wu(1000);

/// Whether this esplora url is contained in the whitelist for this network.
#[must_use]
pub fn url_is_whitelisted(esplora_url: &str, network: LxNetwork) -> bool {
    match network {
        LxNetwork::Mainnet =>
            constants::MAINNET_ESPLORA_WHITELIST.contains(&esplora_url),
        LxNetwork::Testnet =>
            constants::TESTNET_ESPLORA_WHITELIST.contains(&esplora_url),
        LxNetwork::Signet => todo!("Don't have a signet esplora whitelist yet"),
        // Regtest can use whatever
        LxNetwork::Regtest => true,
    }
}

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
    /// Cached map of conf targets (in number of blocks) to estimated feerates
    /// (in sats per vbyte) returned by [`AsyncClient::get_fee_estimates`].
    fee_estimates: ArcSwap<BTreeMap<usize, f64>>,
    test_event_tx: TestEventSender,
}

impl LexeEsplora {
    /// Try initializing a [`LexeEsplora`] from *any* of the given Esplora urls,
    /// trying all of the URLs until one succeeds or all fail. If successful,
    /// returns the client, the fee refresher task, and the chosen esplora url.
    pub async fn init_any(
        rng: &mut impl RngCore,
        mut esplora_urls: Vec<String>,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<(Arc<Self>, LxTask<()>, String)> {
        // Randomize the URL ordering for some basic load balancing
        esplora_urls.shuffle(rng);

        ensure!(!esplora_urls.is_empty(), "No urls provided");

        let mut err_msgs = Vec::new();
        for url in esplora_urls {
            info!("Initializing Esplora from url: {url}");
            let init_result = Self::init(
                url.clone(),
                test_event_tx.clone(),
                shutdown.clone(),
            )
            .await;

            match init_result {
                Ok((client, task)) => {
                    if !err_msgs.is_empty() {
                        let joined = err_msgs.join(", ");
                        warn!("At least one esplora init failed: [{joined}]");
                    }
                    return Ok((client, task, url));
                }
                Err(e) => err_msgs.push(format!("({url}, {e:#})")),
            }
        }

        let joined = err_msgs.join("; ");
        Err(anyhow!("LexeEsplora::init_any failed: [{joined}]"))
    }

    /// Initialize a [`LexeEsplora`] client.
    // NOTE: This makes a call to `/fee-estimates` both as a means to
    //
    // 1) Get the initial fee estimates.
    // 2) Check that the Esplora client is configured correctly.
    //
    // [`LexeEsplora::init_any`] relies on (2) to gracefully recover from 'bad'
    // Esplora URLs (which have likely been fixed in a later version).
    pub async fn init(
        esplora_url: String,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<(Arc<Self>, LxTask<()>)> {
        let google_ca_cert = reqwest11::Certificate::from_der(
            constants::GTS_ROOT_R1_CA_CERT_DER,
        )
        .context("Invalid Google CA cert der")?;
        let amazon_ca_cert = reqwest11::Certificate::from_der(
            constants::AMAZON_ROOT_CA_1_CERT_DER,
        )
        .context("Invalid Amazon Root CA cert der")?;
        let reqwest_client = reqwest11::ClientBuilder::new()
            .add_root_certificate(google_ca_cert)
            .add_root_certificate(amazon_ca_cert)
            .timeout(ESPLORA_CLIENT_TIMEOUT)
            .build()
            .context("Failed to build reqwest client")?;

        // Initialize inner esplora client
        let client = AsyncClient::from_client(esplora_url, reqwest_client);

        // Initial cached fee estimates
        let fee_estimates = client
            .get_fee_estimates()
            .await
            .map(convert_fee_estimates)
            .map(ArcSwap::from_pointee)
            .context("Could not fetch initial esplora fee estimates")?;

        // Instantiate
        let esplora = Arc::new(Self {
            client,
            fee_estimates,
            test_event_tx,
        });

        // Spawn refresh fees task
        let task = Self::spawn_refresh_fees_task(esplora.clone(), shutdown);

        Ok((esplora, task))
    }

    /// Spawns a task that periodically calls [`Self::refresh_fee_estimates`].
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
                    res = esplora.refresh_fee_estimates() => res,
                    () = shutdown.recv() => break,
                };

                match try_refresh {
                    Ok(()) => debug!("Successfully refreshed feerates."),
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

    /// Refreshes our cached fee estimates.
    async fn refresh_fee_estimates(&self) -> anyhow::Result<()> {
        let fee_estimates = self
            .client
            .get_fee_estimates()
            .await
            .map(convert_fee_estimates)
            .context("Could not update cached Esplora fee estimates")?;

        self.fee_estimates.store(Arc::new(fee_estimates));

        Ok(())
    }

    /// Convert a target # of blocks into a [`bdk29::FeeRate`] via a cache
    /// lookup. Since [`bdk29::FeeRate`] is easily convertible to other
    /// units, this is the core feerate function that others delegate to.
    pub fn num_blocks_to_bdk_feerate(
        &self,
        num_blocks: usize,
    ) -> bdk29::FeeRate {
        let guarded_fee_estimates = self.fee_estimates.load();
        let feerate_satsvbyte =
            lookup_fee_rate(num_blocks, &guarded_fee_estimates);
        bdk29::FeeRate::from_sat_per_vb(feerate_satsvbyte as f32)
    }

    /// Convert a [`ConfirmationPriority`] into a [`bdk29::FeeRate`].
    pub fn conf_prio_to_bdk_feerate(
        &self,
        conf_prio: ConfirmationPriority,
    ) -> bdk29::FeeRate {
        let num_blocks = conf_prio.to_num_blocks();
        self.num_blocks_to_bdk_feerate(num_blocks)
    }

    /// Convert a [`ConfirmationTarget`] into a [`bdk29::FeeRate`].
    /// This calls into the [`FeeEstimator`] impl, which as of LDK v0.0.118
    /// requires some special post-estimation logic.
    pub fn conf_target_to_bdk_feerate(
        &self,
        conf_target: ConfirmationTarget,
    ) -> bdk29::FeeRate {
        let fee_for_1000_wu =
            self.get_est_sat_per_1000_weight(conf_target) as u64;
        bdk29::FeeRate::from_wu(fee_for_1000_wu, R1000_WU)
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
            .context("Could not fetch tx status")?;

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
        // Munge with units to get to sats per 1000 weight unit required by LDK
        let num_blocks = conf_target.to_num_blocks();
        let feerate = self.num_blocks_to_bdk_feerate(num_blocks);

        // LDK v0.0.118 introduced changes to `ConfirmationTarget` which require
        // some post-estimation adjustments to the fee rates, which we do here.
        // Our FeeEstimator implementation is based on ldk-node's. More info:
        // https://github.com/lightningdevkit/rust-lightning/releases/tag/v0.0.118
        let adjusted_fee_rate = match conf_target {
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee => {
                let sats_1000wu = feerate.fee_wu(R1000_WU);
                let adjusted_sats_1000wu = sats_1000wu.saturating_sub(250);
                bdk29::FeeRate::from_sat_per_kwu(adjusted_sats_1000wu as f32)
            }
            _ => feerate,
        };

        // Ensure we don't fall below the minimum feerate required by LDK.
        let feerate_sats_per_1000_weight = adjusted_fee_rate
            .fee_wu(R1000_WU)
            .try_into()
            .expect("Overflow");
        cmp::max(feerate_sats_per_1000_weight, FEERATE_FLOOR_SATS_PER_KW)
    }
}

/// Converts the [`HashMap<String, f64>`] returned by
/// [`AsyncClient::get_fee_estimates`] into a parsed [`BTreeMap<usize, f64>`].
fn convert_fee_estimates(
    estimates: HashMap<String, f64>,
) -> BTreeMap<usize, f64> {
    estimates
        .into_iter()
        .filter_map(|(target_str, rate)| {
            let target = usize::from_str(&target_str)
                .inspect_err(|e| {
                    warn!("Invalid pair: ({target_str}, {rate}) ({e})");
                    debug_assert!(false);
                })
                .ok()?;
            Some((target, rate))
        })
        .collect::<BTreeMap<usize, f64>>()
}

/// A version of [`esplora_client::convert_fee_rate`] which avoids an N * log(N)
/// Vec sort (and `HashMap<String, f64>` clone) at every feerate lookup by
/// leveraging a parsed [`BTreeMap<usize, f64>`].
///
/// Functionality: Given a desired target number of blocks by which a tx is
/// confirmed, and the parsed return value of [`AsyncClient::get_fee_estimates`]
/// which maps [`usize`] conf targets (in number of blocks) to the [`f64`]
/// estimated fee rates (in sats per vbyte), extracts the estimated feerate
/// whose corresponding target is the largest of all targets less than or equal
/// to our desired target, or defaults to 1 sat per vbyte if our desired target
/// was lower than the smallest target with a fee estimate.
fn lookup_fee_rate(
    num_blocks_target: usize,
    esplora_estimates: &BTreeMap<usize, f64>,
) -> f64 {
    *esplora_estimates
        .iter()
        .rev()
        .find(|(num_blocks, _)| *num_blocks <= &num_blocks_target)
        .map(|(_, feerate)| feerate)
        .unwrap_or(&1.0)
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    /// Check equivalence of our [`lookup_fee_rate`] implementation and
    /// [`esplora_client`]'s.
    #[test]
    fn convert_fee_rate_equiv() {
        proptest!(|(
            estimates in any::<BTreeMap<usize, f64>>(),
            target in any::<usize>(),
        )| {
            let str_estimates = estimates
                .iter()
                .map(|(k, v)| (k.to_string(), *v))
                .collect::<HashMap<String, f64>>();

            let our_feerate_res = lookup_fee_rate(target, &estimates) as f32;
            let their_feerate_res =
                esplora_client::convert_fee_rate(target, str_estimates)
                    .expect("Their implementation is actually infallible");

            prop_assert_eq!(our_feerate_res, their_feerate_res);
        })
    }
}
