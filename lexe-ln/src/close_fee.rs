//! Helpers for estimating channel close fees in e.g.
//! [`crate::command::close_channel`].

use lexe_common::constants;
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        chainmonitor::LockedChannelMonitor,
    },
    ln::channel_state::ChannelDetails,
};

use crate::{alias::SignerType, esplora::FeeEstimates};

/// Calculate the fees _we_ have to pay to close this channel.
///
/// We're aiming to exactly match the fee that LDK actually negotiates
/// during coop close. The negotiation flow is:
///
/// 1. Funder proposes `(min_fee, max_fee)` range via
///    `FundedChannel::calculate_closing_fee_limits` (channel.rs).
/// 2. Non-funder picks `max_fee` from that range (channel.rs `closing_signed`,
///    "They have to pay, so pick the highest fee in the overlapping range").
/// 3. Funder accepts since it's within their own range.
///
/// So the negotiated fee = funder's `max_fee` =
///   `normal_feerate * close_tx_weight / 1000 + avoidance_fee`.
///
/// On top of this, a 0-or-1 sat msat rounding loss may apply (see
/// [`msat_rounding_loss_sats`]).
///
/// Key LDK sources to keep in sync with:
/// - `channel.rs`:
///   + `calculate_closing_fee_limits`: fee range computation
///   + `get_closing_transaction_weight`: close tx weight
///   + `build_closing_transaction`: output value truncate
///     (`floor(value_to_self_msat / 1000)`) that causes the msat rounding
/// - `channelmonitor.rs`:
///   + `get_claimable_balances`: `BalanceCandidate` computation
///     (`transaction_fee_satoshis = channel_value - sum_i outputs_i`)
pub(crate) fn our_close_tx_fees_sats(
    fee_estimates: &FeeEstimates,
    channel: &ChannelDetails,
    monitor: LockedChannelMonitor<'_, SignerType>,
) -> u64 {
    use lightning::chain::channelmonitor::Balance;

    // Extract our balance and the commitment tx fee from the monitor.
    //
    // `our_sats = amount_satoshis + transaction_fee_satoshis`, where:
    // - `amount_satoshis` = our output on the commitment tx (LDK:
    //   `to_broadcaster_value_sat`)
    // - `transaction_fee_satoshis` = commitment tx fee (incl. rounding loss)
    //   (LDK: `channel_value - sum_i commitment_tx.outputs[i]`)
    //
    // Unlike LDK's `Balance::claimable_amount_satoshis`, this intentionally
    // uses the currently confirmed/on-chain-valid candidate. During coop
    // close, LDK negotiates and signs against the currently locked funding
    // (`self.funding` in channel.rs), not the latest pending splice/RBF
    // candidate.
    let mut our_sats: u64 = 0;
    let mut commit_tx_fee_sats: u64 = 0;
    for b in monitor.get_claimable_balances() {
        match b {
            Balance::ClaimableOnChannelClose {
                balance_candidates,
                confirmed_balance_candidate_index,
                outbound_payment_htlc_rounded_msat: _,
                outbound_forwarded_htlc_rounded_msat: _,
                inbound_claiming_htlc_rounded_msat: _,
                inbound_htlc_rounded_msat: _,
            } => {
                let maybe_b =
                    balance_candidates.get(confirmed_balance_candidate_index);
                if let Some(b) = maybe_b {
                    our_sats += b.amount_satoshis + b.transaction_fee_satoshis;
                    commit_tx_fee_sats += b.transaction_fee_satoshis;
                }
            }
            Balance::ClaimableAwaitingConfirmations { .. }
            | Balance::ContentiousClaimable { .. }
            | Balance::MaybeTimeoutClaimableHTLC { .. }
            | Balance::MaybePreimageClaimableHTLC { .. }
            | Balance::CounterpartyRevokedOutputClaimable { .. } => {}
        }
    }
    if our_sats == 0 {
        return 0;
    };

    // We only pay for the on-chain channel close fees if we're the channel
    // funder.
    //
    // For our purposes, if we're not the funder and our output is also
    // beneath our dust limit, we'll just consider our remaining
    // channel balance as part of the close fee.
    //
    // TODO(phlip9): channel.is_outbound will no longer be an accurate proxy
    // for whether we have to pay the close fees once we move to splices.
    if !channel.is_outbound {
        let fee_sats = if our_sats <= constants::LDK_DUST_LIMIT_SATS.into() {
            our_sats
        } else {
            0
        };
        return fee_sats;
    }

    // The current fees required for this close tx to confirm
    let tx_fees_sats = close_tx_fees_sats(fee_estimates, channel);

    // Account for msat-to-sat rounding loss in the closing tx.
    //
    // When `value_to_self_msat` has a sub-sat remainder (isn't a
    // multiple of 1000), both sides' closing tx outputs are truncated,
    // causing 1 extra sat to go to miners.
    //
    // Our LN balance (`amount_satoshis + transaction_fee_satoshis`)
    // includes this rounding sat (via `transaction_fee_satoshis =
    // commit_fee + msat_round`), but the closing tx output to us is
    // computed from `floor(value_to_self_msat / 1000)` which
    // doesn't include it.
    //
    // To detect this, we compare `transaction_fee_satoshis` with the
    // expected base commitment fee. Any excess (capped at 1) is the
    // msat rounding loss.
    let msat_rounding_loss_sats =
        msat_rounding_loss_sats(channel, commit_tx_fee_sats);
    let tx_fees_sats = tx_fees_sats + msat_rounding_loss_sats;

    // As the funder, if we somehow don't have enough to pay the full
    // `tx_fees_sats`, then the most we can possibly pay (without RBF /
    // anchors) is our current balance. Most likely the remote will
    // force close. Usually the channel reserve should prevent this
    // case from happening, i.e, we should have enough balance to
    // pay the on-chain fees.
    if our_sats <= tx_fees_sats {
        // TODO(phlip9): we'll probably get force closed. So use that fee
        // estimate instead.
        return our_sats;
    }

    // If, after paying the fees, our output would be smaller than our dust
    // limit, then we just donate our sats to the miners.
    let our_sats = our_sats - tx_fees_sats;
    if our_sats <= constants::LDK_DUST_LIMIT_SATS.into() {
        return tx_fees_sats + our_sats;
    }

    // Normally, we just pay the fees
    tx_fees_sats
}

/// Estimate the total on-chain fees for a channel close, which must be paid
/// by the channel funder.
///
/// Mirrors the funder's `max_fee` from LDK's `calculate_closing_fee_limits`
/// (channel.rs):
///   `normal_feerate * tx_weight / 1000 + avoidance_fee`
///
/// The non-funder always picks the funder's `max_fee` (see `closing_signed`
/// in channel.rs, "They have to pay, so pick the highest fee").
fn close_tx_fees_sats(
    fee_estimates: &FeeEstimates,
    channel: &ChannelDetails,
) -> u64 {
    // LDK uses `NonAnchorChannelFee` for the `normal_feerate` in
    // `calculate_closing_fee_limits`.
    let conf_target = ConfirmationTarget::NonAnchorChannelFee;
    let fee_sat_per_kwu =
        fee_estimates.get_est_sat_per_1000_weight(conf_target) as u64;

    // Must match LDK's `get_closing_transaction_weight` (channel.rs).
    let close_tx_weight = CLOSE_TX_WEIGHT;
    let normal_fee_sats =
        fee_sat_per_kwu.saturating_mul(close_tx_weight) / 1000;

    // LDK always adds `force_close_avoidance_max_fee_satoshis` to the
    // funder's max fee (see `calculate_closing_fee_limits`).
    let force_close_avoidance_max_fee_sats = channel
        .config
        .map(|c| c.force_close_avoidance_max_fee_satoshis)
        .unwrap_or(constants::FORCE_CLOSE_AVOIDANCE_MAX_FEE_SATS);

    normal_fee_sats.saturating_add(force_close_avoidance_max_fee_sats)
}

/// Compute msat-to-sat rounding loss for the channel funder.
///
/// When LN payments leave a sub-sat msat remainder on
/// `value_to_self_msat`, both sides' closing tx outputs truncate to whole
/// sats, causing 1 extra sat to go to miners. Our LN balance includes
/// this extra sat (via `transaction_fee_satoshis`), but the closing output
/// doesn't, so our fee estimate needs to account for it.
fn msat_rounding_loss_sats(
    channel: &ChannelDetails,
    commit_tx_fee_sats: u64,
) -> u64 {
    // The non-anchor commitment tx base weight (0 HTLCs).
    // See: `COMMITMENT_TX_BASE_WEIGHT` in chan_utils.rs
    //      `commitment_tx_base_weight`.
    //
    // TODO(phlip9): also support anchor channels if/when we use them
    //   (base weight = 1124).
    const COMMITMENT_TX_BASE_WEIGHT: u64 = 724;

    let commit_feerate =
        channel.feerate_sat_per_1000_weight.unwrap_or(0) as u64;
    let expected_base_commit_fee =
        commit_feerate * COMMITMENT_TX_BASE_WEIGHT / 1000;

    // msat rounding loss is at most 1 sat
    commit_tx_fee_sats
        .saturating_sub(expected_base_commit_fee)
        .min(1)
}

/// Between User and LSP, the close tx is currently predictable.
///
/// LDK (currently) always over-estimates the close tx cost by one output if
/// one side's balance (after fees) is below their dust limit.
pub(crate) const CLOSE_TX_WEIGHT: u64 = close_tx_weight(
    // funding_redeemscript:
    // [ OP_PUSHNUM_2 <a-pubkey> <b-pubkey> OP_PUSHNUM_2 OP_CHECKMULTISIG ]
    71,
    //
    // a/b_scriptpubkey:
    // [ OP_0 OP_PUSHBYTES_20 <20-bytes> ]
    22, 22,
);

/// Calculate the tx weight for a potential channel close.
///
/// Vendored from LDK's `get_closing_transaction_weight` (channel.rs).
/// Must be kept in sync when LDK changes the close tx format (e.g.,
/// splicing may add inputs/outputs).
const fn close_tx_weight(
    funding_redeemscript_len: u64,
    a_scriptpubkey_len: u64,
    b_scriptpubkey_len: u64,
) -> u64 {
    (4 +                                        // version
         1 +                                    // input count
         36 +                                   // prevout
         1 +                                    // script length (0)
         4 +                                    // sequence
         1 +                                    // output count
         4                                      // lock time
         )*4 +                                  // * 4 for non-witness parts
        2 +                                     // witness marker and flag
        1 +                                     // witness element count
        4 +                                     // 4 element lengths (2 sigs, multisig dummy, and witness script)
        funding_redeemscript_len +              // funding witness script
        2*(1 + 71) +                            // two signatures + sighash type flags
        (((8+1) +                               // output values and script length
            a_scriptpubkey_len) * 4) +          // scriptpubkey and witness multiplier
        (((8+1) +                               // output values and script length
            b_scriptpubkey_len) * 4) //         // scriptpubkey and witn multiplier
}

#[cfg(test)]
mod test {
    use bitcoin::{
        key::PublicKey,
        opcodes,
        script::{self, ScriptBuf},
        secp256k1,
    };
    use lexe_common::secp256k1_ctx::SECP256K1;

    use super::*;

    fn pubkey() -> PublicKey {
        let secret_key = secp256k1::SecretKey::from_slice(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ])
        .unwrap();
        PublicKey::new(secp256k1::PublicKey::from_secret_key(
            &*SECP256K1,
            &secret_key,
        ))
    }

    // [ OP_PUSHNUM_2 <a-pubkey> <b-pubkey> OP_PUSHNUM_2 OP_CHECKMULTISIG ]
    fn redeem_script() -> ScriptBuf {
        let pubkey = pubkey();
        script::Builder::new()
            .push_opcode(opcodes::all::OP_PUSHNUM_2)
            .push_key(&pubkey)
            .push_key(&pubkey)
            .push_opcode(opcodes::all::OP_PUSHNUM_2)
            .push_opcode(opcodes::all::OP_CHECKMULTISIG)
            .into_script()
    }

    // [ OP_0 OP_PUSHBYTES_20 <20-bytes> ]
    fn output_script() -> ScriptBuf {
        ScriptBuf::from_bytes(vec![0x69; 22])
    }

    #[test]
    fn check_close_tx_weight_constant() {
        let redeem_script = redeem_script();
        let output_script = output_script();
        let close_wu = close_tx_weight(
            redeem_script.len() as u64,
            output_script.len() as u64,
            output_script.len() as u64,
        );
        assert_eq!(close_wu, CLOSE_TX_WEIGHT);
    }
}
