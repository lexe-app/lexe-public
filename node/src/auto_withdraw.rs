use std::{sync::Arc, time::Duration};

use anyhow::Context;
use lexe_api::{
    models::command::{
        AutoWithdrawDestination, CloseChannelRequest, PayOnchainRequest,
        UserSettings,
    },
    types::{
        bounded_string::BoundedString,
        payments::{ClientPaymentId, PaymentKind},
    },
};
use lexe_common::ln::{
    amount::Amount, channel::LxChannelDetails, network::Network,
    priority::ConfirmationPriority,
};
use lexe_ln::{
    alias::FeeEstimatorType, balance, channel::ChannelEvent, command,
    payments::onchain::OnchainSendV2, tx_broadcaster::TxBroadcaster,
    wallet::OnchainWallet,
};
use lexe_std::fmt::DisplayIter;
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::chain::chaininterface::ConfirmationTarget;
use tracing::{debug, info, info_span, warn};

use crate::{
    alias::{ChainMonitorType, PaymentsManagerType},
    channel_manager::NodeChannelManager,
};

/// How long we wait after connecting to the LSP before the first check, so
/// that our channels are reestablished and become usable.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(2);

/// How often we check whether to withdraw.
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// We don't withdraw while coop closes cost more than this feerate.
// TODO(nicole): Let the user set this once we charge channel fees.
const MAX_CLOSE_FEERATE: bitcoin::FeeRate =
    bitcoin::FeeRate::from_sat_per_vb_unchecked(5);

/// Context required for auto-withdrawals.
pub(crate) struct AutoWithdrawCtx {
    pub chain_monitor: Arc<ChainMonitorType>,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub channel_manager: NodeChannelManager,
    pub fee_estimates: Arc<FeeEstimatorType>,
    pub network: Network,
    pub payments_manager: PaymentsManagerType,
    pub settings: Arc<tokio::sync::RwLock<UserSettings>>,
    pub tx_broadcaster: TxBroadcaster,
    pub wallet: OnchainWallet,
}

/// Spawns a task which periodically withdraws the user's funds per their
/// auto-withdrawal settings.
pub(crate) fn spawn_auto_withdraw_task(
    ctx: AutoWithdrawCtx,
    lsp_connected: NotifyOnce,
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
    const SPAN_NAME: &str = "(auto-withdraw)";
    LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
        // Withdrawals close LSP channels, so wait until we've synced and
        // connected to the LSP.
        tokio::select! {
            () = lsp_connected.recv_owned() => (),
            () = shutdown.recv() => return,
        }

        let first_check = tokio::time::Instant::now() + FIRST_CHECK_DELAY;
        let mut interval =
            tokio::time::interval_at(first_check, CHECK_INTERVAL);

        loop {
            tokio::select! {
                _ = interval.tick() => (),
                () = shutdown.recv() => break,
            }

            let try_withdraw = tokio::select! {
                res = ctx.withdraw_if_needed() => res,
                () = shutdown.recv() => break,
            };
            if let Err(e) = try_withdraw {
                warn!("Auto-withdrawal failed: {e:#}");
            }
        }

        info!("Auto-withdrawal task shutting down");
    })
}

impl AutoWithdrawCtx {
    /// If our total balance exceeds the auto-withdraw threshold, closes our
    /// channels, then sweeps our whole on-chain balance to the auto-withdraw
    /// destination.
    async fn withdraw_if_needed(&self) -> anyhow::Result<()> {
        let (maybe_threshold, maybe_destination) = {
            let locked_settings = self.settings.read().await;
            let maybe_threshold = locked_settings.auto_withdraw_threshold;
            let maybe_destination =
                locked_settings.auto_withdraw_destination.clone();
            (maybe_threshold, maybe_destination)
        };
        let threshold = match maybe_threshold {
            Some(threshold) => threshold,
            None => return Ok(()),
        };
        let destination = match maybe_destination {
            Some(AutoWithdrawDestination::Unknown) => {
                warn!("Unknown auto-withdraw destination kind; skipping");
                return Ok(());
            }
            None => {
                debug!("No auto-withdraw destination; skipping");
                return Ok(());
            }
            Some(dest) => dest,
        };

        // Skip attempt if channel close feerates are high
        let close_feerate = self
            .fee_estimates
            .conf_target_to_feerate(ConfirmationTarget::NonAnchorChannelFee);
        if close_feerate > MAX_CLOSE_FEERATE {
            debug!(%close_feerate, "Close feerate too high; skipping");
            return Ok(());
        }

        let channels = self
            .channel_manager
            .list_channels()
            .into_iter()
            .map(|channel| {
                let balance = balance::balance_from_channel(
                    &self.chain_monitor,
                    &channel,
                )?;
                let counterparty_alias = None;
                LxChannelDetails::from_ldk(channel, balance, counterparty_alias)
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .context("Failed to list channels")?;
        let lightning_balance =
            channels.iter().map(|c| c.our_balance).sum::<Amount>();
        // Only fails if the balance exceeds `Amount::MAX`, which is above any
        // threshold.
        let onchain_balance =
            Amount::try_from(self.wallet.get_balance().total())
                .unwrap_or(Amount::MAX);
        if lightning_balance.saturating_add(onchain_balance) <= threshold {
            return Ok(());
        }

        // Close our channels which hold funds. Only usable channels can be
        // cooperatively closed, so we skip the rest.
        let to_close = channels
            .iter()
            .filter(|c| c.is_usable && c.our_balance > Amount::ZERO);
        let mut funding_txos = Vec::new();
        for channel in to_close {
            let channel_id = channel.channel_id;
            let req = CloseChannelRequest {
                channel_id,
                force_close: false,
                maybe_counterparty: Some(channel.counterparty_node_id),
            };
            // A no-op, since `to_close` only includes usable channels,
            // whose counterparty is already connected.
            let ensure_counterparty_connected = |_| async { Ok(()) };
            let result = command::close_channel(
                &self.channel_manager,
                &self.channel_events_bus,
                ensure_counterparty_connected,
                req,
            )
            .await;

            match result {
                Ok(()) => {
                    info!(
                        %channel_id,
                        channel_value = %channel.channel_value,
                        our_balance = %channel.our_balance,
                        inbound_capacity = %channel.inbound_capacity,
                        "Closed channel for auto-withdrawal"
                    );
                    funding_txos.extend(
                        channel.funding_txo.map(bitcoin::OutPoint::from),
                    );
                }
                Err(e) => {
                    warn!(%channel_id, "Auto-withdrawal close failed: {e:#}")
                }
            }
        }

        // Wait for the close outputs to reach our wallet so the sweep includes
        // them.
        self.wait_for_close_txs(&funding_txos).await;
        if self.wallet.get_balance().total() == bitcoin::Amount::ZERO {
            return Ok(());
        }

        match destination {
            AutoWithdrawDestination::Address { address } => {
                let address = address
                    .require_network(self.network.to_bitcoin())
                    .context("Withdrawal address is for the wrong network")?;
                self.send_withdrawal_to_address(address).await
            }
            AutoWithdrawDestination::Unknown => unreachable!(
                "Unknown auto-withdraw destinations are rejected above"
            ),
        }
    }

    /// Waits, up to a timeout, for the close txs of the channels with these
    /// `funding_txos` to reach our wallet.
    async fn wait_for_close_txs(&self, funding_txos: &[bitcoin::OutPoint]) {
        const TIMEOUT: Duration = Duration::from_secs(30);
        const POLL_INTERVAL: Duration = Duration::from_secs(1);

        let deadline = tokio::time::Instant::now() + TIMEOUT;
        loop {
            let missing = funding_txos
                .iter()
                .copied()
                .filter(|txo| !self.wallet.has_seen_spend_of(*txo))
                .collect::<Vec<_>>();
            if missing.is_empty() {
                return;
            }
            if tokio::time::Instant::now() >= deadline {
                let funding_txos = DisplayIter(missing.iter());
                let timeout_secs = TIMEOUT.as_secs();
                warn!(
                    %funding_txos,
                    "Close txs didn't reach our wallet after {timeout_secs}s"
                );
                return;
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Sweeps our whole on-chain balance to `address` as a regular on-chain
    /// payment, so the withdrawal appears in our payment history.
    async fn send_withdrawal_to_address(
        &self,
        address: bitcoin::Address,
    ) -> anyhow::Result<()> {
        let priority = ConfirmationPriority::Normal;
        let (tx, fee) = self
            .wallet
            .create_sweep_tx(&address, priority)
            .context("Failed to create withdrawal tx")?;
        let amount = tx
            .output
            .iter()
            .map(|output| output.value)
            .sum::<bitcoin::Amount>();
        let amount =
            Amount::try_from(amount).context("Bad withdrawal amount")?;

        // TODO(nicole): Reuse `lexe_ln::command::pay_onchain` once it exposes
        // a wallet sweep (e.g. via `AmountOrAll`).
        let req = PayOnchainRequest {
            client_payment_id: ClientPaymentId::generate(),
            address: address.into_unchecked(),
            amount,
            priority,
            personal_note: BoundedString::new("Auto-withdrawal".to_owned())
                .ok(),
        };
        let client_pk = None;
        let kind = PaymentKind::AutoWithdrawOnchain;
        let oswm = OnchainSendV2::new(tx, req, kind, fee, client_pk)
            .context("Failed to create withdrawal payment")?;
        let tx = oswm.payment.tx.clone();
        let id = oswm.payment.id();
        let txid = oswm.payment.txid;

        // Register before broadcasting, so that a broadcasted tx always has a
        // payment record.
        self.payments_manager
            .new_payment(oswm.into_enum())
            .await
            .context("Could not register withdrawal")?;
        self.tx_broadcaster
            .broadcast_transaction(tx.as_ref().clone())
            .await
            .context("Failed to broadcast withdrawal")?;
        self.payments_manager
            .onchain_send_broadcasted(&id, &txid)
            .await
            .context("Could not register withdrawal broadcast")?;

        info!(%txid, %amount, %fee, "Sent auto-withdrawal");
        Ok(())
    }
}
