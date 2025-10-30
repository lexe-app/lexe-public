use std::{
    collections::HashMap, convert::Infallible, num::NonZeroU64, ops::Deref,
    sync::RwLock, time::Duration,
};

use anyhow::{Context, anyhow, bail, ensure};
use bitcoin::hashes::{Hash as _, sha256};
use common::{
    api::{
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            RevocableClient, RevocableClients, UpdateClientRequest,
            UpdateClientResponse,
        },
        user::{NodePk, Scid, UserPk},
    },
    cli::{LspFees, LspInfo},
    constants, debug_panic_release_log, ed25519,
    enclave::Measurement,
    ln::{
        amount::Amount,
        channel::{LxChannelDetails, LxChannelId, LxUserChannelId},
        network::LxNetwork,
        route::LxRoute,
    },
    rng::SysRng,
    time::TimestampMs,
};
use either::Either;
use futures::Future;
use lexe_api::{
    models::command::{
        CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
        CreateOfferRequest, CreateOfferResponse, ListChannelsResponse,
        NodeInfo, OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
        PayOfferRequest, PayOfferResponse, PayOnchainRequest,
        PayOnchainResponse, PreflightCloseChannelRequest,
        PreflightCloseChannelResponse, PreflightOpenChannelRequest,
        PreflightOpenChannelResponse, PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse, PreflightPayOfferRequest,
        PreflightPayOfferResponse, PreflightPayOnchainRequest,
        PreflightPayOnchainResponse, ResyncRequest,
    },
    rest::API_REQUEST_TIMEOUT,
    types::{
        Empty,
        invoice::LxInvoice,
        offer::{LxOffer, MaxQuantity},
        payments::{LxPaymentId, PaymentDirection},
    },
    vfs::{REVOCABLE_CLIENTS_FILE_ID, Vfs},
};
use lexe_std::{Apply, const_assert};
use lexe_tls::{
    shared_seed::certs::{RevocableClientCert, RevocableIssuingCaCert},
    types::LxCertificateDer,
};
use lexe_tokio::events_bus::{EventsBus, EventsRx};
use lightning::{
    chain::{
        chaininterface::{ConfirmationTarget, FeeEstimator},
        chainmonitor::LockedChannelMonitor,
    },
    ln::{
        channel_state::ChannelDetails,
        channelmanager::{
            PaymentId, RecipientOnionFields, RetryableSendFailure,
        },
        msgs::RoutingMessageHandler,
        types::ChannelId,
    },
    routing::{gossip::NodeId, router::RouteParameters},
    sign::{NodeSigner, Recipient},
    types::payment::PaymentHash,
    util::config::UserConfig,
};
use lightning_invoice::{Bolt11Invoice, Currency, InvoiceBuilder};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, instrument, warn};

use crate::{
    alias::{LexeChainMonitorType, NetworkGraphType, RouterType, SignerType},
    balance,
    channel::ChannelEvent,
    esplora::FeeEstimates,
    keys_manager::LexeKeysManager,
    payments::{
        Payment,
        inbound::InboundInvoicePayment,
        manager::PaymentsManager,
        outbound::{
            LxOutboundPaymentFailure, OUTBOUND_PAYMENT_RETRY_STRATEGY,
            OutboundInvoicePayment, OutboundOfferPayment,
        },
    },
    route::{self, LastHopHint, RoutingContext},
    sync::BdkSyncRequest,
    traits::{LexeChannelManager, LexePeerManager, LexePersister},
    tx_broadcaster::TxBroadcaster,
    wallet::{LexeWallet, UtxoCounts},
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
pub fn node_info<CM, PM, PS, RMH>(
    version: semver::Version,
    measurement: Measurement,
    user_pk: UserPk,
    channel_manager: &CM,
    peer_manager: &PM,
    wallet: &LexeWallet,
    chain_monitor: &LexeChainMonitorType<PS>,
    channels: &[ChannelDetails],
    lsp_fees: LspFees,
) -> NodeInfo
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS, RMH>,
    PS: LexePersister,
    RMH: Deref,
    RMH::Target: RoutingMessageHandler,
{
    let node_pk = NodePk(channel_manager.get_our_node_id());

    let num_peers = peer_manager.list_peers().len();

    let num_channels: usize = channels.len();

    let (lightning_balance, num_usable_channels) =
        balance::all_channel_balances(chain_monitor, channels, lsp_fees);

    let onchain_balance = wallet.get_balance();

    let utxo_counts = wallet.get_utxo_counts();
    let UtxoCounts {
        total: num_utxos,
        confirmed: num_confirmed_utxos,
        unconfirmed: num_unconfirmed_utxos,
    } = utxo_counts;

    let best_block_height = channel_manager.current_best_block().height;

    let pending_monitor_updates = chain_monitor
        .list_pending_monitor_updates()
        .values()
        .map(|v| v.len())
        .sum();

    NodeInfo {
        version,
        measurement,
        user_pk,
        node_pk,
        num_channels,
        num_usable_channels,
        lightning_balance,
        num_peers,
        onchain_balance,
        num_utxos,
        num_confirmed_utxos,
        num_unconfirmed_utxos,
        best_block_height,
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
    channel_events_bus: &EventsBus<ChannelEvent>,
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
    channel_events_rx: &mut EventsRx<'_, ChannelEvent>,
    is_jit_channel: bool,
    user_channel_id: &LxUserChannelId,
) -> anyhow::Result<OpenChannelResponse> {
    let channel_event = channel_events_rx
        .recv_filtered(|event| {
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
    channel_events_bus: &EventsBus<ChannelEvent>,
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

    // Start subscribing to `ChannelEvent`s.
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
    channel_events_rx
        .recv_filtered(|event| {
            matches!(
                event,
                ChannelEvent::Closed { channel_id, .. }
                    if channel_id == &lx_channel_id,
            )
        })
        .apply(|fut| tokio::time::timeout(Duration::from_secs(15), fut))
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
    req: ResyncRequest,
    bdk_resync_tx: &mpsc::Sender<BdkSyncRequest>,
    ldk_resync_tx: &mpsc::Sender<oneshot::Sender<()>>,
) -> anyhow::Result<Empty> {
    /// How long we'll wait to hear a callback before giving up.
    // NOTE: Our default reqwest::Client timeout is 30 seconds.
    const SYNC_TIMEOUT: Duration = Duration::from_secs(27);
    const_assert!(SYNC_TIMEOUT.as_millis() < API_REQUEST_TIMEOUT.as_millis());

    let (bdk_tx, bdk_rx) = oneshot::channel();
    let bdk_sync_req = BdkSyncRequest {
        full_sync: req.full_sync,
        tx: bdk_tx,
    };
    bdk_resync_tx
        .try_send(bdk_sync_req)
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

    debug!("resync successful");
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
    info!("Handling create_invoice command for {amount:?} msats");

    let cltv_expiry = match caller {
        CreateInvoiceCaller::UserNode { .. } =>
            crate::constants::USER_MIN_FINAL_CLTV_EXPIRY_DELTA,
        CreateInvoiceCaller::Lsp =>
            crate::constants::LSP_MIN_FINAL_CLTV_EXPIRY_DELTA,
    };

    // Ensure that description and description_hash are mutually
    // exclusive. rust-lightning crate enforces this constraint.
    if req.description.is_some() && req.description_hash.is_some() {
        return Err(anyhow!(
            "Cannot specify both description and description_hash"
        ));
    }

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

    // TODO(phlip9): set maximum invoice expiry duration
    let expiry_time = Duration::from_secs(u64::from(req.expiry_secs));
    let our_node_pk = channel_manager.get_our_node_id();

    // Add most parts of the invoice, except for the route hints.
    // This is modeled after lightning_invoice's internal utility function
    // _create_invoice_from_channelmanager_and_duration_since_epoch_with_payment_hash
    let builder = InvoiceBuilder::new(currency); // <D, H, T, C, S>
    #[rustfmt::skip]
    let builder = if let Some(description_hash) = req.description_hash {
        let description_hash = sha256::Hash::from_slice(&description_hash)
            .expect("Should never fail with [u8;32]");
        builder.description_hash(description_hash)               // D: False -> True
    } else {
        builder.description(req.description.unwrap_or_default()) // D: False -> True
    };

    #[rustfmt::skip] // Nicer for the generic annotations to be aligned
    let mut builder = builder
        .payment_hash(sha256_hash)                               // H: False -> True
        .current_timestamp()                                     // T: False -> True
        .min_final_cltv_expiry_delta(u64::from(cltv_expiry))     // C: False -> True
        .payment_secret(secret)                                  // S: False -> True
        .basic_mpp()                                             // S: _ -> True
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

            // If there are multiple intercept scids, just pick the last one, as
            // it is likely the most recently generated.
            let intercept_scid = intercept_scids
                .last()
                .context("No intercept SCID provided")
                .inspect_err(|err| debug_panic_release_log!("{err:#}"))?;

            // Build the last hop hint for the payer to route with.
            let last_hop_hint =
                LastHopHint::new(&lsp_info, *intercept_scid, &channels);
            last_hop_hint.invoice_route_hints()
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
    network_graph: &NetworkGraphType,
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
        ..
    } = preflight_pay_invoice_inner(
        req,
        router,
        channel_manager,
        payments_manager,
        network_graph,
        chain_monitor,
        lsp_fees,
    )
    .await?;
    let hash = payment.hash;

    let payment = Payment::from(payment);
    let id = payment.id();
    let created_at = payment.created_at();

    // Pre-flight looks good, now we can register this payment in the Lexe
    // payments manager.
    payments_manager
        .new_payment(payment)
        .await
        .context("Already tried to pay this invoice")?;

    // TODO(phlip9): handle the case where we crash here before the channel
    // manager persists. We'll be left with a payment that gets stuck `Pending`
    // -> `Abandoning` forever, since the `channel_manager` doesn't know about
    // it and so won't emit a `PaymentFailed` event to finalize.
    //
    // Maybe we can check `channel_manager.list_recent_payments()` sometime
    // after startup to see if we have any `Pending` or `Abandoning` LN payments
    // that aren't tracked by the CM?

    // NOTE(phlip9): we rely on `payment_id == payment_hash` to disambiguate
    // invoice/spontaneous payments from offer payments.
    let payment_id = PaymentId::from(hash);

    // Send the payment, letting LDK handle payment retries, and match on the
    // result, registering a failure with the payments manager if appropriate.
    match channel_manager.send_payment(
        PaymentHash::from(hash),
        recipient_fields,
        payment_id,
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
            // perhaps there was a TOCTOU race? Regardless, if this variant is
            // returned, LDK does not track the payment and thus will not emit a
            // PaymentFailed later, so we should fail the payment now.
            payments_manager
                .payment_failed(id, LxOutboundPaymentFailure::Expired)
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
                .payment_failed(id, LxOutboundPaymentFailure::NoRoute)
                .await
                .context("(RouteNotFound) Could not register failure")?;
            Err(anyhow!("LDK returned RouteNotFound (OIP {hash})"))
        }
        Err(RetryableSendFailure::OnionPacketSizeExceeded) => {
            // If the metadata causes us to exceed the maximum onion packet
            // size, it probably isn't possible to pay this. Fail the payment.
            payments_manager
                .payment_failed(id, LxOutboundPaymentFailure::MetadataTooLarge)
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
    network_graph: &NetworkGraphType,
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
        network_graph,
        chain_monitor,
        lsp_fees,
    )
    .await?;
    Ok(PreflightPayInvoiceResponse {
        amount: preflight.payment.amount,
        fees: preflight.payment.fees,
        route: preflight.route,
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
        expires_at.to_duration()
    });

    // Create the initial `OfferBuilder` with:
    // + the given `absolute_expiry` deadline (if any).
    // + a blinded message path to us. built via
    //   `LexeMessageRouter::create_blinded_paths`.
    // + automatically derived offer metadata and signing keys.
    // + the given `max_quantity` (if unset, defaults to 1).
    let mut builder = channel_manager
        .create_offer_builder(absolute_expiry)
        .map_err(|err| anyhow!("Failed to create offer builder: {err:?}"))?
        .supported_quantity(
            req.max_quantity.unwrap_or(MaxQuantity::ONE).into(),
        );

    // TODO(phlip9): don't add `chains` param when mainnet to save space

    // TODO(phlip9): can we condense the blinded path? default offer is ~489B.
    // right now the default offer blinded path has
    //   + ~  54B (1) introductory node pk (LSP NodePk, clear)
    //   + ~  54B (2) blinding point pk
    //   + ~  54B (3) blinded issuer signing pk (user NodePk, blinded)
    //   + ~ 137B (4) hop 1: blinded pk + 51 B encrypted payload
    //   + ~ 137B (5) hop 2: blinded pk + 51 B encrypted payload
    //   = ~ 436B blinded path overhead

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

#[instrument(skip_all, name = "(pay-offer)")]
pub async fn pay_offer<CM, PS>(
    req: PayOfferRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    network_graph: &NetworkGraphType,
    lsp_fees: LspFees,
    lsp_node_pk: &NodePk,
) -> anyhow::Result<PayOfferResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Pre-flight the offer payment (verify and partially route).
    let PreflightedPayOffer { payment, route: _ } = preflight_pay_offer_inner(
        req,
        router,
        channel_manager,
        payments_manager,
        chain_monitor,
        network_graph,
        lsp_fees,
        lsp_node_pk,
    )
    .await?;

    // TODO(phlip9): user should choose whether to show their note to recipient
    let payer_note = None;
    // Use default
    let max_total_routing_fee_msat = None;

    // TODO(phlip9): payments manager should return this
    let created_at = payment.created_at;
    let id = payment.id();

    // Pre-flight looks good, now we can register this payment in the Lexe
    // payments manager.
    payments_manager
        .new_payment(Payment::OutboundOffer(payment.clone()))
        .await
        .context("Already tried to pay this offer")?;

    // Instruct the LDK channel manager to pay this offer, letting LDK handle
    // fetching the BOLT12 Invoice, routing, and retrying.
    let result = channel_manager.pay_for_offer(
        &payment.offer.0,
        payment.quantity.map(NonZeroU64::get),
        Some(payment.amount.msat()),
        payer_note,
        payment.ldk_id(),
        OUTBOUND_PAYMENT_RETRY_STRATEGY,
        max_total_routing_fee_msat,
    );

    // Channel manager returned an error
    if let Err(err) = result {
        use lightning::offers::parse::Bolt12SemanticError as LdkErr;

        use crate::payments::outbound::LxOutboundPaymentFailure as LxErr;
        let reason = match err {
            // This should never happen, since we already checked for this
            // payment id in the payments manager.
            LdkErr::DuplicatePaymentId => {
                debug_panic_release_log!(
                    "LDK believes offer payment is a duplicate, but we don't"
                );
                LxErr::LexeErr
            }
            // Should be very rare, but may be a TOCTOU issue
            LdkErr::AlreadyExpired => LxErr::Expired,
            // Offer uses unknown features
            LdkErr::UnknownRequiredFeatures => LxErr::UnknownFeatures,
            // LDK didn't like something about the offer
            _ => LxErr::InvalidOffer,
        };

        // Fail the payment
        payments_manager
            .payment_failed(id, reason)
            .await
            .context("Could not register failure")?;

        return Err(anyhow!("Invalid offer: {err:?}"));
    }

    info!("Success: outbound offer payment initiated");
    return Ok(PayOfferResponse { created_at });
}

#[instrument(skip_all, name = "(preflight-pay-offer)")]
pub async fn preflight_pay_offer<CM, PS>(
    req: PreflightPayOfferRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    network_graph: &NetworkGraphType,
    lsp_fees: LspFees,
    lsp_node_pk: &NodePk,
) -> anyhow::Result<PreflightPayOfferResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let req = PayOfferRequest {
        cid: req.cid,
        offer: req.offer,
        fallback_amount: req.fallback_amount,
        // User note not relevant for pre-flight.
        note: None,
    };
    let PreflightedPayOffer { payment, route } = preflight_pay_offer_inner(
        req,
        router,
        channel_manager,
        payments_manager,
        chain_monitor,
        network_graph,
        lsp_fees,
        lsp_node_pk,
    )
    .await?;
    Ok(PreflightPayOfferResponse {
        amount: payment.amount,
        fees: payment.fees,
        route,
    })
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
    route: LxRoute,
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
    network_graph: &NetworkGraphType,
    chain_monitor: &LexeChainMonitorType<PS>,
    lsp_fees: LspFees,
) -> anyhow::Result<PreflightedPayInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let invoice = req.invoice;

    // Fail early if invoice is expired.
    ensure!(!invoice.is_expired(), "Invoice has expired");

    // Fail early if we already tried paying this invoice,
    // or we are trying to pay ourselves (yes, users actually do this).
    let payment_id = LxPaymentId::Lightning(invoice.payment_hash());
    let maybe_existing_payment = payments_manager
        .get_payment(&payment_id)
        .await
        .context("Couldn't check for existing payment")?;
    if let Some(existing_payment) = maybe_existing_payment {
        match existing_payment.direction() {
            PaymentDirection::Outbound =>
                return Err(anyhow!("We've already tried paying this invoice")),
            // Yes, users actually hit this case...
            PaymentDirection::Inbound => bail!("We cannot pay ourselves"),
        }
    }

    // Compute the amount; handle amountless invoices.
    let amount = validate::pay_amount(
        "invoices",
        invoice.amount(),
        req.fallback_amount,
    )?;

    // Compute Lightning balances
    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let (lightning_balance, num_usable_channels) =
        balance::all_channel_balances(chain_monitor, &channels, lsp_fees);

    // Check that the user has at least one usable channel.
    validate::has_usable_channels(num_channels, num_usable_channels)?;

    // Construct payment parameters, which are amount-agnostic.
    let payment_params = route::build_payment_params(
        Either::Right(&invoice),
        Some(num_usable_channels),
    )
    .context("Couldn't build payment parameters")?;

    // Construct payment routing context
    let routing_context =
        RoutingContext::from_payment_params(channel_manager, payment_params);

    // Check that the amount is OK wrt `max_sendable`.
    validate::max_sendable_ok(
        router,
        &routing_context,
        amount,
        &lightning_balance,
    )
    .await?;

    // Try to find a Route with the full intended amount.
    let (route, route_params) = validate::can_route_amount(
        router,
        network_graph,
        &routing_context,
        amount,
    )
    .await?;

    let payment_secret = invoice.payment_secret().into();
    let recipient_fields = RecipientOnionFields::secret_only(payment_secret);

    let amount = route.amount();
    let fees = route.fees();
    let payment = OutboundInvoicePayment::new(invoice, amount, fees, req.note);
    Ok(PreflightedPayInvoice {
        payment,
        route,
        route_params,
        recipient_fields,
    })
}

/// An outbound offer payment that we preflighted (validated and routed) but
/// haven't paid yet.
struct PreflightedPayOffer {
    payment: OutboundOfferPayment,
    route: LxRoute,
}

async fn preflight_pay_offer_inner<CM, PS>(
    req: PayOfferRequest,
    router: &RouterType,
    channel_manager: &CM,
    payments_manager: &PaymentsManager<CM, PS>,
    chain_monitor: &LexeChainMonitorType<PS>,
    network_graph: &NetworkGraphType,
    lsp_fees: LspFees,
    lsp_node_pk: &NodePk,
) -> anyhow::Result<PreflightedPayOffer>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let offer = req.offer;

    // Fail early if offer is expired.
    ensure!(!offer.is_expired(), "Offer has expired");

    // We only support paying BTC-denominated offers at the moment.
    ensure!(
        !offer.is_fiat_denominated(),
        "Fiat-denominated offers are not supported yet"
    );

    // Fail early if we already tried paying with this client ID.
    let payment_id = LxPaymentId::OfferSend(req.cid);
    let maybe_existing_payment = payments_manager
        .get_payment(&payment_id)
        .await
        .context("Couldn't check for existing payment")?;
    ensure!(
        maybe_existing_payment.is_none(),
        "Detected duplicate attempt trying to pay this offer. \
         Please refresh and try again."
    );

    // TODO(phlip9): support user choosing quantity. For now just assume
    // quantity=1, but in a way that works.
    let quantity = if offer.expects_quantity() {
        Some(const { NonZeroU64::new(1).unwrap() })
    } else {
        None
    };
    // TODO(phlip9): actual_amount = amount * quantity
    // TODO(phlip9): support over-paying offers? `offer_amount` is actually a
    //               _minimum_ amount and the spec allows over-paying.
    // Compute the amount; handle amountless offers.
    let amount =
        validate::pay_amount("offers", offer.amount(), req.fallback_amount)?;

    // Compute Lightning balances
    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let (lightning_balance, num_usable_channels) =
        balance::all_channel_balances(chain_monitor, &channels, lsp_fees);

    // Check that the user has at least one usable channel.
    validate::has_usable_channels(num_channels, num_usable_channels)?;

    // Construct payment parameters, which are amount-agnostic.
    //
    // Since we don't fetch the BOLT12 invoice in preflight yet, we can only
    // simulate routing to the first public node in the offer. We also don't
    // have the blinded path info yet, so this is clearly imperfect as we don't
    // know the full routing fees, min/max htlc, etc...
    let route_target = offer
        .preflight_routable_node(&network_graph.read_only(), lsp_node_pk)?;
    let payment_params = route::build_payment_params(
        Either::Left(route_target),
        Some(num_usable_channels),
    )
    .context("Couldn't build payment parameters")?;

    // Construct payment routing context
    let routing_context =
        RoutingContext::from_payment_params(channel_manager, payment_params);

    // Check that the amount is OK wrt `max_sendable`.
    validate::max_sendable_ok(
        router,
        &routing_context,
        amount,
        &lightning_balance,
    )
    .await?;

    // Try to find a Route with the full intended amount (well, to the first
    // publicly routable node so this will underestimate the route cost by
    // whatever the blinded hops charge).
    let (route, _route_params) = validate::can_route_amount(
        router,
        network_graph,
        &routing_context,
        amount,
    )
    .await?;

    let amount = route.amount();
    let fees = route.fees();
    let payment = OutboundOfferPayment::new(
        req.cid, offer, amount, quantity, fees, req.note,
    );
    Ok(PreflightedPayOffer { payment, route })
}

/// Payments preflight validation helpers.
mod validate {
    use common::ln::balance::LightningBalance;

    use super::*;

    /// Get the final amount we should pay for an invoice/offer, accounting for
    /// amountless invoices/offers with `fallback_amount` set/unset.
    pub(super) fn pay_amount(
        kind: &'static str,
        amount: Option<Amount>,
        fallback_amount: Option<Amount>,
    ) -> anyhow::Result<Amount> {
        if amount.is_some() && fallback_amount.is_some() {
            // Not a serious error, but better to be unambiguous.
            debug_panic_release_log!(
                "Nit: Only provide fallback amount for amountless {kind}",
            );
        }

        amount.or(fallback_amount).with_context(|| {
            format!("Missing fallback amount for amountless {kind}")
        })
    }

    /// Check that the user has at least one usable channel.
    pub(super) fn has_usable_channels(
        num_channels: usize,
        num_usable_channels: usize,
    ) -> anyhow::Result<()> {
        if num_channels == 0 {
            // Noob error: Take the opportunity to teach the user about LN
            // channels.
            return Err(anyhow!(
                "You don't have any Lightning channels, which are required \
                 to send funds. Consider opening a new channel using on-chain \
                 funds, or receiving funds to your wallet via Lightning."
            ));
        }
        if num_usable_channels == 0 {
            // The user has at least one channel, but none of them are usable.
            // Maybe their channel is being closed or something.
            return Err(anyhow!(
                "You don't have any usable Lightning channels. Consider \
                 opening a new channel using on-chain funds, or receiving \
                 funds to your wallet directly via Lightning."
            ));
        }
        Ok(())
    }

    /// Check various bounds on the amount w.r.t. `max_sendable`:
    /// - They don't have a zero usable Lightning balance, in which case they
    ///   can't send anything.
    /// - If their usable balance is non-zero but `max_sendable` is zero, return
    ///   an error telling them their funds are in the channel reserve.
    /// - If the amount they're trying to send is greater than `max_sendable`,
    ///   return an error telling them the maximum they can send.
    pub(super) async fn max_sendable_ok(
        router: &RouterType,
        routing_context: &RoutingContext,
        amount: Amount,
        lightning_balance: &LightningBalance,
    ) -> anyhow::Result<()> {
        let max_sendable = lightning_balance.max_sendable;
        let usable_lightning_balance = lightning_balance.usable;

        // If the user has no usable Lightning balance, they can't send
        // anything. Not sure how the user can even get here tbh.
        if usable_lightning_balance == Amount::ZERO {
            warn!("User has usable channels but zero usable balance?");
            return Err(anyhow!(
                "Insufficient balance: You have no usable Lightning balance. \
                 Add to your Lightning balance in order to send payments.",
            ));
        }

        if amount <= max_sendable {
            return Ok(());
        }

        // If they have a balance but not enough to exceed the channel reserve,
        // return a dedicated error message, instead of "cannot find route".
        if usable_lightning_balance > Amount::ZERO
            && max_sendable == Amount::ZERO
        {
            return Err(anyhow!(
                "Insufficient balance: Tried to pay {amount} sats, but all of \
                 your Lightning balance is tied up in the channel reserve. \
                 Consider adding to your Lightning balance in order to send \
                 this amount.",
            ));
        }

        // Since we know the recipient, we can compute a more accurate maximum
        // sendable amount to this recipient (i.e. max flow) and expose that to
        // the user as a suggestion.
        //
        // TODO(max): We should also calculate the max flow from the *LSP*, so
        // we can tell whether (1) `max_flow` is limited by the user's balance
        // or (2) there simply isn't enough liquidity from LSP to recipient.
        let max_flow_result = route::compute_max_flow_to_recipient(
            router,
            routing_context,
            amount,
        )
        .await;

        match max_flow_result {
            Ok(max_flow) => Err(anyhow!(
                "Insufficient balance: Tried to pay {amount} sats, but the \
                 maximum amount you can send is {max_sendable} sats, after 
                 accounting for the channel reserve. The maximum amount that \
                 you can route to this recipient is {max_flow} sats. Consider \
                 adding to your Lightning balance or sending a smaller amount.",
            )),
            Err(e) => Err(anyhow!(
                "Couldn't route to this recipient with any amount: {e:#}"
            )),
        }
    }

    // Ensure we can find a Route with the full intended amount.
    pub(super) async fn can_route_amount(
        router: &RouterType,
        network_graph: &NetworkGraphType,
        routing_context: &RoutingContext,
        amount: Amount,
    ) -> anyhow::Result<(LxRoute, RouteParameters)> {
        let route_result = routing_context.find_route(router, amount);
        let (route, route_params) = match route_result {
            Ok((r, p)) => (r, p),
            // This error is just "Failed to find a path to the given
            // destination", which is not helpful, so we don't include it in our
            // error message.
            Err(_) => {
                // We couldn't find a route with the full intended amount.
                // But since we know the recipient, we can compute a more
                // accurate maximum sendable amount to this recipient
                // (i.e. max flow).
                let max_flow_result = route::compute_max_flow_to_recipient(
                    router,
                    routing_context,
                    amount,
                )
                .await;

                let error = match max_flow_result {
                    Ok(max_flow) => {
                        // TODO(max): We should also calculate the max flow from
                        // the *LSP*, so we can tell whether (1) `max_flow` is
                        // limited by the user's balance or (2) there simply
                        // isn't enough liquidity from the LSP to the recipient.
                        //
                        // This call to action could then be one of:
                        // 1) "You must add to your Lightning balance in order
                        //    to send this amount to this recipient"
                        // 2) "Consider sending a smaller amount or asking the
                        //    recipient to increase their inbound liquidity."
                        anyhow!(
                            "Tried to pay {amount} sats. The maximum amount \
                             that you can route to this recipient is {max_flow} \
                             sats, after accounting for the channel reserve. \
                             Consider adding to your Lightning balance or \
                             sending a smaller amount.",
                        )
                    }
                    Err(e) => anyhow!(
                        "Couldn't route to this recipient with any amount: {e:#}"
                    ),
                };

                return Err(error);
            }
        };

        let route = LxRoute::from_ldk(route, network_graph);
        // TODO(max): Don't log for privacy; instead, expose in app.
        info!("Preflighted route: {route}");
        Ok((route, route_params))
    }
}

#[instrument(skip_all, name = "(create-revocable-client)")]
pub async fn create_revocable_client(
    persister: &impl LexePersister,
    eph_ca_cert_der: LxCertificateDer,
    rev_ca_cert: &RevocableIssuingCaCert,
    revocable_clients: &RwLock<RevocableClients>,
    req: CreateRevocableClientRequest,
) -> anyhow::Result<CreateRevocableClientResponse> {
    let mut rng = SysRng::new();

    if let Some(label) = &req.label
        && label.len() > RevocableClient::MAX_LABEL_LEN
    {
        return Err(anyhow!(
            "Label must not be longer than {} bytes",
            RevocableClient::MAX_LABEL_LEN
        ));
    }

    // TODO(max): Might want some logic on req.scope here,
    // e.g. the caller can't assign a more permissive scope than its own scope,
    // and most clients shouldn't have the ability to create clients.

    let rev_client_cert = RevocableClientCert::generate_from_rng(&mut rng);
    let pubkey = rev_client_cert.public_key();
    let now = TimestampMs::now();
    let revocable_client = RevocableClient {
        pubkey,
        created_at: now,
        expires_at: req.expires_at,
        label: req.label,
        scope: req.scope,
        is_revoked: false,
    };

    let rev_client_cert_der = rev_client_cert
        .serialize_der_ca_signed(rev_ca_cert)
        .context("Failed to serialize revocable client cert")?;
    let rev_client_cert_key_der = rev_client_cert.serialize_key_der();

    let updated_file = {
        let mut revocable_clients = revocable_clients.write().unwrap();

        // We don't allow more than `MAX_LEN` clients for DoS reasons. We also
        // don't delete revoked clients immediately. If we're above the limit,
        // we'll prune the oldest revoked client(s).
        maybe_evict_revoked_clients(
            &mut revocable_clients.clients,
            now,
            RevocableClients::MAX_LEN,
        )?;

        let existing =
            revocable_clients.clients.insert(pubkey, revocable_client);

        if existing.is_some() {
            debug_panic_release_log!(
                "Somehow overwrote existing client {pubkey}"
            );
        }

        persister.encrypt_json::<RevocableClients>(
            REVOCABLE_CLIENTS_FILE_ID.clone(),
            &revocable_clients,
        )
    };

    let retries = 0;
    persister
        .persist_file(updated_file, retries)
        .await
        .context("Failed to persisted updated RevocableClients")?;

    Ok(CreateRevocableClientResponse {
        pubkey,
        created_at: now,
        eph_ca_cert_der: eph_ca_cert_der.0,
        rev_client_cert_der: rev_client_cert_der.0,
        rev_client_cert_key_der: rev_client_cert_key_der.0,
    })
}

/// Evicts old revoked/expired clients if we have too many. Returns an `Err`
/// if we have too many clients and can't evict any.
fn maybe_evict_revoked_clients(
    clients: &mut HashMap<ed25519::PublicKey, RevocableClient>,
    now: TimestampMs,
    limit: usize,
) -> anyhow::Result<()> {
    if clients.len() < limit {
        return Ok(());
    }

    // We'll almost never be at the limit, and if we are, we'll likely only
    // remove one client. However this while loop handles the case where we
    // reduce the limit and need to remove multiple clients.
    let target_len = limit.saturating_sub(1);
    while clients.len() > target_len {
        // Find the oldest revoked or expired clients
        let oldest_revoked_client = clients
            .values()
            .filter(|client| !client.is_valid_at(now))
            .min_by_key(|client| client.created_at)
            .map(|client| client.pubkey);

        match oldest_revoked_client {
            Some(pubkey) => {
                // Evict old, revoked client
                clients.remove(&pubkey).expect("Client should exist");
            }
            None =>
                return Err(anyhow!(
                    "Reached maximum # of API clients. For more clients, please \
                 contact Lexe to explain why you need more than {} clients.",
                    RevocableClients::MAX_LEN,
                )),
        }
    }

    Ok(())
}

/// Update an existing [`RevocableClient`] (revoke, set expiration, etc...).
#[instrument(skip_all, name = "(update-revocable-client)")]
pub async fn update_revocable_client(
    persister: &impl LexePersister,
    revocable_clients: &RwLock<RevocableClients>,
    req: UpdateClientRequest,
) -> anyhow::Result<UpdateClientResponse> {
    let (updated_file, response) = {
        let mut revocable_clients = revocable_clients.write().unwrap();

        // Get the client
        let pubkey = req.pubkey;
        let client = revocable_clients
            .clients
            .get_mut(&pubkey)
            .ok_or_else(|| anyhow!("No revocable client with pk {pubkey}"))?;

        // Update
        let updated_client = client.update(req)?;
        *client = updated_client.clone();
        let response = UpdateClientResponse {
            client: updated_client,
        };

        // Generate the new file
        let updated_file = persister.encrypt_json::<RevocableClients>(
            REVOCABLE_CLIENTS_FILE_ID.clone(),
            &revocable_clients,
        );

        (updated_file, response)
    };

    // NOTE: If persist fails, the persisted state will be out of sync until the
    // next successful persist (or until the next boot). Ideally we ensure
    // consistency by holding a `tokio::sync::RwLock` during the persist, but
    // `RevocableClients` is read in the `ClientCertVerifier` which is sync.
    // Since the in-memory struct represents the user's intention and is
    // sufficient to enforce the server's client policies, this is probably OK.
    // Using `Arc<tokio::sync::RwLock<Arc<RwLock<RevocableClients>>>>` or
    // similar doesn't seem worth the minimal consistency benefit.
    let retries = 0;
    persister
        .persist_file(updated_file, retries)
        .await
        .context("Failed to persisted updated RevocableClients")?;

    Ok(response)
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
    use proptest::proptest;

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

    #[test]
    fn test_maybe_evict_revoked_clients() {
        use proptest::{arbitrary::any, collection::vec};
        proptest!(|(
            clients in vec(any::<RevocableClient>(), 0..10),
            now: TimestampMs,
            limit in 0_usize..10,
        )| {
            // setup
            let mut clients = clients
                .into_iter()
                .map(|client| (client.pubkey, client))
                .collect::<HashMap<_, _>>();
            let num_before = clients.len();
            let num_evictable = clients
                .values()
                .filter(|client| !client.is_valid_at(now))
                .count();

            // evict bad clients
            let result = maybe_evict_revoked_clients(&mut clients, now, limit);

            // (rough) can't evict more than we expect
            let num_after = clients.len();
            let num_evicted = num_before - num_after;
            assert!(num_evicted <= num_evictable);

            // (precise) evict exactly what we expect
            let target_len = limit.saturating_sub(1);
            let expected_len = if num_before <= target_len {
                num_before
            } else {
                std::cmp::max(num_before - num_evictable, target_len)
            };
            assert_eq!(num_after, expected_len);

            // if we didn't evict enough, should return Err
            if num_after > target_len {
                result.unwrap_err();
            } else {
                result.unwrap();
            }
        });
    }
}
