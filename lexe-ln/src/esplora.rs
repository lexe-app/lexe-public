use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, ensure, Context};
use arc_swap::ArcSwap;
use bitcoin::OutPoint;
use common::{
    constants,
    ln::{
        hashes::LxTxid,
        network::LxNetwork,
        priority::{ConfirmationPriority, ToNumBlocks},
    },
};
use esplora_client::{api::OutputStatus, AsyncClient};
use lexe_tls::rustls;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::chain::chaininterface::{
    ConfirmationTarget, FeeEstimator, FEERATE_FLOOR_SATS_PER_KW,
};
use rand::{seq::SliceRandom, RngCore};
use tokio::time;
use tracing::{debug, info, instrument, warn};

/// The interval at which we refresh estimated fee rates.
// Since we want to reduce the number of API calls made to our (external)
// Esplora backend, we set this to a fairly high value of refreshing just once
// an hour. There is a guaranteed refresh at init.
const REFRESH_FEE_ESTIMATES_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// The default `-mempoolexpiry` value in Bitcoin Core (14 days). If a
/// transaction is older than this and still hasn't been confirmed, it is
/// likely that most nodes will have evicted this tx from their mempool. Txs
/// which have reached this age should be considered to be dropped.
const BITCOIN_CORE_MEMPOOL_EXPIRY: Duration =
    Duration::from_secs(60 * 60 * 24 * 14);

/// The feerate we fall back to if fee rate lookup fails.
const FALLBACK_FEE_RATE: f64 = 1.0;

/// The timeout we'll use for requests to our Esplora backends.
pub const ESPLORA_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Whether this esplora url is contained in the whitelist for this network.
#[must_use]
pub fn url_is_whitelisted(esplora_url: &str, network: LxNetwork) -> bool {
    match network {
        LxNetwork::Mainnet =>
            constants::MAINNET_ESPLORA_WHITELIST.contains(&esplora_url),
        LxNetwork::Testnet3 =>
            constants::TESTNET3_ESPLORA_WHITELIST.contains(&esplora_url),
        LxNetwork::Testnet4 => todo!("Don't have testnet4 esplora whitelist"),
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

/// The possible confirmation statuses of a given [`bitcoin::Transaction`].
#[cfg_attr(test, derive(Debug))]
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

/// Thin wrapper around cached feerate estimates fetched from Esplora.
/// - The cached feerates are periodically updated by [`LexeEsplora`].
/// - Implements [`FeeEstimator`] and provides other useful getters.
pub struct FeeEstimates {
    /// Cached map of conf targets (in number of blocks) to estimated feerates
    /// (in sats per vbyte) returned by [`AsyncClient::get_fee_estimates`].
    cached: ArcSwap<BTreeMap<u16, f64>>,
}

impl FeeEstimates {
    /// Initialize from the result of [`AsyncClient::get_fee_estimates`].
    fn from_esplora_result(estimates: HashMap<u16, f64>) -> Arc<Self> {
        let cached = ArcSwap::from_pointee(Self::convert_estimates(estimates));
        Arc::new(Self { cached })
    }

    /// Updates the cached feerate estimates to the given value.
    fn update(&self, estimates: HashMap<u16, f64>) {
        let new_estimates = Self::convert_estimates(estimates);
        self.cached.store(Arc::new(new_estimates));
    }

    /// Convert a target # of blocks into a [`bitcoin::FeeRate`] via a cache
    /// lookup. Since [`bitcoin::FeeRate`] is easily convertible to other units,
    /// this is the core feerate function that others delegate to.
    pub fn num_blocks_to_feerate(&self, num_blocks: u16) -> bitcoin::FeeRate {
        let guarded_estimates = self.cached.load();
        let feerate_sats_vbyte =
            Self::lookup_fee_rate(num_blocks, &guarded_estimates);

        // (X sat/1 vb) * (1 vb/4 wu) * (1000 wu/1 kwu)
        // = (X sat/vb) * (250.0 vb/kwu)
        let feerate_sats_kwu = (feerate_sats_vbyte * 250.0) as u64;
        bitcoin::FeeRate::from_sat_per_kwu(feerate_sats_kwu)
    }

    /// Convert a [`ConfirmationPriority`] into a [`bitcoin::FeeRate`].
    pub fn conf_prio_to_feerate(
        &self,
        conf_prio: ConfirmationPriority,
    ) -> bitcoin::FeeRate {
        let num_blocks = conf_prio.to_num_blocks();
        self.num_blocks_to_feerate(num_blocks)
    }

    /// Convert a [`ConfirmationTarget`] into a [`bitcoin::FeeRate`].
    /// This calls into the [`FeeEstimator`] impl, which as of LDK v0.0.118
    /// requires some special post-estimation logic.
    pub fn conf_target_to_feerate(
        &self,
        conf_target: ConfirmationTarget,
    ) -> bitcoin::FeeRate {
        let fee_for_1000_wu =
            self.get_est_sat_per_1000_weight(conf_target) as u64;
        bitcoin::FeeRate::from_sat_per_kwu(fee_for_1000_wu)
    }

    /// A version of [`esplora_client::convert_fee_rate`] which avoids an N *
    /// log(N) Vec sort (and `HashMap<u16, f64>` clone) at every feerate lookup
    /// by leveraging a parsed [`BTreeMap<u16, f64>`].
    ///
    /// Functionality: Given a desired target number of blocks by which a tx is
    /// confirmed, and the parsed return value of
    /// [`AsyncClient::get_fee_estimates`] which maps [`u16`] conf targets (in
    /// number of blocks) to the [`f64`] estimated fee rates (in sats per
    /// vbyte), extracts the estimated feerate whose corresponding target is the
    /// largest of all targets less than or equal to our desired target, or
    /// defaults to 1 sat per vbyte if our desired target was lower than the
    /// smallest target with a fee estimate.
    fn lookup_fee_rate(
        num_blocks_target: u16,
        estimates: &BTreeMap<u16, f64>,
    ) -> f64 {
        estimates
            .iter()
            .rev()
            .find(|(num_blocks, _)| *num_blocks <= &num_blocks_target)
            .map(|(_, feerate)| *feerate)
            .unwrap_or(FALLBACK_FEE_RATE)
    }

    /// Converts [`HashMap<u16, f64>`] from [`AsyncClient::get_fee_estimates`]
    /// into the [`BTreeMap<usize, f64>`] stored by this struct.
    fn convert_estimates(estimates: HashMap<u16, f64>) -> BTreeMap<u16, f64> {
        estimates.into_iter().collect()
    }

    #[cfg(test)]
    pub(crate) fn dummy() -> Arc<Self> {
        let estimates = BTreeMap::from_iter([
            (1, 2.5),
            (3, 2.0),
            (5, 1.5),
            (10, 1.3),
            (20, 1.2),
            (1008, 1.1),
        ]);
        let cached = ArcSwap::from_pointee(estimates);
        Arc::new(Self { cached })
    }
}

impl FeeEstimator for FeeEstimates {
    fn get_est_sat_per_1000_weight(
        &self,
        conf_target: ConfirmationTarget,
    ) -> u32 {
        // Munge with units to get to sats per 1000 weight unit required by LDK
        let num_blocks = conf_target.to_num_blocks();
        let feerate = self.num_blocks_to_feerate(num_blocks);

        // LDK v0.0.118 introduced changes to `ConfirmationTarget` which require
        // some post-estimation adjustments to the fee rates, which we do here.
        // Our FeeEstimator implementation is based on ldk-node's. More info:
        // https://github.com/lightningdevkit/rust-lightning/releases/tag/v0.0.118
        let adjusted_fee_rate = match conf_target {
            ConfirmationTarget::MinAllowedNonAnchorChannelRemoteFee => {
                let sats_kwu = feerate.to_sat_per_kwu();
                let adjusted_sats_kwu = sats_kwu.saturating_sub(250);
                bitcoin::FeeRate::from_sat_per_kwu(adjusted_sats_kwu)
            }
            _ => feerate,
        };

        // Ensure we don't fall below the minimum feerate required by LDK.
        let feerate_sat_kwu = adjusted_fee_rate.to_sat_per_kwu();
        debug_assert!(feerate_sat_kwu <= u32::MAX as u64, "Feerate overflow");
        cmp::max(feerate_sat_kwu as u32, FEERATE_FLOOR_SATS_PER_KW)
    }
}

pub struct LexeEsplora {
    client: AsyncClient,
    fee_estimates: Arc<FeeEstimates>,
}

impl LexeEsplora {
    /// Try initializing a [`LexeEsplora`] from *any* of the given Esplora urls,
    /// trying all of the URLs until one succeeds or all fail. If successful,
    /// returns the client, the fee refresher task, and the chosen esplora url.
    pub async fn init_any(
        user_agent: &'static str,
        rng: &mut impl RngCore,
        mut esplora_urls: Vec<String>,
        shutdown: NotifyOnce,
    ) -> anyhow::Result<(Arc<Self>, Arc<FeeEstimates>, LxTask<()>, String)>
    {
        // Randomize the URL ordering for some basic load balancing
        esplora_urls.shuffle(rng);

        ensure!(!esplora_urls.is_empty(), "No urls provided");

        let mut err_msgs = Vec::new();
        for url in esplora_urls {
            info!("Initializing Esplora from url: {url}");
            let init_result =
                Self::init(user_agent, url.clone(), shutdown.clone()).await;

            match init_result {
                Ok((client, fee_estimates, task)) => {
                    if !err_msgs.is_empty() {
                        let joined = err_msgs.join(", ");
                        warn!("At least one esplora init failed: [{joined}]");
                    }
                    return Ok((client, fee_estimates, task, url));
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
        user_agent: &'static str,
        esplora_url: String,
        shutdown: NotifyOnce,
    ) -> anyhow::Result<(Arc<Self>, Arc<FeeEstimates>, LxTask<()>)> {
        // - Use WebPKI certs to avoid production outages when external Esplora
        //   providers change their CA certs.
        // - We must also use the default ring `CryptoProvider` because our
        //   providers may not support our specific ciphersuite, but we can at
        //   least enforce use of TLSv1.3.
        #[allow(clippy::disallowed_methods)]
        let tls_config = rustls::ClientConfig::builder_with_protocol_versions(
            lexe_tls::LEXE_TLS_PROTOCOL_VERSIONS,
        )
        .with_root_certificates(lexe_tls::WEBPKI_ROOT_CERTS.clone())
        .with_no_client_auth();

        // LexeEsplora wraps AsyncClient which in turn wraps reqwest::Client.
        let reqwest_client = {
            let builder = reqwest::Client::builder()
                .user_agent(user_agent)
                .https_only(true)
                .timeout(ESPLORA_REQUEST_TIMEOUT)
                .use_preconfigured_tls(tls_config);

            // Only allow http in tests
            #[cfg(any(test, feature = "test-utils"))]
            let builder = builder.https_only(false);

            builder
                .build()
                .expect("Failed to build esplora reqwest client")
        };
        let client = AsyncClient::from_client(esplora_url, reqwest_client);

        // Initial cached fee estimates
        let fee_estimates = client
            .get_fee_estimates()
            .await
            .map(FeeEstimates::from_esplora_result)
            .context("Could not fetch initial esplora fee estimates")?;

        let esplora = Arc::new(Self {
            client,
            fee_estimates: fee_estimates.clone(),
        });

        // Spawn refresh fees task
        let task = Self::spawn_refresh_fees_task(esplora.clone(), shutdown);

        Ok((esplora, fee_estimates, task))
    }

    /// Spawns a task that periodically calls [`Self::refresh_fee_estimates`].
    fn spawn_refresh_fees_task(
        esplora: Arc<LexeEsplora>,
        mut shutdown: NotifyOnce,
    ) -> LxTask<()> {
        LxTask::spawn("refresh fees", async move {
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
        let estimates = self
            .client
            .get_fee_estimates()
            .await
            .context("Could not update cached Esplora fee estimates")?;

        self.fee_estimates.update(estimates);

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
    async fn get_tx_conf_status(
        &self,
        best_height: u32,
        now: SystemTime,
        query: &TxConfQuery,
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

#[cfg(test)]
mod arb {
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for TxConfStatus {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let confs = arbitrary::any_tx_confs();
            let rp_txid = proptest::option::weighted(0.2, any::<LxTxid>());
            let is_dropped = proptest::bool::weighted(0.2);

            (confs, rp_txid, is_dropped)
                .prop_map(|(confs, rp_txid, is_dropped)| {
                    if confs == 0 {
                        TxConfStatus::ZeroConf
                    } else if let Some(rp_txid) = rp_txid {
                        TxConfStatus::HasReplacement { confs, rp_txid }
                    } else if is_dropped {
                        TxConfStatus::Dropped
                    } else {
                        TxConfStatus::InBestChain { confs }
                    }
                })
                .boxed()
        }
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    /// Check equivalence of our [`FeeEstimates::lookup_fee_rate`]
    /// implementation and [`esplora_client`]'s.
    #[test]
    fn convert_fee_rate_equiv() {
        proptest!(|(
            estimates in any::<BTreeMap<u16, f64>>(),
            target in any::<u16>(),
        )| {
            let hashmap_estimates = estimates
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect::<HashMap<u16, f64>>();
            let target_usize = usize::from(target);

            let our_feerate_res =
                FeeEstimates::lookup_fee_rate(target, &estimates) as f32;
            let their_feerate_res =
                esplora_client::convert_fee_rate(target_usize, hashmap_estimates)
                    .unwrap_or(FALLBACK_FEE_RATE as f32);

            prop_assert_eq!(our_feerate_res, their_feerate_res);
        })
    }
}
