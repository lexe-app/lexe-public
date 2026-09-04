//! Coin selection for fee-bump (anchor CPFP) txs, observing failures.
//!
//! Each connected block and each BGP rebroadcast tick makes the chain monitor
//! re-emit `Event::BumpTransaction` for every pending claim. The event handler
//! delegates these to LDK's `BumpTransactionEventHandler`, which selects UTXOs
//! via [`RecoveryCoinSelector`], then signs and broadcasts the bump tx. Each
//! selection attempt updates the shared [`UnderfundedClaims`], which the
//! [`StallDetector`] reads each poll to detect a stalled exit and publish its
//! deposit requirement.
//!
//! [`RecoveryCoinSelector`]: crate::coin_selector::RecoveryCoinSelector
//! [`StallDetector`]: crate::recover::StallDetector
//! [`UnderfundedClaims`]: crate::coin_selector::UnderfundedClaims

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex},
};

use bitcoin::{
    Amount, OutPoint, Psbt, ScriptBuf, Transaction, TxOut, WScriptHash,
    hashes::Hash as _,
};
use lexe_ln::{
    esplora::FeeEstimates, logger::LexeTracingLogger, wallet::OnchainWallet,
};
use lexe_tokio::notify_once::NotifyOnce;
use lightning::{
    chain::{ClaimId, chaininterface::ConfirmationTarget},
    ln::{
        chan_utils::{ANCHOR_INPUT_WITNESS_WEIGHT, EMPTY_WITNESS_WEIGHT},
        channel_state::ChannelDetails,
    },
    util::wallet_utils::{
        self, CoinSelection, CoinSelectionSource, Input, Utxo, WalletSource,
    },
};
/// LDK's [`wallet_utils::Wallet`]. Despite its name, it is a coin selector,
/// not a wallet. It selects confirmed UTXOs from our [`OnchainWallet`]'s
/// [`WalletSource`] impl to fund fee-bump txs, signs the resulting PSBTs, and
/// locks each claim's selected UTXOs so concurrent claims don't conflict.
//
// NOTE: These locks are in-memory and invisible to BDK, so a concurrently
// built BDK tx could double-spend a selection that hasn't been broadcast
// yet. However, `TxBroadcaster` feeds every broadcast straight back into
// BDK's tx graph, and the external sweep, our only BDK-built tx, comes
// after all claims have resolved.
pub(crate) type LdkCoinSelector =
    wallet_utils::Wallet<Arc<OnchainWallet>, LexeTracingLogger>;

/// The claims whose latest fee-bump coin selection failed, each with its
/// [`DepositAsk`]. Shared between the [`RecoveryCoinSelector`], which
/// records every attempt, and the [`StallDetector`], which reads it.
///
/// [`StallDetector`]: crate::recover::StallDetector
pub(crate) type UnderfundedClaims =
    Arc<Mutex<HashMap<Option<ClaimId>, DepositAsk>>>;

/// A failed coin selection's estimates of the deposit which would let it
/// succeed.
#[derive(Copy, Clone)]
pub(crate) struct DepositAsk {
    /// At the attempt's own feerate, i.e. the minimum.
    pub needed: DepositEstimate,
    /// At LDK's fee-bump cap (see [`DepositEstimator::bump_cap_feerate`]),
    /// i.e. enough to keep succeeding as LDK bumps, unless the fee estimate
    /// itself rises.
    pub suggested: DepositEstimate,
}

/// What a coin selection needs at one feerate.
#[derive(Copy, Clone)]
pub(crate) struct DepositEstimate {
    /// The confirmed sats the selection needs, as if the wallet held nothing.
    pub total_sats: u64,
    /// `total_sats` minus what the wallet can already contribute, i.e. the
    /// minimum deposit.
    pub needed_sats: u64,
}

/// The [`CoinSelectionSource`] for the recovery node's fee-bump handler.
///
/// Selection spends only confirmed UTXOs and skips UTXOs worth little more
/// than the fee to spend them, so the wallet balance alone can't tell whether
/// selection will succeed. Instead, this delegates to the [`LdkCoinSelector`]
/// and records each attempt in [`UnderfundedClaims`], so the recovery loop can
/// tell the user when the exit is stalled on missing funds.
pub(crate) struct RecoveryCoinSelector {
    fee_estimates: Arc<FeeEstimates>,
    ldk_coin_selector: LdkCoinSelector,
    wallet: Arc<OnchainWallet>,
    /// Signaled once the wallet has completed its first chain sync.
    wallet_synced: NotifyOnce,
    underfunded_claims: UnderfundedClaims,
}

impl RecoveryCoinSelector {
    pub(crate) fn new(
        wallet: OnchainWallet,
        logger: LexeTracingLogger,
        fee_estimates: Arc<FeeEstimates>,
        wallet_synced: NotifyOnce,
        underfunded_claims: UnderfundedClaims,
    ) -> Self {
        let wallet = Arc::new(wallet);
        Self {
            fee_estimates,
            ldk_coin_selector: LdkCoinSelector::new(wallet.clone(), logger),
            wallet,
            wallet_synced,
            underfunded_claims,
        }
    }
}

impl CoinSelectionSource for RecoveryCoinSelector {
    async fn select_confirmed_utxos<'a>(
        &'a self,
        claim_id: Option<ClaimId>,
        must_spend: Vec<Input>,
        must_pay_to: &'a [TxOut],
        target_feerate_sat_per_1000_weight: u32,
        max_tx_weight: u64,
    ) -> Result<CoinSelection, ()> {
        // The BGP re-emits every pending claim's `BumpTransaction` at startup,
        // before the wallet's first sync. Selecting from the unsynced wallet
        // would fail spuriously, and LDK escalates the feerate after every
        // failed attempt, so wait for the sync instead.
        self.wallet_synced.clone().recv_owned().await;

        // The same UTXOs selection will see, to estimate a deposit on failure.
        let wallet_utxos =
            self.wallet.list_confirmed_utxos().await.unwrap_or_default();
        let ask = DepositEstimator::new(&self.fee_estimates, &wallet_utxos)
            .ask(&must_spend, must_pay_to, target_feerate_sat_per_1000_weight);
        let result = self
            .ldk_coin_selector
            .select_confirmed_utxos(
                claim_id,
                must_spend,
                must_pay_to,
                target_feerate_sat_per_1000_weight,
                max_tx_weight,
            )
            .await;
        let mut underfunded_claims = self.underfunded_claims.lock().unwrap();
        match &result {
            // Selection succeeded; this claim's fee-bump tx is funded.
            Ok(_) => underfunded_claims.remove(&claim_id),
            // Selection failed; the wallet doesn't have enough confirmed funds.
            Err(()) => underfunded_claims.insert(claim_id, ask),
        };
        result
    }

    fn sign_psbt<'a>(
        &'a self,
        psbt: Psbt,
    ) -> impl Future<Output = Result<Transaction, ()>> + Send + 'a {
        self.ldk_coin_selector.sign_psbt(psbt)
    }
}

/// Estimates deposits from the wallet's confirmed UTXOs, as coin selection
/// sees them.
pub(crate) struct DepositEstimator<'a> {
    /// LDK's target for a close with pending HTLCs. HTLC-free closes use a
    /// slower one, so this over-asks for them.
    urgent_feerate_sat_per_1000_weight: u32,
    wallet_utxos: &'a [Utxo],
}

impl<'a> DepositEstimator<'a> {
    pub(crate) fn new(
        fee_estimates: &FeeEstimates,
        wallet_utxos: &'a [Utxo],
    ) -> Self {
        let urgent_feerate_sat_per_1000_weight = fee_estimates
            .conf_target_to_feerate(ConfirmationTarget::UrgentOnChainSweep)
            .to_sat_per_kwu()
            as u32;
        Self {
            urgent_feerate_sat_per_1000_weight,
            wallet_utxos,
        }
    }

    /// The [`DepositAsk`] for one coin selection attempt.
    pub(crate) fn ask(
        &self,
        must_spend: &[Input],
        must_pay_to: &[TxOut],
        target_feerate_sat_per_1000_weight: u32,
    ) -> DepositAsk {
        let cap_feerate_sat_per_1000_weight =
            self.bump_cap_feerate(target_feerate_sat_per_1000_weight);
        let estimate = |feerate| {
            helpers::estimate_needed_deposit(
                self.wallet_utxos,
                must_spend,
                must_pay_to,
                feerate,
            )
        };
        DepositAsk {
            needed: estimate(target_feerate_sat_per_1000_weight),
            suggested: estimate(cap_feerate_sat_per_1000_weight),
        }
    }

    /// The [`DepositAsk`] for fee-bumping `channel`'s commitment tx if it
    /// were force closed now, or `None` if no fee-bump would be needed.
    ///
    /// Synthesizes the anchor CPFP input as LDK's `BumpTransactionEventHandler`
    /// does when handling `Event::BumpTransaction`. The commitment weight
    /// counts every pending HTLC, including dust HTLCs which the tx omits, so
    /// this over-approximates.
    pub(crate) fn channel_close_ask(
        &self,
        channel: &ChannelDetails,
    ) -> Option<DepositAsk> {
        // Mirroring LDK's pub(crate) constants:
        // - `chan_utils::COMMITMENT_TX_BASE_ANCHOR_WEIGHT`, the heaviest base
        //   weight across anchor channel types.
        const COMMITMENT_TX_BASE_ANCHOR_WEIGHT: u64 = 1124;
        // - `chan_utils::COMMITMENT_TX_WEIGHT_PER_HTLC`
        const COMMITMENT_TX_WEIGHT_PER_HTLC: u64 = 172;
        // - `channel::ANCHOR_OUTPUT_VALUE_SATOSHI`
        const ANCHOR_OUTPUT_VALUE_SATOSHI: u64 = 330;

        // Only anchor channels CPFP; other commitment txs pay their own fee.
        // Mirrors LDK's `PackageTemplate::requires_external_funding`.
        let features = channel.channel_type.as_ref()?;
        let (anchor_witness_weight, anchor_value_sats) =
            if features.supports_anchor_zero_fee_commitments() {
                // The P2A anchor spends with an empty witness and usually
                // carries no value (at most the trimmed dust).
                (EMPTY_WITNESS_WEIGHT, 0)
            } else if features.supports_anchors_zero_fee_htlc_tx() {
                (ANCHOR_INPUT_WITNESS_WEIGHT, ANCHOR_OUTPUT_VALUE_SATOSHI)
            } else {
                return None;
            };
        // LDK broadcasts the commitment tx alone if its own feerate suffices.
        let target_feerate = self.urgent_feerate_sat_per_1000_weight;
        let commitment_feerate =
            channel.feerate_sat_per_1000_weight.unwrap_or(0);
        if commitment_feerate >= target_feerate {
            return None;
        }

        let num_htlcs = channel.pending_inbound_htlcs.len()
            + channel.pending_outbound_htlcs.len();
        let commitment_weight = COMMITMENT_TX_BASE_ANCHOR_WEIGHT
            + COMMITMENT_TX_WEIGHT_PER_HTLC * num_htlcs as u64;
        // Mirrors LDK's `chan_utils::commit_tx_fee_sat`.
        let commitment_fee_sats =
            u64::from(commitment_feerate) * commitment_weight / 1000;
        let must_spend = [helpers::anchor_cpfp_input(
            OutPoint::null(), // Unused by the estimate
            commitment_weight,
            anchor_witness_weight,
            anchor_value_sats + commitment_fee_sats,
        )];
        Some(self.ask(&must_spend, &[], target_feerate))
    }

    /// The most LDK's `FeerateStrategy::ForceBump` escalates a claim to while
    /// the fee estimate stays flat: 5x the estimate, or the attempt's feerate
    /// if already higher.
    fn bump_cap_feerate(
        &self,
        attempt_feerate_sat_per_1000_weight: u32,
    ) -> u32 {
        let cap = self.urgent_feerate_sat_per_1000_weight.saturating_mul(5);
        attempt_feerate_sat_per_1000_weight.max(cap)
    }
}

mod helpers {
    use super::*;

    /// Estimates the deposit a coin selection needs to succeed, assuming the
    /// user deposits one new UTXO and the tx takes back one change output.
    ///
    /// LDK folds the weight and fee of the tx being CPFP'd into its anchor
    /// input's `satisfaction_weight` and `previous_utxo.value`, so the
    /// `must_spend` inputs fully determine the required package fee.
    ///
    /// The estimate includes the deposit UTXO's own spend fee, and its weights
    /// assume the heaviest common types, so it over-approximates the required
    /// deposit.
    ///
    /// Mirrors LDK's selection math: the tx weight assembly, the per-UTXO fees
    /// and eligibility, and the `selected_amount < target_amount_sat +
    /// total_fees` failure condition. See:
    /// <https://github.com/lightningdevkit/rust-lightning/blob/090e09f3992694040d3a55c1c798b3a92fb77182/lightning/src/util/wallet_utils.rs#L674-L697>
    /// <https://github.com/lightningdevkit/rust-lightning/blob/090e09f3992694040d3a55c1c798b3a92fb77182/lightning/src/util/wallet_utils.rs#L544-L556>
    /// <https://github.com/lightningdevkit/rust-lightning/blob/090e09f3992694040d3a55c1c798b3a92fb77182/lightning/src/util/wallet_utils.rs#L604-L612>
    pub(super) fn estimate_needed_deposit(
        wallet_utxos: &[Utxo],
        must_spend: &[Input],
        must_pay_to: &[TxOut],
        target_feerate_sat_per_1000_weight: u32,
    ) -> DepositEstimate {
        // Weights (WU) mirroring LDK's pub(crate) constants used in the linked
        // file:
        // - Tx overhead: `4 * BASE_TX_SIZE + SEGWIT_MARKER_FLAG_WEIGHT`.
        const TX_BASE_WEIGHT: u64 = 42;
        // - Input overhead excluding script_sig: `BASE_INPUT_WEIGHT`. The empty
        //   script_sig len byte lives in each input's satisfaction weight
        //   instead.
        const BASE_INPUT_WEIGHT: u64 = 160;
        // - One wallet-contributed input: `EMPTY_SCRIPT_SIG_WEIGHT +
        //   P2WPKH_WITNESS_WEIGHT` satisfaction, as LDK's wallet assumes for
        //   its own UTXOs.
        const WALLET_INPUT_WEIGHT: u64 = BASE_INPUT_WEIGHT + 4 + 108;
        // - One change output: `P2WSH_TXOUT_WEIGHT`, the heaviest standard
        //   type.
        const CHANGE_TXOUT_WEIGHT: u64 = 172;

        let fee_sats = |weight: u64| {
            (u64::from(target_feerate_sat_per_1000_weight) * weight)
                .div_ceil(1000)
        };

        // The weight of the tx excluding wallet contributions, i.e. LDK's
        // `preexisting_tx_weight`. LDK computes the per-UTXO fees separately,
        // so ceil in the same split to never round below its total.
        let must_spend_weight = must_spend
            .iter()
            .map(|input| BASE_INPUT_WEIGHT + input.satisfaction_weight)
            .sum::<u64>();
        let must_pay_weight = must_pay_to
            .iter()
            .map(|txout| 4 * (8 + 1 + txout.script_pubkey.len() as u64))
            .sum::<u64>();
        let preexisting_weight =
            TX_BASE_WEIGHT + must_spend_weight + must_pay_weight;
        let fee = fee_sats(preexisting_weight)
            + fee_sats(WALLET_INPUT_WEIGHT + CHANGE_TXOUT_WEIGHT);

        // Even LDK's most lenient retry only spends UTXOs worth more than their
        // own spend fee, so credit each wallet UTXO's value net of that fee,
        // floored at 0.
        let credit_sats = wallet_utxos
            .iter()
            .map(|utxo| {
                let fee_to_spend =
                    fee_sats(BASE_INPUT_WEIGHT + utxo.satisfaction_weight);
                utxo.output.value.to_sat().saturating_sub(fee_to_spend)
            })
            .sum::<u64>();

        let must_spend_sats = must_spend
            .iter()
            .map(|input| input.previous_utxo.value.to_sat())
            .sum::<u64>();
        let must_pay_sats = must_pay_to
            .iter()
            .map(|txout| txout.value.to_sat())
            .sum::<u64>();
        let total_sats = (fee + must_pay_sats).saturating_sub(must_spend_sats);
        let needed_sats = total_sats.saturating_sub(credit_sats);
        DepositEstimate {
            total_sats,
            needed_sats,
        }
    }

    /// The anchor CPFP `must_spend` input, with the commitment tx's weight and
    /// fee folded in as LDK does (see [`estimate_needed_deposit`]).
    pub(super) fn anchor_cpfp_input(
        outpoint: OutPoint,
        commitment_weight: u64,
        anchor_witness_weight: u64,
        utxo_sats: u64,
    ) -> Input {
        Input {
            outpoint,
            previous_utxo: TxOut {
                value: Amount::from_sat(utxo_sats),
                script_pubkey: ScriptBuf::new_p2wsh(&WScriptHash::all_zeros()),
            },
            // The anchor spend witness, plus the empty script_sig's 1-byte
            // length prefix, which is a non-witness byte (4 WU).
            satisfaction_weight: commitment_weight + 4 + anchor_witness_weight,
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use bitcoin::{
        Amount, OutPoint, ScriptBuf, Transaction, Txid, WPubkeyHash,
        WScriptHash, absolute::LockTime, hashes::Hash as _,
        transaction::Version,
    };

    use super::*;

    /// A generous max tx weight cap; not what's under test.
    const MAX_TX_WEIGHT: u64 = 400_000;

    /// Checks [`helpers::estimate_needed_deposit`] against LDK's real coin
    /// selection: depositing a single UTXO of the estimated value must always
    /// fund the claim, and the ask must not exceed LDK's true minimum deposit
    /// by more than our deliberate over-approximations.
    #[test]
    fn estimate_needed_deposit_matches_ldk() {
        // (must_spend, must_pay_to) shapes.
        let claims = [
            // Anchor CPFP for a small commitment tx.
            (vec![anchor_input(724, 330, 0)], vec![]),
            // Anchor CPFP for a large commitment tx.
            (vec![anchor_input(3_000, 330, 0)], vec![]),
            // Zero-value anchor output.
            (vec![anchor_input(1_100, 0, 0)], vec![]),
            // Two anchors bumped in one claim.
            (
                vec![anchor_input(1_100, 330, 0), anchor_input(1_500, 330, 1)],
                vec![],
            ),
            // An output to fund, beyond the input value.
            (
                vec![anchor_input(1_500, 330, 0)],
                vec![p2wpkh_txout(10_000)],
            ),
        ];
        // Existing confirmed wallet UTXOs, to be credited.
        let wallets: [&[u64]; 4] = [
            &[],
            // Worth little or nothing net of their spend fee.
            &[100, 200],
            // Usable, but usually insufficient.
            &[5_000],
            // A mix.
            &[30_000, 150],
        ];
        let feerates = [253u32, 1_000, 5_000, 25_000, 100_000];

        for (must_spend, must_pay_to) in &claims {
            for existing in wallets {
                for feerate in feerates {
                    let existing_utxos = FakeWalletSource::new(existing).utxos;
                    let needed = helpers::estimate_needed_deposit(
                        &existing_utxos,
                        must_spend,
                        must_pay_to,
                        feerate,
                    )
                    .needed_sats;

                    // A deposit of exactly `needed` must be sufficient.
                    assert!(
                        ldk_selection_succeeds(
                            existing,
                            needed,
                            must_spend,
                            must_pay_to,
                            feerate
                        ),
                        "Deposit ask {needed} insufficient at feerate \
                         {feerate} with wallet {existing:?}: \
                         {must_spend:?} -> {must_pay_to:?}"
                    );

                    // Success is monotone in the deposit's value, so binary
                    // search for LDK's true minimum deposit.
                    let (mut lo, mut hi) = (0, needed);
                    while lo < hi {
                        let mid = (lo + hi) / 2;
                        if ldk_selection_succeeds(
                            existing,
                            mid,
                            must_spend,
                            must_pay_to,
                            feerate,
                        ) {
                            hi = mid;
                        } else {
                            lo = mid + 1;
                        }
                    }
                    let true_min = lo;

                    // The ask may only exceed LDK's true minimum by the fee
                    // for our deliberate padding, the deposit input and
                    // change output weights (444 WU), plus rounding slack.
                    let slack = (u64::from(feerate) * 444).div_ceil(1000) + 2;
                    assert!(
                        needed - true_min <= slack,
                        "Deposit ask {needed} exceeds LDK minimum \
                         {true_min} by more than {slack} at feerate \
                         {feerate} with wallet {existing:?}: {must_spend:?}"
                    );
                }
            }
        }
    }

    /// Whether LDK's real coin selection succeeds when the wallet holds
    /// confirmed P2WPKH UTXOs of the `existing` values plus a deposit of
    /// `deposit_sats`, or no deposit if 0.
    fn ldk_selection_succeeds(
        existing: &[u64],
        deposit_sats: u64,
        must_spend: &[Input],
        must_pay_to: &[TxOut],
        target_feerate_sat_per_1000_weight: u32,
    ) -> bool {
        let mut wallet_sats = existing.to_vec();
        if deposit_sats > 0 {
            wallet_sats.push(deposit_sats);
        }
        // LDK's selector locks UTXOs per claim, so use a fresh one per probe.
        let ldk = wallet_utils::Wallet::new(
            Arc::new(FakeWalletSource::new(&wallet_sats)),
            LexeTracingLogger::new(),
        );
        futures::executor::block_on(ldk.select_confirmed_utxos(
            None,
            must_spend.to_vec(),
            must_pay_to,
            target_feerate_sat_per_1000_weight,
            MAX_TX_WEIGHT,
        ))
        .is_ok()
    }

    /// A keyed-anchor CPFP `must_spend` input at a distinct `vout`.
    fn anchor_input(
        commitment_weight: u64,
        anchor_sats: u64,
        vout: u32,
    ) -> Input {
        let outpoint = OutPoint {
            txid: Txid::all_zeros(),
            vout,
        };
        helpers::anchor_cpfp_input(
            outpoint,
            commitment_weight,
            ANCHOR_INPUT_WITNESS_WEIGHT,
            anchor_sats,
        )
    }

    fn p2wpkh_txout(value_sats: u64) -> TxOut {
        TxOut {
            value: Amount::from_sat(value_sats),
            script_pubkey: ScriptBuf::new_p2wpkh(
                &WPubkeyHash::from_byte_array([0x69; 20]),
            ),
        }
    }

    /// A [`WalletSource`] over a fixed set of confirmed P2WPKH UTXOs,
    /// matching the UTXO type and satisfaction weight our BIP84
    /// [`OnchainWallet`] reports.
    struct FakeWalletSource {
        utxos: Vec<Utxo>,
        prevtxs: HashMap<Txid, Transaction>,
    }

    impl FakeWalletSource {
        /// A wallet holding one confirmed P2WPKH UTXO per value in `sats`.
        fn new(sats: &[u64]) -> Self {
            let wpkh = WPubkeyHash::from_byte_array([0x42; 20]);
            let prevtx = Transaction {
                version: Version::TWO,
                lock_time: LockTime::ZERO,
                input: Vec::new(),
                output: sats
                    .iter()
                    .map(|&value_sats| TxOut {
                        value: Amount::from_sat(value_sats),
                        script_pubkey: ScriptBuf::new_p2wpkh(&wpkh),
                    })
                    .collect(),
            };
            let txid = prevtx.compute_txid();
            let utxos = sats
                .iter()
                .enumerate()
                .map(|(vout, &value_sats)| {
                    Utxo::new_v0_p2wpkh(
                        OutPoint {
                            txid,
                            vout: vout as u32,
                        },
                        Amount::from_sat(value_sats),
                        &wpkh,
                    )
                })
                .collect();
            Self {
                utxos,
                prevtxs: HashMap::from([(txid, prevtx)]),
            }
        }
    }

    #[allow(clippy::manual_async_fn)] // Lint suggestion breaks compilation
    impl WalletSource for FakeWalletSource {
        fn list_confirmed_utxos(
            &self,
        ) -> impl Future<Output = Result<Vec<Utxo>, ()>> + Send + '_ {
            async move { Ok(self.utxos.clone()) }
        }

        fn get_prevtx(
            &self,
            outpoint: OutPoint,
        ) -> impl Future<Output = Result<Transaction, ()>> + Send + '_ {
            async move { self.prevtxs.get(&outpoint.txid).cloned().ok_or(()) }
        }

        fn get_change_script(
            &self,
        ) -> impl Future<Output = Result<ScriptBuf, ()>> + Send + '_ {
            async move { Ok(ScriptBuf::new_p2wsh(&WScriptHash::all_zeros())) }
        }

        fn sign_psbt(
            &self,
            _psbt: Psbt,
        ) -> impl Future<Output = Result<Transaction, ()>> + Send + '_ {
            async move { unimplemented!("Selection never signs") }
        }
    }
}
