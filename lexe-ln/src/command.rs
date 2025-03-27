use std::{
    cmp::{max, min},
    convert::Infallible,
    time::Duration,
};

use anyhow::{anyhow, bail, ensure, Context};
use bitcoin_hashes::{sha256, Hash};
use common::{
    api::{
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            CreateOfferRequest, CreateOfferResponse, ListChannelsResponse,
            NodeInfo, OpenChannelResponse, PayInvoiceRequest,
            PayInvoiceResponse, PayOnchainRequest, PayOnchainResponse,
            PreflightCloseChannelRequest, PreflightCloseChannelResponse,
            PreflightOpenChannelRequest, PreflightOpenChannelResponse,
            PreflightPayInvoiceRequest, PreflightPayInvoiceResponse,
            PreflightPayOnchainRequest, PreflightPayOnchainResponse,
        },
        user::{NodePk, Scid},
        Empty,
    },
    cli::{LspFees, LspInfo},
    constants, debug_panic_release_log,
    enclave::Measurement,
    ln::{
        amount::Amount,
        channel::{LxChannelDetails, LxChannelId, LxUserChannelId},
        invoice::LxInvoice,
        network::LxNetwork,
        offer::LxOffer,
    },
    time::TimestampMs,
    Apply,
};
use either::Either;
use futures::Future;
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        chainmonitor::LockedChannelMonitor,
    },
    ln::{
        channel_state::ChannelDetails,
        channelmanager::{
            PaymentId, RecipientOnionFields, RetryableSendFailure,
            MIN_FINAL_CLTV_EXPIRY_DELTA,
        },
        types::ChannelId,
    },
    routing::{
        gossip::NodeId,
        router::{RouteHint, RouteParameters},
    },
    sign::{NodeSigner, Recipient},
    types::payment::PaymentHash,
    util::config::UserConfig,
};
use lightning_invoice::{
    Bolt11Invoice, Currency, InvoiceBuilder, RouteHintHop, RoutingFees,
};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, instrument};

use crate::{
    alias::{LexeChainMonitorType, NetworkGraphType, RouterType, SignerType},
    balance,
    channel::{ChannelEvent, ChannelEventsBus, ChannelEventsRx},
    esplora::FeeEstimates,
    keys_manager::LexeKeysManager,
    payments::{
        inbound::InboundInvoicePayment,
        manager::PaymentsManager,
        outbound::{
            LxOutboundPaymentFailure, OutboundInvoicePayment,
            OUTBOUND_PAYMENT_RETRY_STRATEGY,
        },
        Payment,
    },
    route::{self, RoutingContext},
    traits::{LexeChannelManager, LexePeerManager, LexePersister},
    tx_broadcaster::TxBroadcaster,
    wallet::LexeWallet,
};

/// The max # of route hints containing intercept scids we'll add to invoices.
// NOTE: We previously had issues failing to route Lexe -> Lexe MPPs with only
// one route hint because LDK's routing algorithm includes a hack which disables
// the central hop in any found routes for subsequent MPP iterations, which
// happens to be the LSP -> Payee hop in a two hop path. A lot of work was done
// to migrate to multiple SCIDs per user, but it turns out we can just comment
// out the hack in LDK to fix the Lexe -> Lexe MPP routing issue. Removing the
// hack should also make Lexe user -> External MPPs more reliable as well, as
// multiple shards can use the same (reliable) path, instead of being forced to
// diversify to longer, higher cost, less liquid paths.
//
// Issue: https://github.com/lightningdevkit/rust-lightning/issues/3685
pub const MAX_INTERCEPT_HINTS: usize = 1;

/// Specifies whether it is the user node or the LSP calling the
/// [`create_invoice`] fn. There are some differences between how the user node
/// and LSP generate invoices which this tiny enum makes clearer.
#[derive(Clone)]
pub enum CreateInvoiceCaller {
    /// When a user node calls [`create_invoice`], it must provide an
    /// [`LspInfo`], which is required for generating a [`RouteHintHop`] for
    /// receiving a payment (possibly while offline, or over a JIT channel)
    /// routed to us by the LSP.
    ///
    /// [`RouteHintHop`]: lightning::routing::router::RouteHintHop
    UserNode {
        lsp_info: LspInfo,
        intercept_scids: Vec<Scid>,
    },
    Lsp,
}

#[instrument(skip_all, name = "(node-info)")]
pub fn node_info<CM, PM, PS>(
    version: semver::Version,
    measurement: Measurement,
    channel_manager: &CM,
    peer_manager: &PM,
    wallet: &LexeWallet,
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: &[ChannelDetails],
    lsp_fees: LspFees,
) -> NodeInfo
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    let node_pk = NodePk(channel_manager.get_our_node_id());

    let num_peers = peer_manager.list_peers().len();

    let num_channels: usize = channels.len();

    let (lightning_balance, num_usable_channels) =
        balance::all_channel_balances(chain_monitor, channels, lsp_fees);

    let onchain_balance = wallet.get_balance();

    let pending_monitor_updates = chain_monitor
        .list_pending_monitor_updates()
        .values()
        .map(|v| v.len())
        .sum();

    NodeInfo {
        version,
        measurement,
        node_pk,
        num_channels,
        num_usable_channels,
        lightning_balance,
        num_peers,
        onchain_balance,
        pending_monitor_updates,
    }
}

#[instrument(skip_all, name = "(list-channels)")]
pub fn list_channels<PS: LexePersister>(
    network_graph: &NetworkGraphType,
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: impl IntoIterator<Item = ChannelDetails>,
) -> anyhow::Result<ListChannelsResponse> {
    let read_only_network_graph = network_graph.read_only();

    let channels = channels
        .into_iter()
        .map(|channel| {
            let channel_id = channel.channel_id;
            let channel_balance =
                balance::channel_balance(chain_monitor, &channel)?;

            let counterparty_node_id =
                NodeId::from_pubkey(&channel.counterparty.node_id);
            let counterparty_alias = read_only_network_graph
                .node(&counterparty_node_id)
                .and_then(|node_info| node_info.announcement_info.as_ref())
                // The Display impl here handles non-printable chars safely.
                .map(|ann_info| ann_info.alias().to_string());

            LxChannelDetails::from_ldk(
                channel,
                channel_balance,
                counterparty_alias,
            )
            .context(channel_id)
        })
        .collect::<anyhow::Result<Vec<LxChannelDetails>>>()
        .context("Error listing channel details")?;
    Ok(ListChannelsResponse { channels })
}

/// Open and fund a new channel with `channel_value` and `counterparty_node_pk`.
///
/// After checking that we have enough balance for the new channel, we'll await
/// on `ensure_counterparty_connected()`, which should proactively try to
/// connect to the new channel counterparty if we're not already connected.
///
/// Once the new channel is registered with LDK, we wait for the channel to
/// become `Pending` (success) or `Closed` (failure). If the new channel
/// `is_jit_channel` (an LSP->User JIT channel), it will wait for full channel
/// `Ready`.
#[instrument(skip_all, name = "(open-channel)")]
pub async fn open_channel<CM, PS, F>(
    channel_manager: &CM,
    channel_events_bus: &ChannelEventsBus,
    wallet: &LexeWallet,
    ensure_counterparty_connected: impl FnOnce() -> F,
    user_channel_id: LxUserChannelId,
    channel_value: Amount,
    counterparty_node_pk: &NodePk,
    user_config: UserConfig,
    is_jit_channel: bool,
) -> anyhow::Result<OpenChannelResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    F: Future<Output = anyhow::Result<()>>,
{
    info!(
        %counterparty_node_pk, %user_channel_id,
        %channel_value, %is_jit_channel,
        "Opening channel"
    );

    // Check if we've already opened a channel with this `user_channel_id`.
    //
    // NOTE(phlip9): The idempotency here is not perfect; there's still a race
    // if we send multiple `open_channel` requests with the same
    // `user_channel_id` concurrently. But we mostly care about serial retries,
    // for which this is good enough^tm.
    {
        // Start listening for channel events. Do this before we look at our
        // channels so we pick up any events that occur while we're looking.
        let mut channel_events_rx = channel_events_bus.subscribe();

        let uid = user_channel_id.to_u128();
        let maybe_channel = channel_manager
            .list_channels_with_counterparty(&counterparty_node_pk.0)
            .into_iter()
            .find(|channel| channel.user_channel_id == uid);

        // Check if there's an existing channel with this `user_channel_id`.
        if let Some(channel) = maybe_channel {
            let temp_channel_id =
                ChannelId::from(user_channel_id.derive_temporary_channel_id());

            // If the channel doesn't have the `temp_channel_id` anymore, it
            // must be `Pending`. We can return it.
            let channel_id = channel.channel_id;
            if channel_id != temp_channel_id {
                // If it's a JIT channel, it also needs to be `Ready`
                #[allow(clippy::nonminimal_bool)] // More readable IMO
                if !is_jit_channel
                    || (is_jit_channel && channel.is_channel_ready)
                {
                    return Ok(OpenChannelResponse {
                        channel_id: LxChannelId::from(channel_id),
                    });
                }
            }

            // Wait for the next relevant channel event with this
            // `user_channel_id`.
            return wait_for_our_channel_open_event(
                &mut channel_events_rx,
                is_jit_channel,
                &user_channel_id,
            )
            .await;
        }

        // No existing channel, proceed normally.
    }

    // Check if we actually have enough on-chain funds for this channel +
    // on-chain fees. This check isn't safety critical; it just lets us quickly
    // avoid a lot of unnecessary work.
    let _fees = wallet.preflight_channel_funding_tx(channel_value)?;

    // Ensure channel counterparty is connected.
    ensure_counterparty_connected()
        .await
        .context("Failed to connect to channel counterparty")?;

    // Start listening for channel events. Do this before we notify LDK so we
    // definitely pick up any events.
    let mut channel_events_rx = channel_events_bus.subscribe();

    // Tell LDK to start the open channel process.
    let push_msat = 0; // No need for this yet
    let temp_channel_id =
        ChannelId::from(user_channel_id.derive_temporary_channel_id());
    channel_manager
        .create_channel(
            counterparty_node_pk.0,
            channel_value.sats_u64(),
            push_msat,
            user_channel_id.to_u128(),
            Some(temp_channel_id),
            Some(user_config),
        )
        .map_err(|e| anyhow!("Failed to create channel: {e:?}"))?;

    // Wait for the next relevant channel event with this `user_channel_id`.
    wait_for_our_channel_open_event(
        &mut channel_events_rx,
        is_jit_channel,
        &user_channel_id,
    )
    .await
}

/// Wait for the next relevant channel event for a new `open_channel` with this
/// `user_channel_id`.
///
/// If this is a JIT channel open, we can wait for channel `Ready` and not
/// just `Pending`.
async fn wait_for_our_channel_open_event(
    channel_events_rx: &mut ChannelEventsRx<'_>,
    is_jit_channel: bool,
    user_channel_id: &LxUserChannelId,
) -> anyhow::Result<OpenChannelResponse> {
    let channel_event = channel_events_rx
        .next_filtered(|event| {
            if event.user_channel_id() != user_channel_id {
                return false;
            }

            if is_jit_channel {
                matches!(
                    event,
                    ChannelEvent::Ready { .. } | ChannelEvent::Closed { .. }
                )
            } else {
                matches!(event, ChannelEvent::Pending { .. })
            }
        })
        .apply(|fut| tokio::time::timeout(Duration::from_secs(15), fut))
        .await
        .context("Waiting for channel event")?;

    match channel_event {
        ChannelEvent::Pending { .. } =>
            debug!(%user_channel_id, "Received ChannelEvent::Pending"),
        ChannelEvent::Ready { .. } =>
            debug!(%user_channel_id, "Received ChannelEvent::Ready"),
        ChannelEvent::Closed { reason, .. } =>
            return Err(anyhow!("Channel open failed: {reason}")),
    }

    Ok(OpenChannelResponse {
        channel_id: *channel_event.channel_id(),
    })
}

/// Check if we actually have enough on-chain funds for this channel and return
/// the on-chain fees required.
pub async fn preflight_open_channel(
    wallet: &LexeWallet,
    req: PreflightOpenChannelRequest,
) -> anyhow::Result<PreflightOpenChannelResponse> {
    let fee_estimate = wallet.preflight_channel_funding_tx(req.value)?;
    Ok(PreflightOpenChannelResponse { fee_estimate })
}

/// Close a channel and wait for the corresponding [`Event::ChannelClosed`].
///
/// For a co-operative channel close, we'll first call
/// `ensure_counterparty_connected`, which should proactively connect to the
/// counterparty (if not already connected).
///
/// If `req.force_close` is set, this will begin channel force closure, without
/// first trying to connect to the counterparty.
///
/// If `req.maybe_counterparty` is unset, we'll look it up from the channels
/// list. Note that this is somewhat expensive--ideally the caller should
/// provide the channel counterparty.
///
/// [`Event::ChannelClosed`]: lightning::events::Event::ChannelClosed
#[instrument(skip_all, name = "(close-channel)")]
pub async fn close_channel<CM, PS, F>(
    channel_manager: &CM,
    channel_events_bus: &ChannelEventsBus,
    ensure_counterparty_connected: impl FnOnce(NodePk) -> F,
    req: CloseChannelRequest,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    F: Future<Output = anyhow::Result<()>>,
{
    let CloseChannelRequest {
        channel_id,
        force_close,
        maybe_counterparty,
    } = req;
    let lx_channel_id = channel_id;
    let ln_channel_id = ChannelId::from(lx_channel_id);

    info!(%channel_id, ?force_close, "closing channel");

    // Lookup the counterparty's NodePk from our channels, if the request
    // didn't specify.
    let counterparty = maybe_counterparty
        .or_else(|| {
            // TODO(phlip9): this is fairly inefficient...
            channel_manager
                .list_channels()
                .into_iter()
                .find(|c| c.channel_id.0 == channel_id.0)
                .map(|c| NodePk(c.counterparty.node_id))
        })
        .context("No channel exists with this channel id")?;

    // Before cooperatively closing the channel, we need to ensure we're
    // connected with the counterparty.
    if !force_close {
        ensure_counterparty_connected(counterparty)
            .await
            .with_context(|| format!("{counterparty}"))
            .context("Failed to connect to channel counterparty")?;
    }

    // Start subscribing to ChannelEvents.
    let mut channel_events_rx = channel_events_bus.subscribe();

    // Tell the channel manager to begin co-op or forced channel close.
    if !force_close {
        // Co-operative close
        channel_manager
            .close_channel(&ln_channel_id, &counterparty.0)
            .map_err(|e| anyhow!("{e:?}"))
            .context("Channel manager failed to begin coop channel close")?;
    } else {
        // Force close
        let error_msg = "User-initiated force close".to_owned();
        channel_manager
            .force_close_broadcasting_latest_txn(
                &ln_channel_id,
                &counterparty.0,
                error_msg,
            )
            .map_err(|e| anyhow!("{e:?}"))
            .context("Channel manager failed to force close channel")?;
    }

    // Wait for the corresponding ChannelClosed event
    tokio::time::timeout(
        Duration::from_secs(15),
        channel_events_rx.next_filtered(|event| {
            matches!(
                event,
                ChannelEvent::Closed { channel_id, .. } if channel_id == &lx_channel_id,
            )
        }),
    )
    .await
    .context("Waiting for channel close event")?;

    // TODO(phlip9): return txid so user can track close
    info!(%channel_id, "channel closed");

    Ok(())
}

/// Estimate the on-chain fees required to close this channel.
pub async fn preflight_close_channel<CM, PS>(
    channel_manager: &CM,
    chain_monitor: &LexeChainMonitorType<PS>,
    fee_estimates: &FeeEstimates,
    req: PreflightCloseChannelRequest,
) -> anyhow::Result<PreflightCloseChannelResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let channels = match &req.maybe_counterparty {
        Some(counterparty) =>
            channel_manager.list_channels_with_counterparty(&counterparty.0),
        None => channel_manager.list_channels(),
    };
    let channel = channels
        .into_iter()
        .find(|chan| chan.channel_id.0 == req.channel_id.0)
        .context("No channel with this id")?;

    // If we haven't negotiated a funding_txo, the channel is free to close.
    let monitor = channel
        .funding_txo
        .and_then(|txo| chain_monitor.get_monitor(txo).ok());
    let monitor = match monitor {
        Some(x) => x,
        None =>
            return Ok(PreflightCloseChannelResponse {
                fee_estimate: Amount::ZERO,
            }),
    };

    // TODO(phlip9): handle force_close=true
    let fee_estimate = our_close_tx_fees_sats(fee_estimates, &channel, monitor);
    let fee_estimate = Amount::try_from_sats_u64(fee_estimate)?;

    // TODO(phlip9): include est. blocks to confirmation? Esp. for force close.
    Ok(PreflightCloseChannelResponse { fee_estimate })
}

/// Calculate the fees _we_ have to pay to close this channel.
///
/// TODO(phlip9): support v2/anchor channels
fn our_close_tx_fees_sats(
    fee_estimates: &FeeEstimates,
    channel: &ChannelDetails,
    monitor: LockedChannelMonitor<'_, SignerType>,
) -> u64 {
    use lightning::chain::channelmonitor::Balance;
    let our_sats: u64 = monitor
        .get_claimable_balances()
        .into_iter()
        .map(|b| match b {
            Balance::ClaimableOnChannelClose {
                amount_satoshis,
                transaction_fee_satoshis,
                outbound_payment_htlc_rounded_msat: _,
                outbound_forwarded_htlc_rounded_msat: _,
                inbound_claiming_htlc_rounded_msat: _,
                inbound_htlc_rounded_msat: _,
            } => amount_satoshis + transaction_fee_satoshis,
            Balance::ClaimableAwaitingConfirmations { .. } => 0,
            Balance::ContentiousClaimable { .. } => 0,
            Balance::MaybeTimeoutClaimableHTLC { .. } => 0,
            Balance::MaybePreimageClaimableHTLC { .. } => 0,
            Balance::CounterpartyRevokedOutputClaimable { .. } => 0,
        })
        .sum();
    if our_sats == 0 {
        return 0;
    };

    // We only pay for the on-chain channel close fees if we're the channel
    // funder.
    //
    // For our purposes, if we're not the funder and our output is also beneath
    // our dust limit, we'll just consider our remaining channel balance as part
    // // of the close fee.
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

    // As the funder, if we somehow don't have enough to pay the full
    // `tx_fees_sats`, then the most we can possibly pay (without RBF / anchors)
    // is our current balance. Most likely the remote will force close.
    // Usually the channel reserve should prevent this case from happening, i.e,
    // we should have enough balance to pay the on-chain fees.
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

/// Estimate the total on-chain fees for a channel close, which must be paid by
/// the channel funder.
fn close_tx_fees_sats(
    fee_estimates: &FeeEstimates,
    channel: &ChannelDetails,
) -> u64 {
    let conf_target = ConfirmationTarget::NonAnchorChannelFee;
    let fee_sat_per_kwu =
        fee_estimates.get_est_sat_per_1000_weight(conf_target) as u64;

    let close_tx_weight = CLOSE_TX_WEIGHT;
    let normal_fee_sats =
        fee_sat_per_kwu.saturating_mul(close_tx_weight) / 1000;

    let force_close_avoidance_max_fee_sats = channel
        .config
        .map(|c| c.force_close_avoidance_max_fee_satoshis)
        .unwrap_or(constants::FORCE_CLOSE_AVOIDANCE_MAX_FEE_SATS);

    // For some reason the `force_close_avoidance_max_fee_sats` is always
    // getting added?

    normal_fee_sats.saturating_add(force_close_avoidance_max_fee_sats)
}

/// Between User and LSP, the close tx is currently predictable.
///
/// LDK (currently) always over-estimates the close tx cost by one output if one
/// side's balance (after fees) is below their dust limit.
const CLOSE_TX_WEIGHT: u64 = close_tx_weight(
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
/// See: <https://github.com/lightningdevkit/rust-lightning/blob/70add1448b5c36368b8f1c17d672d8871cee14de/lightning/src/ln/channel.rs#L3962>
const fn close_tx_weight(
    funding_redeemscript_len: u64,
    a_scriptpubkey_len: u64,
    b_scriptpubkey_len: u64,
) -> u64 {
    (4 +                                    // version
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

/// Uses the given `[bdk|ldk]_resync_tx` to retrigger BDK and LDK sync, and
/// returns once sync has either completed or timed out.
pub async fn resync(
    bdk_resync_tx: &mpsc::Sender<oneshot::Sender<()>>,
    ldk_resync_tx: &mpsc::Sender<oneshot::Sender<()>>,
) -> anyhow::Result<Empty> {
    /// How long we'll wait to hear a callback before giving up.
    // NOTE: Our default reqwest::Client timeout is 15 seconds.
    const SYNC_TIMEOUT: Duration = Duration::from_secs(12);

    let (bdk_tx, bdk_rx) = oneshot::channel();
    bdk_resync_tx
        .try_send(bdk_tx)
        .map_err(|_| anyhow!("Failed to retrigger BDK sync"))?;
    let (ldk_tx, ldk_rx) = oneshot::channel();
    ldk_resync_tx
        .try_send(ldk_tx)
        .map_err(|_| anyhow!("Failed to retrigger LDK sync"))?;

    let bdk_fut = tokio::time::timeout(SYNC_TIMEOUT, bdk_rx);
    let ldk_fut = tokio::time::timeout(SYNC_TIMEOUT, ldk_rx);
    let (try_bdk, try_ldk) = tokio::join!(bdk_fut, ldk_fut);
    try_bdk
        .context("BDK sync timed out")?
        .context("BDK recv errored")?;
    try_ldk
        .context("LDK sync timed out")?
        .context("LDK recv errored")?;

    debug!("/resync successful");
    Ok(Empty {})
}

#[instrument(skip_all, name = "(create-invoice)")]
pub async fn create_invoice<CM, PS>(
    req: CreateInvoiceRequest,
    channel_manager: &CM,
    keys_manager: &LexeKeysManager,
    payments_manager: &PaymentsManager<CM, PS>,
    caller: CreateInvoiceCaller,
    network: LxNetwork,
) -> anyhow::Result<CreateInvoiceResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let amount = &req.amount;
    let cltv_expiry = MIN_FINAL_CLTV_EXPIRY_DELTA;
    info!("Handling create_invoice command for {amount:?} msats");

    // TODO(max): We should set some sane maximum for the invoice expiry time,
    // e.g. 180 days. This will not cause LDK state to blow up since
    // create_inbound_payment derives its payment preimages and hashes, but it
    // could bloat Lexe's DB with fairly large `LxInvoice`s.

    // We use ChannelManager::create_inbound_payment because this method allows
    // the channel manager to store the hash and preimage for us, instead of
    // having to manage a separate inbound payments storage outside of LDK.
    // NOTE that `handle_payment_claimable` will panic if the payment preimage
    // is not known by (and therefore cannot be provided by) LDK.
    let (hash, secret) = channel_manager
        .create_inbound_payment(
            req.amount.map(|amt| amt.msat()),
            req.expiry_secs,
            Some(cltv_expiry),
        )
        .map_err(|()| anyhow!("Supplied amount > total bitcoin supply!"))?;
    let preimage = channel_manager
        .get_payment_preimage(hash, secret)
        .map_err(|e| anyhow!("Could not get preimage: {e:?}"))?;

    let currency = Currency::from(network);
    let sha256_hash = sha256::Hash::from_slice(&hash.0)
        .expect("Should never fail with [u8;32]");
    let expiry_time = Duration::from_secs(u64::from(req.expiry_secs));
    let our_node_pk = channel_manager.get_our_node_id();

    // Add most parts of the invoice, except for the route hints.
    // This is modeled after lightning_invoice's internal utility function
    // _create_invoice_from_channelmanager_and_duration_since_epoch_with_payment_hash
    #[rustfmt::skip] // Nicer for the generic annotations to be aligned
    let mut builder = InvoiceBuilder::new(currency)          // <D, H, T, C, S>
        .description(req.description.unwrap_or_default())    // D: False -> True
        .payment_hash(sha256_hash)                           // H: False -> True
        .current_timestamp()                                 // T: False -> True
        .min_final_cltv_expiry_delta(u64::from(cltv_expiry)) // C: False -> True
        .payment_secret(secret)                              // S: False -> True
        .basic_mpp()                                         // S: _ -> True
        .expiry_time(expiry_time)
        .payee_pub_key(our_node_pk);

    if let Some(amount) = req.amount {
        builder = builder.amount_milli_satoshis(amount.invoice_safe_msat()?);
    }

    if let CreateInvoiceCaller::UserNode {
        intercept_scids, ..
    } = &caller
    {
        ensure!(
            !intercept_scids.is_empty(),
            "User node must include intercept hints to receive payments"
        );
    }

    // Construct the route hint(s).
    let route_hints = match caller {
        // If the LSP is calling create_invoice, include no hints and let
        // the sender route to us by looking at the lightning network graph.
        CreateInvoiceCaller::Lsp => Vec::new(),
        // If a user node is calling create_invoice, always include at least one
        // intercept hint. We do this even when the user already has a channel
        // with enough balance to service the payment, because it allows the LSP
        // to intercept the HTLC and wake the user if a payment comes in while
        // the user is offline.
        CreateInvoiceCaller::UserNode {
            lsp_info,
            intercept_scids,
        } => {
            let channels = channel_manager.list_channels();

            // For the fee rates and CLTV delta to include in our route hint(s),
            // use the maximum of the values observed in our channels and the
            // LSP's configured value according to `LspInfo`, defaulting to the
            // `LspInfo` value if a value is not available from our channels.
            let base_msat = channels
                .iter()
                .filter_map(|channel| {
                    channel
                        .counterparty
                        .forwarding_info
                        .as_ref()
                        .map(|info| info.fee_base_msat)
                })
                .max()
                // This ensures that we can still receive the payment over a JIT
                // channel if the JIT channel feerate is higher than any of our
                // current channel feerates.
                .map(|value| max(value, lsp_info.lsp_usernode_base_fee_msat))
                .unwrap_or(lsp_info.lsp_usernode_base_fee_msat);
            let proportional_millionths = channels
                .iter()
                .filter_map(|channel| {
                    channel
                        .counterparty
                        .forwarding_info
                        .as_ref()
                        .map(|info| info.fee_proportional_millionths)
                })
                .max()
                // Likewise as above
                .map(|value| max(value, lsp_info.lsp_usernode_prop_fee_ppm))
                .unwrap_or(lsp_info.lsp_usernode_prop_fee_ppm);
            let cltv_expiry_delta = channels
                .iter()
                .filter_map(|channel| {
                    channel
                        .counterparty
                        .forwarding_info
                        .as_ref()
                        .map(|info| info.cltv_expiry_delta)
                })
                .max()
                // Likewise as above
                .map(|value| max(value, lsp_info.cltv_expiry_delta))
                .unwrap_or(lsp_info.cltv_expiry_delta);

            // Take the min HTLC minimum across all our channels and the LSP's
            // configured value, even though it's currently 1 msat everywhere.
            //
            // Rationale:
            // - If we have any channels open, we can most likely receive a
            //   value equal to the minimum of the `htlc_minimum_msat`s across
            //   our channels (unless we have absolutely 0 liquidity left).
            // - If we have no channels open, we have to use the LSP's
            //   configured value for JIT channels. This may come in play in a
            //   scerario where (1) Lexe *isn't* subsidizing channel open costs
            //   but (2) we haven't implemented Ark/Spark/etc for handling small
            //   amounts, and thus need the user's first receive to be beyond 3k
            //   sats or whatever the prevailing on-chain fee is. In this case,
            //   the JIT hint with a higher HTLC minimum would alert the sender
            //   that such a small payment is not routable.
            let htlc_minimum_msat = channels
                .iter()
                .filter_map(|channel| channel.inbound_htlc_minimum_msat)
                .min()
                .map(|value| min(value, lsp_info.htlc_minimum_msat))
                .unwrap_or(lsp_info.htlc_minimum_msat);

            // Our capacity to receive is effectively infinite, bounded only by
            // the largest HTLCs Lexe's LSP is willing to forward to us. An
            // alternative approach would set one intercept hint with the LSP's
            // HTLC maximum, with the remaining hints set to the largest
            // `inbound_capacity` amounts available in existing channels. But
            // we can't incentivize the sender to use our existing channels by
            // setting the feerate higher in the JIT hint, because this would
            // cause them to overpay fees if they actually do use the JIT hint.
            // Thus, we just uniformly use the LSP's configured HTLC maximum.
            let htlc_maximum_msat = lsp_info.htlc_maximum_msat;

            let fees = RoutingFees {
                base_msat,
                proportional_millionths,
            };

            // Multi-hint impl, in case we switch back
            /*
            intercept_scids
                .into_iter()
                .take(MAX_INTERCEPT_HINTS)
                .map(|scid| {
                    let route_hint_hop = RouteHintHop {
                        src_node_id: lsp_info.node_pk.0,
                        short_channel_id: scid.0,
                        fees,
                        cltv_expiry_delta,
                        htlc_minimum_msat: Some(htlc_minimum_msat),
                        htlc_maximum_msat: Some(htlc_maximum_msat),
                    };
                    RouteHint(vec![route_hint_hop])
                })
                .collect::<Vec<RouteHint>>()
            */

            // If there are multiple intercept scids, just pick the last one, as
            // it is likely the most recently generated.
            let scid = intercept_scids
                .last()
                .context("No intercept hints provided")?;
            let route_hint_hop = RouteHintHop {
                src_node_id: lsp_info.node_pk.0,
                short_channel_id: scid.0,
                fees,
                cltv_expiry_delta,
                htlc_minimum_msat: Some(htlc_minimum_msat),
                htlc_maximum_msat: Some(htlc_maximum_msat),
            };
            vec![RouteHint(vec![route_hint_hop])]
        }
    };
    debug!("Including route hints: {route_hints:?}");
    for hint in route_hints {
        builder = builder.private_route(hint);
    }

    // Build, sign, and return the invoice
    let raw_invoice =
        builder.build_raw().context("Could not build raw invoice")?;
    let recipient = Recipient::Node;
    let raw_invoice_signature = keys_manager
        .sign_invoice(&raw_invoice, recipient)
        .map_err(|()| anyhow!("Failed to sign invoice"))?;
    let signed_raw_invoice = raw_invoice
        .sign(|_| Ok::<_, Infallible>(raw_invoice_signature))
        .expect("Infallible");
    let invoice = Bolt11Invoice::from_signed(signed_raw_invoice)
        .map(LxInvoice)
        .context("Invoice was semantically incorrect")?;

    let payment = InboundInvoicePayment::new(
        invoice.clone(),
        hash.into(),
        secret.into(),
        preimage.into(),
    );
    payments_manager
        .new_payment(payment.into())
        .await
        .context("Could not register new payment")?;

    info!("Success: Generated invoice {invoice}");

    Ok(CreateInvoiceResponse { invoice })
}

#[instrument(skip_all, name = "(pay-invoice)")]
pub async fn pay_invoice<CM, PS>(
    req: PayInvoiceRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    lsp_fees: LspFees,
) -> anyhow::Result<PayInvoiceResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Pre-flight the invoice payment (verify and route).
    let PreflightedPayInvoice {
        payment,
        route_params,
        recipient_fields,
    } = preflight_pay_invoice_inner(
        req,
        router,
        channel_manager,
        payments_manager,
        chain_monitor,
        lsp_fees,
    )
    .await?;
    let hash = payment.hash;

    let payment = Payment::from(payment);
    let created_at = payment.created_at();

    // Pre-flight looks good, now we can register this payment in the Lexe
    // payments manager.
    payments_manager
        .new_payment(payment)
        .await
        .context("Already tried to pay this invoice")?;

    // Send the payment, letting LDK handle payment retries, and match on the
    // result, registering a failure with the payments manager if appropriate.
    match channel_manager.send_payment(
        PaymentHash::from(hash),
        recipient_fields,
        PaymentId::from(hash),
        route_params,
        OUTBOUND_PAYMENT_RETRY_STRATEGY,
    ) {
        Ok(()) => {
            info!(%hash, "Success: OIP initiated immediately");
            Ok(PayInvoiceResponse { created_at })
        }
        Err(RetryableSendFailure::DuplicatePayment) => {
            // This should never happen because we should have already checked
            // for uniqueness when registering the new payment above. If it
            // somehow does, we should let the first payment follow its course,
            // and wait for a PaymentSent or PaymentFailed event.
            Err(anyhow!("Somehow got DuplicatePayment error (OIP {hash})"))
        }
        Err(RetryableSendFailure::PaymentExpired) => {
            // We've already checked the expiry of the invoice to be paid, but
            // perhaps there was a TOCTTOU race? Regardless, if this variant is
            // returned, LDK does not track the payment and thus will not emit a
            // PaymentFailed later, so we should fail the payment now.
            payments_manager
                .payment_failed(hash.into(), LxOutboundPaymentFailure::Expired)
                .await
                .context("(PaymentExpired) Could not register failure")?;
            Err(anyhow!("LDK returned PaymentExpired (OIP {hash})"))
        }
        Err(RetryableSendFailure::RouteNotFound) => {
            // It appears that if this variant is returned, LDK does not track
            // the payment, so we should fail the payment immediately.
            // If the user wants to retry, they'll need to ask the recipient to
            // generate a new invoice. TODO(max): Is this really what we want?
            payments_manager
                .payment_failed(hash.into(), LxOutboundPaymentFailure::NoRoute)
                .await
                .context("(RouteNotFound) Could not register failure")?;
            Err(anyhow!("LDK returned RouteNotFound (OIP {hash})"))
        }
        Err(RetryableSendFailure::OnionPacketSizeExceeded) => {
            // If the metadata causes us to exceed the maximum onion packet
            // size, it probably isn't possible to pay this. Fail the payment.
            payments_manager
                .payment_failed(
                    hash.into(),
                    LxOutboundPaymentFailure::MetadataTooLarge,
                )
                .await
                .context(
                    "(OnionPacketSizeExceeded) Could not register failure",
                )?;
            Err(anyhow!("LDK returned OnionPacketSizeExceeded (OIP {hash})"))
        }
    }
}

#[instrument(skip_all, name = "(preflight-pay-invoice)")]
pub async fn preflight_pay_invoice<CM, PS>(
    req: PreflightPayInvoiceRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    lsp_fees: LspFees,
) -> anyhow::Result<PreflightPayInvoiceResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let req = PayInvoiceRequest {
        invoice: req.invoice,
        fallback_amount: req.fallback_amount,
        // User note not relevant for pre-flight.
        note: None,
    };
    let preflight = preflight_pay_invoice_inner(
        req,
        router,
        channel_manager,
        payments_manager,
        chain_monitor,
        lsp_fees,
    )
    .await?;
    Ok(PreflightPayInvoiceResponse {
        amount: preflight.payment.amount,
        fees: preflight.payment.fees,
    })
}

#[instrument(skip_all, name = "(create-offer)")]
pub async fn create_offer<CM, PS>(
    req: CreateOfferRequest,
    channel_manager: &CM,
) -> anyhow::Result<CreateOfferResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Make absolute expiry deadline.
    let absolute_expiry = req.expiry_secs.map(|secs| {
        let expiry = Duration::from_secs(u64::from(secs));
        let expires_at = TimestampMs::now().saturating_add(expiry);
        expires_at.into_duration()
    });

    let mut builder = channel_manager
        .create_offer_builder(absolute_expiry)
        .map_err(|err| anyhow!("Failed to create offer builder: {err:?}"))?;

    // TODO(phlip9): don't add `chains` param when mainnet to save space

    // TODO(phlip9): Probably need to build the blinded path ourselves. It's
    // not clear how the channel manager would pick the right blinded path
    // params / route hints the same way as `create_invoice`. How could it
    // possibly know the LSP info if the user has no channels?

    // TODO(phlip9): can we condense the blinded path? default offer is ~489B.
    // right now the default offer blinded path has
    //   + ~  54B (1) introductory node pk (LSP NodePk, clear)
    //   + ~  54B (2) blinding point pk
    //   + ~  54B (3) blinded issuer signing pk (user NodePk, blinded)
    //   + ~ 137B (4) hop 1: blinded pk + 51 B encrypted payload
    //   + ~ 137B (5) hop 2: blinded pk + 51 B encrypted payload
    //   = ~ 436B blinded path overhead

    // TODO(phlip9): what happens to long-lived offers after the LSP changes
    // the fee rates?

    // TODO(phlip9): LSP should not use blinded path at all

    if let Some(amount) = req.amount {
        builder = builder.amount_msats(amount.msat());
    }
    if let Some(description) = req.description {
        builder = builder.description(description);
    }

    let offer: LxOffer = builder
        .build()
        .map(LxOffer)
        .map_err(|err| anyhow!("Failed to build offer: {err:?}"))?;
    Ok(CreateOfferResponse { offer })
}

#[instrument(skip_all, name = "(pay-onchain)")]
pub async fn pay_onchain<CM, PS>(
    req: PayOnchainRequest,
    network: LxNetwork,
    wallet: &LexeWallet,
    tx_broadcaster: &TxBroadcaster,
    payments_manager: &PaymentsManager<CM, PS>,
) -> anyhow::Result<PayOnchainResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Create and sign the onchain send tx.
    let onchain_send = wallet
        .create_onchain_send(req, network)
        .context("Error while creating outbound tx")?;
    let tx = onchain_send.tx.clone();
    let id = onchain_send.id();
    let txid = onchain_send.txid;

    let payment = Payment::from(onchain_send);
    let created_at = payment.created_at();

    // Register the transaction.
    payments_manager
        .new_payment(payment)
        .await
        .context("Could not register new onchain send")?;

    // Broadcast.
    tx_broadcaster
        .broadcast_transaction(tx)
        .await
        .context("Failed to broadcast tx")?;

    // Register the successful broadcast.
    payments_manager
        .onchain_send_broadcasted(&id, &txid)
        .await
        .context("Could not register broadcast of tx")?;

    // NOTE: The reason why we call into the payments manager twice (and thus
    // persist the payment twice) instead of simply registering the new payment
    // after it has been successfully broadcast is so that we don't end up in a
    // situation where a payment is successfully sent but we have no record of
    // it; i.e. the broadcast succeeds but our registration doesn't. This also
    // ensures that the txid is unique before we broadcast in case there is a
    // txid collision for some reason (e.g. duplicate requests)

    Ok(PayOnchainResponse { created_at, txid })
}

#[instrument(skip_all, name = "(estimate-fee-send-onchain)")]
pub fn preflight_pay_onchain(
    req: PreflightPayOnchainRequest,
    wallet: &LexeWallet,
    network: LxNetwork,
) -> anyhow::Result<PreflightPayOnchainResponse> {
    wallet.preflight_pay_onchain(req, network)
}

// A preflighted BOLT11 invoice payment. That is, this is the outcome of
// validating and routing a BOLT11 invoice, without actually paying yet.
struct PreflightedPayInvoice {
    payment: OutboundInvoicePayment,
    route_params: RouteParameters,
    recipient_fields: RecipientOnionFields,
}

// Preflight (validate and route) a new potential BOLT11 invoice that we might
// pay.
async fn preflight_pay_invoice_inner<CM, PS>(
    req: PayInvoiceRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    lsp_fees: LspFees,
) -> anyhow::Result<PreflightedPayInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let invoice = req.invoice;

    // Fail expired invoices early.
    ensure!(!invoice.is_expired(), "Invoice has expired");

    // Fail invoice double-payment early.
    if payments_manager
        .contains_payment_id(&invoice.payment_id())
        .await
    {
        bail!("We've already tried paying this invoice");
    }

    // Compute the amount; handle amountless invoices
    let amount = {
        if invoice.amount().is_some() && req.fallback_amount.is_some() {
            // Not a serious error, but better to be unambiguous.
            debug_panic_release_log!(
                "Nit: Only provide fallback amount for amountless invoices",
            );
        }

        invoice
            .amount()
            .or(req.fallback_amount)
            .context("Missing fallback amount for amountless invoice")?
    };

    // Construct payment parameters, which are amount-agnostic.
    let payment_params = route::build_payment_params(Either::Right(&invoice))
        .context("Couldn't build payment parameters")?;

    // Construct payment routing context
    let routing_context =
        RoutingContext::from_payment_params(channel_manager, payment_params);

    // Compute Lightning balances
    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let (lightning_balance, num_usable_channels) =
        balance::all_channel_balances(chain_monitor, &channels, lsp_fees);
    let max_sendable = lightning_balance.max_sendable;

    // Check that the user has at least one usable channel.
    if num_channels == 0 {
        // Noob error: Take the opportunity to teach the user about LN channels.
        return Err(anyhow!(
            "You don't have any Lightning channels, which are required to \
             send funds. Consider opening a new channel using on-chain funds, \
             or receiving funds to your wallet directly via Lightning."
        ));
    } else if num_usable_channels == 0 {
        // The user has at least one channel, but none of them are usable.
        // Maybe their channel is being closed or something.
        return Err(anyhow!(
            "You don't have any usable Lightning channels. Consider opening a \
             new channel using on-chain funds, or receiving funds to your \
             wallet directly via Lightning."
        ));
    }

    // Check that we're not trying to send over `max_sendable`.
    if amount > max_sendable {
        // Since we know the recipient, we can compute a more accurate maximum
        // sendable amount to this recipient (i.e. max flow) and expose that to
        // the user as a suggestion.
        //
        // TODO(max): We should also calculate the max flow from the *LSP*, so
        // we can tell whether (1) `max_flow` is limited by the user's balance
        // or (2) there simply isn't enough liquidity from LSP to recipient.
        let max_flow_result = route::compute_max_flow_to_recipient(
            router,
            &routing_context,
            amount,
        );

        let error = match max_flow_result {
            Ok(max_flow) => anyhow!(
                "Insufficient balance: Tried to pay {amount} sats, but the \
                 maximum amount you can send is {max_sendable} sats. \
                 The maximum amount that you can route to this recipient is \
                 {max_flow} sats. Consider adding to your Lightning balance \
                 or sending a smaller amount.",
            ),
            Err(e) => anyhow!(
                "Couldn't route to this recipient with any amount: {e:#}"
            ),
        };

        return Err(error);
    }

    // Try to find a Route with the full intended amount.
    let route_result = routing_context.find_route(router, amount);
    let (route, route_params) = match route_result {
        Ok((r, p)) => (r, p),
        // This error is just "Failed to find a path to the given destination",
        // which is not helpful, so we don't include it in our error message.
        Err(_) => {
            // We couldn't find a route with the full intended amount.
            // But since we know the recipient, we can compute a more accurate
            // maximum sendable amount to this recipient (i.e. max flow).
            let max_flow_result = route::compute_max_flow_to_recipient(
                router,
                &routing_context,
                amount,
            );

            let error = match max_flow_result {
                Ok(max_flow) => {
                    // TODO(max): We should also calculate the max flow from the
                    // *LSP*, so we can tell whether (1) `max_flow` is limited
                    // by the user's balance or (2) there simply isn't enough
                    // liquidity from the LSP to the recipient.
                    //
                    // This call to action could then be one of:
                    // 1) "You must add to your Lightning balance in order to
                    //    send this amount to this recipient"
                    // 2) "Consider sending a smaller amount or asking the
                    //    recipient to increase their inbound liquidity."
                    let call_to_action =
                        "Consider adding to your Lightning balance \
                         or sending a smaller amount.";

                    anyhow!(
                        "Tried to pay {amount} sats. The maximum amount that \
                         you can route to this recipient is {max_flow} sats. \
                         {call_to_action}",
                    )
                }
                Err(e) => anyhow!(
                    "Couldn't route to this recipient with any amount: {e:#}"
                ),
            };

            return Err(error);
        }
    };

    let payment_secret = invoice.payment_secret().into();
    let recipient_fields = RecipientOnionFields::secret_only(payment_secret);

    let payment = OutboundInvoicePayment::new(invoice, &route, req.note);
    Ok(PreflightedPayInvoice {
        payment,
        route_params,
        recipient_fields,
    })
}

#[cfg(test)]
mod test {
    use bitcoin::{
        key::PublicKey,
        opcodes,
        script::{self, ScriptBuf},
        secp256k1,
    };
    use common::rng::{Crng, FastRng};

    use super::*;

    fn pubkey() -> PublicKey {
        let mut rng = FastRng::new();
        let secp_ctx = rng.gen_secp256k1_ctx_signing();
        let secret_key = secp256k1::SecretKey::from_slice(&[
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
        ])
        .unwrap();
        PublicKey::new(secp256k1::PublicKey::from_secret_key(
            &secp_ctx,
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
