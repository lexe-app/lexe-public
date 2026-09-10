use std::cmp;

use anyhow::Context;
use lexe_api::cli::LspFees;
use lexe_common::{
    constants, dec,
    ln::{amount::Amount, balance::LightningBalance},
    ppm,
    ppm::Ppm,
};
use lexe_std::Apply;
use lightning::{
    chain::channelmonitor::{Balance, HolderCommitmentTransactionBalance},
    ln::channel_state::ChannelDetails,
    routing::router::DEFAULT_MAX_PATH_COUNT,
};
use tracing::warn;

use crate::{alias::LexeChainMonitorType, traits::LexePersister};

/// An estimate (in millionths) of the total proportional routing fees a Lexe
/// user will pay when making an outbound Lightning payment to an unspecified
/// receiver. Excludes the non-channel fee we add to our own first hop.
//
// Estimate a single-shard 5-hop route: User->LSP->3 nodes->payee.
//
// 2026-09: LSP external prop fee 750 ppm, network mean fee rate 817 ppm.
pub const EST_OUTBOUND_TOTAL_PROP_FEE: Ppm = ppm!(750 + 3 * 817);

/// An estimate of the total base fees a Lexe user will pay when making an
/// outbound Lightning payment over one shard of a MPP (or simply one path) to
/// an unspecified receiver.
// TODO(max): Base fees are low, but we should still get a better estimate.
// Phoenix charges 0.4% + 4 sat, which seems on the order of what we'd pay.
// 2026-09: LSP external base fee 0, network mean base fee 881 msat.
#[allow(clippy::identity_op)]
pub const EST_OUTBOUND_SHARD_BASE_FEE_SAT: u32 = 3 * 1;

/// Computes our [`LightningBalance`] summed over all channels.
/// Also returns the number of channels marked as usable.
pub fn all_channel_balances<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: &[ChannelDetails],
    lsp_fees: LspFees,
    caller_is_lsp: bool,
) -> (LightningBalance, usize) {
    let est_shard_base_fee =
        Amount::from_sats_u32(EST_OUTBOUND_SHARD_BASE_FEE_SAT);

    // TODO(nicole): We shouldn't be computing the mins separately otherwise our
    //               maximum is not guaranteed to be reachable. Also consider
    //               just hardcoding `lsp_usernode` fees instead of taking min.
    let min_lsp_prop_fee = cmp::min(
        lsp_fees.lsp_usernode_prop_fee,
        lsp_fees.lsp_external_prop_fee,
    );
    let min_lsp_base_fee = cmp::min(
        lsp_fees.lsp_usernode_base_fee,
        lsp_fees.lsp_external_base_fee,
    );

    let mut total_balance = LightningBalance::ZERO;
    let mut num_usable_channels = 0;
    let mut total_htlc_limit = Amount::ZERO;

    // The minimum number of shards we must use if we send `max_sendable`.
    // Equal to the number of usable channels with outbound HTLC capacity.
    let mut min_shards = 0;

    for channel in channels {
        let balance = match balance_from_channel(chain_monitor, channel) {
            Ok(bal) => bal,
            Err(e) => {
                warn!("Error getting channel balance: {e:#}");
                continue;
            }
        };

        if channel.is_usable {
            total_balance.usable += balance;
            total_htlc_limit +=
                Amount::from_msat(channel.next_outbound_htlc_limit_msat);
            num_usable_channels += 1;

            if channel.next_outbound_htlc_limit_msat > 0 {
                min_shards += 1;
            }
        } else {
            total_balance.pending += balance;
        }
    }

    // The most we can send once our own first hop fee is paid: the amount `A`
    // where `A + base_fee + A * prop_fee` exhausts the HTLC limits.
    let limit_after_first_hop_fee = if caller_is_lsp {
        // The LSP pays no first-hop fee.
        total_htlc_limit
    } else {
        let prop_fee = constants::DEFAULT_USERNODE_FIRST_HOP_PROP_FEE;
        let base_fee = constants::DEFAULT_USERNODE_FIRST_HOP_BASE_FEE;
        // A + A·p + b <= L  ===>  A <= (L - b) / (1 + p)
        let amount = {
            let numerator = total_htlc_limit.saturating_sub(base_fee);
            let denominator = dec!(1) + prop_fee.to_decimal();
            numerator / denominator
        };
        // `Amount` arithmetic rounds to a whole msat, so the division may
        // land up to 1 msat above the true bound, whichever direction it
        // rounds. Routing also scales each first hop's limit by
        // `A / (A + fee)`, truncating < 1 msat per channel. A margin of
        // 1 msat per shard covers both.
        let max_paths = u64::from(DEFAULT_MAX_PATH_COUNT);
        let truncation_margin =
            Amount::from_msat(cmp::min(min_shards, max_paths));
        amount.saturating_sub(truncation_margin)
    };

    // TODO(max): LDK appears to reapply the prop fee for each MPP shard. The
    // fee multiplier should be 1.
    // https://github.com/lightningdevkit/rust-lightning/issues/3675
    let prop_fee_multiplier = num_usable_channels;

    // Account for the estimated total channel fees
    let est_total_base_fee =
        Amount::from_msat(est_shard_base_fee.msat().saturating_mul(min_shards));
    total_balance.sendable = {
        let amount_without_base_fees =
            limit_after_first_hop_fee.saturating_sub(est_total_base_fee);
        ldk_max_final_value(
            amount_without_base_fees,
            EST_OUTBOUND_TOTAL_PROP_FEE,
            prop_fee_multiplier,
        )
    };

    // Account for the minimum LSP fees in a two-hop payment:
    // Sender -> LSP -> Receiver.
    let min_lsp_total_base_fee =
        Amount::from_msat(min_lsp_base_fee.msat().saturating_mul(min_shards));
    total_balance.max_sendable = {
        let amount_without_base_fees =
            limit_after_first_hop_fee.saturating_sub(min_lsp_total_base_fee);
        ldk_max_final_value(
            amount_without_base_fees,
            min_lsp_prop_fee,
            prop_fee_multiplier,
        )
    };

    (total_balance, num_usable_channels)
}

/// Mirrors LDK's integer calculation of a path's maximum final value after
/// proportional fees.
///
/// For effective proportional fee `p` ppm, LDK computes:
///
/// `floor((amount_msat * 1_000_000 + p) / (1_000_000 + p))`.
///
/// Here, `p = prop_fee * prop_fee_multiplier`. This is not equivalent to
/// simply dividing by `1 + p / 1_000_000` and rounding toward zero because LDK
/// adds `p` to the numerator before dividing.
///
/// <https://github.com/lightningdevkit/rust-lightning/pull/3755>
/// <https://github.com/lexe-app/rust-lightning/blob/2db1963bcc7bb29a8de3a49f60ec41b7b805cd03/lightning/src/routing/router.rs#L2427-L2460>
fn ldk_max_final_value(
    amount_without_base_fees: Amount,
    prop_fee: Ppm,
    prop_fee_multiplier: usize,
) -> Amount {
    const MILLION: u128 = 1_000_000;

    let prop_fee_multiplier = prop_fee_multiplier as u128;
    let effective_prop_fee =
        u128::from(prop_fee.to_u32()) * prop_fee_multiplier;
    let numerator = u128::from(amount_without_base_fees.msat()) * MILLION
        + effective_prop_fee;
    let max_msat = numerator / (MILLION + effective_prop_fee);
    let max_msat = u64::try_from(max_msat)
        .expect("Fee-adjusted amount cannot exceed its input");
    Amount::from_msat(max_msat)
}

/// Compute the contribution of a single channel to our "top-level" balance.
/// Also handles new channels that are pending open and thus don't have a
/// channel monitor yet.
///
/// For our top-level balance display, we want the value to behave intuitively,
/// without weird discontinuities around the dust threshold or weird channel
/// reserve accounting.
///
/// If I receive 100 sats, it should display 100 sats and not 0 sats, even if
/// it's technically not spendable. Otherwise a users receives a few sats, sees
/// _literally zero_ balance, and thinks we've lost their money. We'll rather
/// communicate what portion of their balance is spendable in a context
/// appropriate way.
///
/// The balance should also behave intuitively across transactions. If I receive
/// 50 sats then 5000 sats, it should display 50 sats then 5050 sats, not 0 sats
/// then 5050 sats. If I then spend 4900 sats, it should display 150 sats, not
/// 0 sats.
pub fn balance_from_channel<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    channel: &ChannelDetails,
) -> anyhow::Result<Amount> {
    let monitor = chain_monitor.get_monitor(channel.channel_id).ok();
    match monitor {
        Some(monitor) => {
            let amount_sats = monitor
                .get_claimable_balances()
                .into_iter()
                .map(balance_sats_from_channel_claimable_balance)
                .sum();
            Amount::try_from_sats_u64(amount_sats)
                .with_context(|| channel.channel_id)
        }
        None => {
            // No way to call `get_claimable_balances` for this channel.
            // Approximate our channel balance by summing our outbound
            // capacity + unspendable punishment reserve.
            let outbound_capacity =
                Amount::from_msat(channel.outbound_capacity_msat);
            let reserve_sat = channel
                .unspendable_punishment_reserve
                .unwrap_or(0)
                .apply(Amount::try_from_sats_u64)
                .with_context(|| channel.channel_id)?;
            Ok(outbound_capacity + reserve_sat)
        }
    }
}

/// Compute the contribution of a single channel to our "top-level" balance.
fn balance_sats_from_channel_claimable_balance(balance: Balance) -> u64 {
    match balance {
        Balance::ClaimableOnChannelClose {
            balance_candidates,
            confirmed_balance_candidate_index,
            outbound_payment_htlc_rounded_msat: _,
            outbound_forwarded_htlc_rounded_msat: _,
            inbound_claiming_htlc_rounded_msat: _,
            inbound_htlc_rounded_msat: _,
        } => {
            let idx = confirmed_balance_candidate_index;
            // Mirror LDK's `claimable_amount_satoshis` semantics for top-level
            // user balance: when no alternative funding tx has confirmed yet,
            // show the latest negotiated splice/RBF candidate instead of the
            // currently confirmed one. This keeps the user-visible balance
            // stable while a splice/RBF is pending.
            let maybe_b = if idx != 0 {
                Some(&balance_candidates[idx])
            } else {
                balance_candidates.last()
            };
            maybe_b
                .map(|b| {
                    let HolderCommitmentTransactionBalance {
                        // Our to-self commitment outputs
                        amount_satoshis,
                        // The commitment tx fee _we_ pay.
                        // outbound: `channel_value - ∑ outputs`
                        //  inbound: 0
                        transaction_fee_satoshis,
                        // ≈ our balance if it's lost to dust.
                        // Can only be non-zero for inbound.
                        our_inbound_dust_loss_satoshis,
                        // ≈ their balance if it's lost to dust.
                        // Can only be non-zero for outbound.
                        their_inbound_dust_loss_satoshis,
                    } = b;

                    (amount_satoshis
                        + transaction_fee_satoshis
                        + our_inbound_dust_loss_satoshis)
                        .saturating_sub(*their_inbound_dust_loss_satoshis)
                })
                .unwrap_or(0)
        }
        Balance::ClaimableAwaitingConfirmations {
            amount_satoshis, ..
        } => amount_satoshis,
        Balance::ContentiousClaimable {
            amount_satoshis, ..
        } => amount_satoshis,
        Balance::MaybeTimeoutClaimableHTLC {
            amount_satoshis, ..
        } => amount_satoshis,
        Balance::MaybePreimageClaimableHTLC {
            amount_satoshis, ..
        } => amount_satoshis,
        Balance::CounterpartyRevokedOutputClaimable {
            amount_satoshis, ..
        } => amount_satoshis,
        // TODO(phlip9): upstream has different logic for these variants.
        // Determine whether this behavior better matches our expectations.
        // Balance::MaybeTimeoutClaimableHTLC {
        //     amount_satoshis,
        //     outbound_payment,
        //     ..
        //  } => if *outbound_payment { 0 } else { *amount_satoshis },
        // Balance::MaybePreimageClaimableHTLC { .. } => 0,
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ldk_max_final_value_matches_router() {
        // Worked example:
        //
        // ```
        // $ python
        // >>> (limit, ppm, num_usable_channels) = (100_000, 0.3 * 10_000, 1)
        // >>> p = ppm * num_usable_channels
        // >>> value_dec = (limit * 1_000_000 + p) / (1_000_000 + p)
        // >>> value_dec
        // 99700.9002991027
        // >>> int(value_dec)
        // 99700
        // ```
        let limit = Amount::from_msat(100_000);
        let prop_fee = ppm!(0.3%);
        assert_eq!(
            ldk_max_final_value(limit, prop_fee, 1),
            Amount::from_msat(99_700),
        );

        // Example from a failing smoketest that exercises the exact rounding
        let limit = Amount::from_msat(999_936_000);
        assert_eq!(
            ldk_max_final_value(limit, prop_fee, 1),
            Amount::from_msat(996_945_164)
        );
        assert_eq!(
            ldk_max_final_value(limit, prop_fee, 2),
            Amount::from_msat(993_972_167)
        );
        assert_eq!(ldk_max_final_value(limit, Ppm::ZERO, 1), limit);
    }
}
