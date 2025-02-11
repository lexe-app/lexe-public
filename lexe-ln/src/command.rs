use std::{convert::Infallible, time::Duration};

use anyhow::{anyhow, bail, Context};
use bitcoin_hashes::{sha256, Hash};
use common::{
    api::{
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            ListChannelsResponse, NodeInfo, OpenChannelResponse,
            PayInvoiceRequest, PayInvoiceResponse, PayOnchainRequest,
            PayOnchainResponse, PreflightCloseChannelRequest,
            PreflightCloseChannelResponse, PreflightOpenChannelRequest,
            PreflightOpenChannelResponse, PreflightPayInvoiceRequest,
            PreflightPayInvoiceResponse, PreflightPayOnchainRequest,
            PreflightPayOnchainResponse,
        },
        user::{NodePk, Scid},
        Empty,
    },
    cli::LspInfo,
    constants,
    enclave::Measurement,
    ln::{
        amount::Amount,
        channel::{LxChannelDetails, LxChannelId, LxUserChannelId},
        invoice::LxInvoice,
        network::LxNetwork,
    },
};
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
        PaymentHash,
    },
    routing::router::{PaymentParameters, RouteHint, RouteParameters, Router},
    sign::{NodeSigner, Recipient},
    util::config::UserConfig,
};
use lightning_invoice::{Bolt11Invoice, Currency, InvoiceBuilder};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, instrument};

use crate::{
    alias::{LexeChainMonitorType, RouterType, SignerType},
    balance,
    channel::{ChannelEvent, ChannelEventsBus, ChannelEventsRx},
    esplora::LexeEsplora,
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
    traits::{LexeChannelManager, LexePeerManager, LexePersister},
    wallet::LexeWallet,
};

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
        scid: Scid,
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
        balance::all_channel_balances(chain_monitor, channels);

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
pub fn list_channels<CM, PS>(
    channel_manager: &CM,
    chain_monitor: &LexeChainMonitorType<PS>,
) -> anyhow::Result<ListChannelsResponse>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let channels = channel_manager
        .list_channels()
        .into_iter()
        .map(|channel| {
            let channel_id = channel.channel_id;
            let channel_balance =
                balance::channel_balance(chain_monitor, &channel)?;
            LxChannelDetails::from_details_and_balance(channel, channel_balance)
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
    let channel_event = tokio::time::timeout(
        Duration::from_secs(15),
        channel_events_rx.next_filtered(|event| {
            if is_jit_channel {
                matches!(event,
                    ChannelEvent::Ready { .. } | ChannelEvent::Closed { .. }
                    if event.user_channel_id() == user_channel_id
                )
            } else {
                event.user_channel_id() == user_channel_id
            }
        }),
    )
    .await
    .context("Waiting for channel event")?;

    if let ChannelEvent::Closed { reason, .. } = channel_event {
        return Err(anyhow!("Channel open failed: {reason}"));
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
    esplora: &LexeEsplora,
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
    let fee_estimate = our_close_tx_fees_sats(esplora, &channel, monitor);
    let fee_estimate = Amount::try_from_sats_u64(fee_estimate)?;

    // TODO(phlip9): include est. blocks to confirmation? Esp. for force close.
    Ok(PreflightCloseChannelResponse { fee_estimate })
}

/// Calculate the fees _we_ have to pay to close this channel.
///
/// TODO(phlip9): support v2/anchor channels
fn our_close_tx_fees_sats(
    esplora: &LexeEsplora,
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
        let fee_sats = if our_sats <= constants::LDK_DUST_LIMIT_SATS {
            our_sats
        } else {
            0
        };
        return fee_sats;
    }

    // The current fees required for this close tx to confirm
    let tx_fees_sats = close_tx_fees_sats(esplora, channel);

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
    if our_sats <= constants::LDK_DUST_LIMIT_SATS {
        return tx_fees_sats + our_sats;
    }

    // Normally, we just pay the fees
    tx_fees_sats
}

/// Estimate the total on-chain fees for a channel close, which must be paid by
/// the channel funder.
fn close_tx_fees_sats(esplora: &LexeEsplora, channel: &ChannelDetails) -> u64 {
    let conf_target = ConfirmationTarget::NonAnchorChannelFee;
    let fee_sat_per_kwu =
        esplora.get_est_sat_per_1000_weight(conf_target) as u64;

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

    // Construct the route hints.
    let route_hints = match caller {
        // If the LSP is calling create_invoice, include no hints and let
        // the sender route to us by looking at the lightning network graph.
        CreateInvoiceCaller::Lsp => Vec::new(),
        // If a user node is calling create_invoice, always include just an
        // intercept hint. We do this even when the user already has a channel
        // with enough balance to service the payment because it allows the LSP
        // to intercept the HTLC and wake the user if a payment comes in while
        // the user is offline.
        CreateInvoiceCaller::UserNode { lsp_info, scid } =>
            vec![RouteHint(vec![lsp_info.route_hint_hop(scid)])],
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
    )
    .await?;
    Ok(PreflightPayInvoiceResponse {
        amount: preflight.payment.amount,
        fees: preflight.payment.fees,
    })
}

#[instrument(skip_all, name = "(pay-onchain)")]
pub async fn pay_onchain<CM, PS>(
    req: PayOnchainRequest,
    network: LxNetwork,
    wallet: &LexeWallet,
    esplora: &LexeEsplora,
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
    esplora
        .broadcast_tx(&tx)
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
) -> anyhow::Result<PreflightedPayInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let invoice = req.invoice;

    // Fail expired invoices early.
    if invoice.is_expired() {
        bail!("Invoice has expired");
    }

    // Fail invoice double-payment early.
    if payments_manager
        .contains_payment_id(&invoice.payment_id())
        .await
    {
        bail!("We've already tried paying this invoice");
    }

    // Construct a RouteParameters for the payment, modeled after how
    // `lightning_invoice::payment::pay_invoice_using_amount` does it.
    let payer_pubkey = channel_manager.get_our_node_id();
    let payee_pubkey = invoice.payee_node_pk().0;
    let amount = invoice
        .amount()
        .inspect(|_| {
            debug_assert!(
                req.fallback_amount.is_none(),
                "Nit: Fallback should only be provided for amountless invoices",
            )
        })
        .or(req.fallback_amount)
        .context("Missing fallback amount for amountless invoice")?;
    let expires_at = invoice.expires_at()?.into_duration().as_secs();

    // TODO(max): Support paying BOLT12 invoices
    let mut payment_params = PaymentParameters::from_node_id(
        payee_pubkey,
        invoice.min_final_cltv_expiry_delta_u32()?,
    )
    .with_expiry_time(expires_at)
    .with_route_hints(invoice.0.route_hints())
    .map_err(|()| anyhow!("(route hints) Wrong payment param kind"))?;

    if let Some(features) = invoice.0.features().cloned() {
        // TODO(max): Support paying BOLT12 invoices
        payment_params = payment_params
            .with_bolt11_features(features)
            .map_err(|()| anyhow!("(features) Wrong payment param kind"))?;
    }

    // TODO(max): We may want to set a fee limit at some point
    let max_total_routing_fee_msat = None;
    let route_params = RouteParameters {
        payment_params,
        final_value_msat: amount.msat(),
        max_total_routing_fee_msat,
    };

    // TODO(phlip9): need better error messages for simpler failure cases like
    // trying to send above User<->LSP channel max outbound HTLC limit, etc...
    //
    // Right now we just get "Could not find route to recipient", which is
    // completely useless and not actionable.
    //
    // More generally, we could also try to compute the Max-Flow to the
    // destination and suggest that value as an upper bound.

    // Find a Route so we can estimate the fees to be paid. Modeled after
    // `lightning::ln::outbound_payment::OutboundPayments::pay_internal`.
    let usable_channels = channel_manager.list_usable_channels();
    let refs_usable_channels = usable_channels.iter().collect::<Vec<_>>();
    let first_hops = Some(refs_usable_channels.as_slice());
    let in_flight_htlcs = channel_manager.compute_inflight_htlcs();
    let route = router
        .find_route(&payer_pubkey, &route_params, first_hops, in_flight_htlcs)
        .map_err(|e| anyhow!("Could not find route to recipient: {}", e.err))?;

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
