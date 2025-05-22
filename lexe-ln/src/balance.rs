use std::cmp;

use anyhow::Context;
use common::{
    cli::LspFees,
    ln::{amount::Amount, balance::LightningBalance},
};
use lexe_std::Apply;
use lightning::ln::channel_state::ChannelDetails;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tracing::{trace, warn};

use crate::{alias::LexeChainMonitorType, traits::LexePersister};

/// An estimate (in millionths) of the total proportional fees a Lexe user will
/// pay when making an outbound Lightning payment to an unspecified receiver.
pub const EST_OUTBOUND_TOTAL_PROP_FEE_PPM: u32 = 5000;

/// An estimate of the total base fees a Lexe user will pay when making an
/// outbound Lightning payment over one shart of a MPP (or simply one path) to
/// an unspecified receiver.
// TODO(max): Base fees are low, but we should still get a better estimate.
// Phoenix charges 0.4% + 4 sat, which seems on the order of what we'd pay.
pub const EST_OUTBOUND_SHARD_BASE_FEE_SAT: u32 = 5;

/// Computes our [`LightningBalance`] summed over all channels.
/// Also returns the number of channels marked as usable.
pub fn all_channel_balances<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: &[ChannelDetails],
    lsp_fees: LspFees,
) -> (LightningBalance, usize) {
    let est_total_prop_feerate =
        Decimal::from(EST_OUTBOUND_TOTAL_PROP_FEE_PPM) / dec!(1_000_000);
    let est_shard_base_fee =
        Amount::from_sats_u32(EST_OUTBOUND_SHARD_BASE_FEE_SAT);

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

    for channel in channels {
        let balance = match channel_balance(chain_monitor, channel) {
            Ok(bal) => bal,
            Err(e) => {
                warn!("Error getting channel balance: {e:#}");
                continue;
            }
        };

        if channel.is_usable {
            let next_outbound_htlc_limit =
                Amount::from_msat(channel.next_outbound_htlc_limit_msat);

            // Both of these have a one sat tweak to account for a floor in
            // LDK's calculation of `compute_max_final_value_contribution` for
            // paths. Otherwise `smoketest::payments::max_sendable_multihop`
            // fails with "Tried to pay `x` sats. The max you can route to this
            // recipient is `y` sats."
            //   `x` = 986500.499, `y` = 986499 (`y` from `max_flow` is floored)
            // https://github.com/lightningdevkit/rust-lightning/pull/3755
            let sendable =
                next_outbound_htlc_limit
                    .saturating_sub(est_shard_base_fee)
                    .saturating_sub(Amount::from_sats_u32(1));
            let max_sendable =
                next_outbound_htlc_limit
                    .saturating_sub(min_lsp_base_fee)
                    .saturating_sub(Amount::from_sats_u32(1));

            total_balance.usable += balance;
            total_balance.sendable += sendable;
            total_balance.max_sendable += max_sendable;
            num_usable_channels += 1;
        } else {
            total_balance.pending += balance;
        }
    }

    let num_usable_channels_dec = Decimal::from(num_usable_channels);

    // Tweak sendable to account for the estimated total proportional fee.
    // sendable + sendable * prop_fee = sum(next_outbound_htlc_limit - base_fee)
    // sendable * (1 + prop_fee) = sum(next_outbound_htlc_limit - base_fee)
    // sendable = sum(next_outbound_htlc_limit - base_fee) / (1 + prop_fee)
    total_balance.sendable = total_balance
        .sendable
        .checked_div(dec!(1) + num_usable_channels_dec * est_total_prop_feerate)
        // TODO(max): LDK appears to reapply the prop fee for each MPP shard
        // when it should be `.checked_div(dec!(1) + est_total_prop_feerate)`
        // https://github.com/lightningdevkit/rust-lightning/issues/3675
        .expect("Can't overflow because divisor is > 1");

    total_balance.max_sendable = total_balance
        .max_sendable
        // Tweak max_sendable to account for the minimum LSP prop fee that would
        // be paid in the case of a two hop payment: Sender -> LSP -> Receiver.
        //
        // max_sendable =
        //     sum(next_outbound_htlc_limit - base_fee) / (1 + prop_fee)
        //
        // TODO(max): LDK appears to reapply the prop fee for each MPP shard
        // when it should be `.checked_div(dec!(1) + min_lsp_prop_fee)`.
        // https://github.com/lightningdevkit/rust-lightning/issues/3675
        .checked_div(dec!(1) + num_usable_channels_dec * min_lsp_prop_fee)
        .expect("Can't overflow because divisor is > 1");

    (total_balance, num_usable_channels)
}

/// Get our claimable channel balance for a given channel.
pub fn channel_balance<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    channel: &ChannelDetails,
) -> anyhow::Result<Amount> {
    use lightning::chain::channelmonitor::Balance;

    let monitor = channel
        .funding_txo
        .and_then(|txo| chain_monitor.get_monitor(txo).ok());
    match monitor {
        Some(monitor) => {
            let amount_sats = monitor
                .get_claimable_balances()
                .into_iter()
                .map(|b| {
                    trace!("ln_balance: {b:?}");
                    match b {
                        Balance::ClaimableOnChannelClose {
                            amount_satoshis,
                            transaction_fee_satoshis,
                            outbound_payment_htlc_rounded_msat: _,
                            outbound_forwarded_htlc_rounded_msat: _,
                            inbound_claiming_htlc_rounded_msat: _,
                            inbound_htlc_rounded_msat: _,
                        } => {
                            // Add back in the "reserved"
                            // `transaction_fee_satoshis` to make things more
                            // intuitive, i.e., open 10_000 sat channel, get
                            // 10_000 sats balance.
                            amount_satoshis + transaction_fee_satoshis
                        }
                        Balance::ClaimableAwaitingConfirmations {
                            amount_satoshis,
                            ..
                        } => amount_satoshis,
                        Balance::ContentiousClaimable {
                            amount_satoshis,
                            ..
                        } => amount_satoshis,
                        Balance::MaybeTimeoutClaimableHTLC {
                            amount_satoshis,
                            ..
                        } => amount_satoshis,
                        Balance::MaybePreimageClaimableHTLC {
                            amount_satoshis,
                            ..
                        } => amount_satoshis,
                        Balance::CounterpartyRevokedOutputClaimable {
                            amount_satoshis,
                            ..
                        } => amount_satoshis,
                    }
                })
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
