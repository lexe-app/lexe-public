//! The recovery flow: close every channel and drive every satoshi of channel
//! funds into the BDK wallet.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, anyhow};
use lexe_api::models::command::CloseChannelRequest;
use lexe_common::{
    api::user::NodePk,
    ln::{amount::Amount, channel::ChannelId},
};
use lexe_ln::{
    background_processor::{self, QuiescenceRequest},
    channel::ChannelEvent,
    command,
    esplora::{FeeEstimates, LexeEsplora},
    p2p,
    persister::LightningPersisterMethods,
    sync::BdkSyncRequest,
    wallet::OnchainWallet,
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::{
    chain::channelmonitor::Balance, ln::channel_state::ChannelDetails,
    util::wallet_utils::WalletSource,
};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::{info, warn};

use crate::{
    alias::{ChainMonitorType, ChannelManagerType},
    coin_selector::{
        DepositAsk, DepositEstimate, DepositEstimator, UnderfundedClaims,
    },
    peer_manager::RecoveryPeerManager,
    persister::RecoveryPersister,
    types::{AnchorFundsNeeded, CloseMode, RecoveryProgress, RecoveryReport},
};

/// The delay before the first progress poll. See `drive_until_settled`.
const FIRST_POLL_DELAY: Duration = Duration::from_secs(5);
/// The most a poll waits for BGP quiescence before proceeding without it.
/// Needs to cover one BGP event-processing turn, which is bounded by a few
/// sequential esplora broadcasts (fee-bumps) on a slow public instance.
const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Everything the recovery loop operates on. Assembled by
/// [`RecoveryNode::init`] and owned by the recovery task.
///
/// [`RecoveryNode::init`]: crate::node::RecoveryNode::init
pub(crate) struct RecoveryCtx {
    pub bdk_resync_tx: mpsc::Sender<BdkSyncRequest>,
    pub bgp_control_tx: mpsc::Sender<QuiescenceRequest>,
    pub chain_monitor: Arc<ChainMonitorType>,
    pub channel_events_bus: EventsBus<ChannelEvent>,
    pub channel_manager: Arc<ChannelManagerType>,
    pub close_mode: CloseMode,
    pub eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    pub esplora: Arc<LexeEsplora>,
    pub ldk_resync_tx: mpsc::Sender<oneshot::Sender<()>>,
    pub peer_manager: RecoveryPeerManager,
    pub persister: Arc<RecoveryPersister>,
    pub progress_tx: Option<watch::Sender<Option<RecoveryProgress>>>,
    pub shutdown: NotifyOnce,
    pub stall_detector: StallDetector,
    pub sync_interval: Duration,
    pub wallet: OnchainWallet,
}

impl RecoveryCtx {
    /// Closes all channels and drives every channel satoshi into the BDK
    /// wallet.
    pub(crate) async fn recover(&mut self) -> anyhow::Result<RecoveryReport> {
        let num_monitors = self.chain_monitor.list_monitors().len();
        let (coop_closed, force_closed) = self.close_all_channels().await?;
        let confirmed_balance = self.drive_until_settled().await?;
        Ok(RecoveryReport {
            coop_closed,
            force_closed,
            num_monitors,
            onchain_balance: confirmed_balance,
        })
    }

    /// Closes all channels per [`CloseMode`], returning how many were coop
    /// closed and force closed.
    async fn close_all_channels(&mut self) -> anyhow::Result<(usize, usize)> {
        let channels = self.channel_manager.list_channels();
        let num_channels = channels.len();
        info!(%num_channels, "Closing all channels");
        if channels.is_empty() {
            return Ok((0, 0));
        }

        let can_force_close = self.close_mode.can_force_close();
        let lsp_connected = match self.close_mode.lsp() {
            None => {
                info!("Force closing all channels");
                false
            }
            Some(lsp) => {
                let try_connect = p2p::connect_peer_if_necessary(
                    &self.peer_manager,
                    &lsp.node_pk,
                    &lsp.addrs,
                )
                .await;
                match try_connect {
                    Ok(maybe_task) => {
                        if let Some(task) = maybe_task.0 {
                            let _ = self.eph_tasks_tx.try_send(task);
                        }
                        info!("Connected to LSP; will try cooperative closes");
                        true
                    }
                    Err(e) if !can_force_close =>
                        return Err(e).context("Couldn't connect to the LSP"),
                    Err(e) => {
                        warn!(
                            "Couldn't connect to LSP; force closing all \
                             channels: {e:#}"
                        );
                        false
                    }
                }
            }
        };

        // Every channel is about to be force closed, so first make sure the
        // wallet can fee-bump them all. A shortfall found mid-close would
        // leave commitment txs stuck unconfirmed instead of channels open.
        if !lsp_connected {
            self.await_anchor_funds().await?;
        }

        let mut coop_closed = 0;
        let mut force_closed = 0;
        for channel in channels {
            // Each close can block until `close_channel`'s event timeout, so
            // check for shutdown between channels.
            if self.shutdown.try_recv() {
                return Err(anyhow!("Interrupted; re-run to resume"));
            }

            let channel_id = ChannelId::from(channel.channel_id);
            let counterparty = NodePk(channel.counterparty.node_id);
            // We connected to the LSP above, and force closes don't need the
            // peer, so `ensure_counterparty_connected` is a no-op.
            let close = |force_close: bool| {
                command::close_channel(
                    &self.channel_manager,
                    &self.channel_events_bus,
                    |_cp: NodePk| async { Ok(()) },
                    CloseChannelRequest {
                        channel_id,
                        force_close,
                        maybe_counterparty: Some(counterparty),
                    },
                )
            };

            if lsp_connected {
                let force_close = false;
                match close(force_close).await {
                    Ok(()) => {
                        // TODO(max): LDK's `convert_channel_err_internal`
                        // logs coop closes at ERROR as "Closed channel due to
                        // close-required error: Coop Closed". Upstream a fix
                        // which logs them at a lower level.
                        info!(%channel_id, "Cooperatively closed channel");
                        coop_closed += 1;
                        continue;
                    }
                    Err(e) if !can_force_close =>
                        return Err(e).with_context(|| {
                            format!("Couldn't coop close {channel_id}")
                        }),
                    Err(e) => warn!(
                        %channel_id,
                        "Coop close failed; falling back to force close: {e:#}"
                    ),
                }
            }

            let force_close = true;
            match close(force_close).await {
                Ok(()) => {
                    info!(%channel_id, "Force closed channel");
                    force_closed += 1;
                }
                // The channel may already be closing, and the monitor
                // recovers the funds regardless, so keep going.
                Err(e) => warn!(%channel_id, "Force close failed: {e:#}"),
            }
        }

        Ok((coop_closed, force_closed))
    }

    /// Waits until the wallet's confirmed funds can fee-bump every open
    /// channel's commitment tx, publishing the deposit requirement meanwhile.
    /// The node keeps syncing while it waits, so the monitors can still
    /// contest a stale commitment tx from the counterparty.
    async fn await_anchor_funds(&mut self) -> anyhow::Result<()> {
        loop {
            let channels = self.channel_manager.list_channels();
            let mut progress = self.measure_progress().await?;
            self.stall_detector
                .check_before_close(&mut progress, &channels)
                .await?;
            self.publish_progress(&progress);
            if progress.awaiting_anchor_funds.is_none() {
                return Ok(());
            }

            tokio::select! {
                () = self.shutdown.recv() =>
                    return Err(anyhow!("Interrupted; re-run to resume")),
                () = tokio::time::sleep(self.sync_interval) => (),
            }
            self.resync().await?;
        }
    }

    /// Polls the chain until all channel funds have settled into the BDK
    /// wallet, publishing progress along the way. Returns the confirmed
    /// balance.
    async fn drive_until_settled(&mut self) -> anyhow::Result<Amount> {
        // The sync interval may be long, but a stall should be reported
        // promptly.
        let mut poll_delay = FIRST_POLL_DELAY.min(self.sync_interval);

        loop {
            tokio::select! {
                () = self.shutdown.recv() =>
                    return Err(anyhow!("Interrupted; re-run to resume")),
                () = tokio::time::sleep(poll_delay) => (),
            }
            poll_delay = self.sync_interval;

            self.resync().await?;

            // The resync may have just emitted a `SpendableOutputs` event
            // into LDK's in-memory queue. Its funds are then in neither the
            // monitor balances, the persisted events, nor the wallet, so
            // measuring now could see `is_settled()` with funds in flight.
            // Thus, wait for the BGP to handle all pending events first.
            // Quiescence also waits out pending HTLCs, which only a channel
            // close can resolve, so bound the wait.
            let quiesce_fut =
                background_processor::wait_quiescent(&self.bgp_control_tx);
            #[allow(clippy::needless_late_init)] // Clearer this way IMO
            let is_quiescent;
            match tokio::time::timeout(QUIESCENCE_TIMEOUT, quiesce_fut).await {
                Ok(result) => {
                    result.context("Error while waiting for BGP quiescence")?;
                    is_quiescent = true;
                }
                Err(_timeout) => {
                    warn!("BGP not quiescent after {QUIESCENCE_TIMEOUT:?}");
                    is_quiescent = false;
                }
            };

            let mut progress = self.measure_progress().await?;
            self.stall_detector.check_for_stall(&mut progress)?;

            // Publish the snapshot for progress displays (e.g. the CLI's
            // per-block countdown line).
            self.publish_progress(&progress);

            if is_quiescent && progress.is_settled() {
                return Ok(progress.confirmed_balance);
            }
        }
    }

    /// Publishes a progress snapshot to `progress_tx`, if a subscriber
    /// exists and the snapshot changed.
    fn publish_progress(&self, progress: &RecoveryProgress) {
        if let Some(progress_tx) = &self.progress_tx {
            progress_tx.send_if_modified(|latest| {
                let modified = latest.as_ref() != Some(progress);
                if modified {
                    *latest = Some(progress.clone());
                }
                modified
            });
        }
    }

    /// Takes a [`RecoveryProgress`] snapshot and logs it.
    async fn measure_progress(&mut self) -> anyhow::Result<RecoveryProgress> {
        let num_channels = self.channel_manager.list_channels().len();

        // Channel monitors with unswept/unconfirmed balances.
        let mut num_unresolved_monitors = 0;
        let mut claimable_sats = 0u64;
        let mut unlock_height = None;
        for channel_id in self.chain_monitor.list_monitors() {
            let Ok(monitor) = self.chain_monitor.get_monitor(channel_id) else {
                continue;
            };
            let balances = monitor.get_claimable_balances();
            if !balances.is_empty() {
                num_unresolved_monitors += 1;
                claimable_sats += balances
                    .iter()
                    .map(|balance| balance.claimable_amount_satoshis())
                    .sum::<u64>();
                // The height by which every timelocked balance will have
                // unlocked.
                unlock_height = balances
                    .iter()
                    .filter_map(|balance| match balance {
                        Balance::ClaimableAwaitingConfirmations {
                            confirmation_height,
                            ..
                        } => Some(*confirmation_height),
                        _ => None,
                    })
                    .chain(unlock_height)
                    .max();
            }
        }

        let num_unhandled_events = self
            .persister
            .read_events()
            .await
            .context("Failed to read unhandled events")?
            .len();

        let wallet_balance = self.wallet.get_balance();
        let pending_balance =
            wallet_balance.trusted_pending + wallet_balance.untrusted_pending;
        let best_height = self.channel_manager.current_best_block().height;

        // Money first, then timing, then debug details.
        info!(
            confirmed_sats = %wallet_balance.confirmed.to_sat(),
            pending_sats = %pending_balance.to_sat(),
            %claimable_sats, ?unlock_height, %best_height,
            %num_channels, %num_unresolved_monitors, %num_unhandled_events,
            "Recovery progress"
        );

        let confirmed_balance = Amount::try_from(wallet_balance.confirmed)
            .context("Bad confirmed balance")?;

        Ok(RecoveryProgress {
            // Filled in by [`StallDetector::check_for_stall`].
            awaiting_anchor_funds: None,
            best_height,
            claimable_balance: Amount::try_from_sats_u64(claimable_sats)
                .context("Bad claimable balance")?,
            confirmed_balance,
            num_channels,
            num_unhandled_events,
            num_unresolved_monitors,
            pending_balance: Amount::try_from(pending_balance)
                .context("Bad pending balance")?,
            unlock_height,
        })
    }

    /// Resyncs BDK, then LDK, waiting for each to complete.
    async fn resync(&self) -> anyhow::Result<()> {
        // BDK goes first so that the fee-bump attempts LDK makes on connecting
        // a block select from a wallet which has seen that block too, e.g. a
        // deposit which just confirmed. A failed attempt would escalate the
        // next one's feerate and publish a stale deposit requirement.
        let (bdk_tx, bdk_rx) = oneshot::channel();
        self.bdk_resync_tx
            .send(BdkSyncRequest {
                full_sync: false,
                tx: bdk_tx,
            })
            .await
            .context("BDK sync task died")?;
        bdk_rx.await.context("BDK sync task dropped request")?;

        let (ldk_tx, ldk_rx) = oneshot::channel();
        self.ldk_resync_tx
            .send(ldk_tx)
            .await
            .context("LDK sync task died")?;
        ldk_rx.await.context("LDK sync task dropped request")?;
        Ok(())
    }
}

/// Detects when the recovery is stalled because force close fee-bumps can't
/// gather enough confirmed on-chain funds, and computes the deposit
/// requirement to publish.
///
/// LDK retries each claim's fee-bump on every block, so the recovery is
/// stalled while any claim's latest attempt has failed.
//
// NOTE: On each new block, LDK regenerates pending claims with
// `FeerateStrategy::ForceBump`. When the fee estimate hasn't risen, this
// raises the previous attempt's package feerate by 25%, up to 5x the
// estimate. This RBF escalation is meant for stuck txs, but LDK applies it
// even when coin selection failed and nothing was broadcast. Thus, while
// stalled, the `DepositEstimate`s and the published `needed_deposit` climb
// every block at an unchanged fee estimate.
//
// TODO(max): Report this behavior upstream to rust-lightning.
pub(crate) struct StallDetector {
    fee_estimates: Arc<FeeEstimates>,
    outstanding_requirement: Option<AnchorFundsNeeded>,
    underfunded_claims: UnderfundedClaims,
    wallet: OnchainWallet,
}

impl StallDetector {
    pub(crate) fn new(
        fee_estimates: Arc<FeeEstimates>,
        underfunded_claims: UnderfundedClaims,
        wallet: OnchainWallet,
    ) -> Self {
        Self {
            fee_estimates,
            outstanding_requirement: None,
            underfunded_claims,
            wallet,
        }
    }

    /// Sets `progress.awaiting_anchor_funds` to the deposit requirement for
    /// force closing all of `channels`, if the wallet can't fund their
    /// fee-bumps. Checked before closing, so that a shortfall leaves the
    /// channels open rather than their commitment txs stuck unconfirmed.
    async fn check_before_close(
        &mut self,
        progress: &mut RecoveryProgress,
        channels: &[ChannelDetails],
    ) -> anyhow::Result<()> {
        let wallet_utxos = self
            .wallet
            .list_confirmed_utxos()
            .await
            .map_err(|()| anyhow!("Failed to list confirmed UTXOs"))?;
        let estimator =
            DepositEstimator::new(&self.fee_estimates, &wallet_utxos);
        let asks = channels
            .iter()
            .filter_map(|channel| estimator.channel_close_ask(channel))
            .collect::<Vec<_>>();
        self.update_requirement(progress, &asks)
    }

    /// Sets `progress.awaiting_anchor_funds` to the outstanding deposit
    /// requirement if the recovery is stalled on fee-bump coin selection.
    fn check_for_stall(
        &mut self,
        progress: &mut RecoveryProgress,
    ) -> anyhow::Result<()> {
        let mut underfunded_claims = self.underfunded_claims.lock().unwrap();
        // A claim can resolve without a final successful coin selection,
        // e.g. if an earlier bump confirms first, leaving a stale entry.
        // Every pending claim backs an unresolved monitor, so once none
        // remain, all entries are stale.
        if progress.num_unresolved_monitors == 0 {
            underfunded_claims.clear();
        }
        let asks = underfunded_claims.values().copied().collect::<Vec<_>>();
        drop(underfunded_claims);
        self.update_requirement(progress, &asks)
    }

    /// Aggregates per-claim asks into one deposit requirement and sets
    /// `progress.awaiting_anchor_funds` to it, if a deposit is needed.
    fn update_requirement(
        &mut self,
        progress: &mut RecoveryProgress,
        asks: &[DepositAsk],
    ) -> anyhow::Result<()> {
        /// The wallet needs the sum of the claims' `total_sats`, minus what it
        /// can already contribute. Each claim's `needed_sats` subtracts that
        /// contribution too, so summing them would subtract it once per claim.
        /// Instead, subtract the largest per-claim `total_sats - needed_sats`
        /// from the summed totals.
        fn aggregate_needed_sats(
            estimates: impl Iterator<Item = DepositEstimate>,
        ) -> u64 {
            let (total_sats, credit_sats) =
                estimates.fold((0, 0), |(total, credit), estimate| {
                    let total = total + estimate.total_sats;
                    let credit =
                        credit.max(estimate.total_sats - estimate.needed_sats);
                    (total, credit)
                });
            total_sats.saturating_sub(credit_sats)
        }

        let confirmed_balance = progress.confirmed_balance;

        let needed_sats =
            aggregate_needed_sats(asks.iter().map(|ask| ask.needed));
        if needed_sats == 0 {
            self.outstanding_requirement = None;
            return Ok(());
        }
        let suggested_sats =
            aggregate_needed_sats(asks.iter().map(|ask| ask.suggested));

        // `needed_sats` climbs every block while stalled (see the NOTE on
        // [`StallDetector`]), so renewing every poll would churn the
        // published requirement. Instead, renew it only once `needed_sats`
        // outgrows the last suggested deposit, i.e. the fee estimate rose, or
        // when the confirmed balance changes while still short, e.g. after
        // an insufficient deposit.
        let renew = match &self.outstanding_requirement {
            None => true,
            Some(outstanding_requirement) =>
                needed_sats
                    > outstanding_requirement.suggested_deposit.sats_u64()
                    || confirmed_balance
                        != outstanding_requirement.confirmed_balance,
        };
        if renew {
            let deposit_address = self.wallet.get_next_unused_address();
            let needed_deposit = Amount::try_from_sats_u64(needed_sats)
                .context("Bad needed_sats")?;
            let suggested_deposit = Amount::try_from_sats_u64(suggested_sats)
                .context("Bad suggested_sats")?;
            warn!(
                %deposit_address, %confirmed_balance, %needed_deposit,
                %suggested_deposit,
                "Force close needs confirmed on-chain funds for fee-bumping; \
                 deposit some funds to proceed"
            );
            self.outstanding_requirement = Some(AnchorFundsNeeded {
                confirmed_balance,
                deposit_address,
                needed_deposit,
                suggested_deposit,
            });
        }
        progress.awaiting_anchor_funds = self.outstanding_requirement.clone();
        Ok(())
    }
}

