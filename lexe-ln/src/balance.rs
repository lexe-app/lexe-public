use anyhow::Context;
use common::{
    ln::{amount::Amount, balance::LightningBalance},
    Apply,
};
use lightning::ln::channel_state::ChannelDetails;
use tracing::{trace, warn};

use crate::{alias::LexeChainMonitorType, traits::LexePersister};

/// Computes our [`LightningBalance`] summed over all channels.
/// Also returns the number of channels marked as usable.
pub fn all_channel_balances<PS: LexePersister>(
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: &[ChannelDetails],
) -> (LightningBalance, usize) {
    let mut total_balance = LightningBalance::ZERO;
    let mut num_usable_channels: usize = 0;

    for channel in channels {
        let amount = match channel_balance(chain_monitor, channel) {
            Ok(amt) => amt,
            Err(e) => {
                warn!("Error getting channel balance: {e:#}");
                continue;
            }
        };

        if channel.is_usable {
            total_balance.usable += amount;
            num_usable_channels += 1;
        } else {
            total_balance.pending += amount;
        }
    }

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
