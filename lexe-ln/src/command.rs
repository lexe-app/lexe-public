use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{anyhow, bail, Context};
use bitcoin::{bech32::ToBase32, Address};
use bitcoin_hashes::{sha256, Hash};
use common::{
    api::{
        command::{
            CreateInvoiceRequest, NodeInfo, PayInvoiceRequest,
            SendOnchainRequest,
        },
        NodePk, Scid,
    },
    cli::{LspInfo, Network},
    ln::{
        amount::Amount, channel::LxChannelDetails, hashes::LxTxid,
        invoice::LxInvoice, payments::LxPaymentHash,
    },
};
use lightning::{
    chain::keysinterface::{NodeSigner, Recipient},
    ln::{
        channelmanager::{
            PaymentId, RetryableSendFailure, MIN_FINAL_CLTV_EXPIRY_DELTA,
        },
        PaymentHash,
    },
    routing::{
        gossip::RoutingFees,
        router::{
            PaymentParameters, RouteHint, RouteHintHop, RouteParameters, Router,
        },
    },
};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder};
use tokio::sync::broadcast;
use tracing::{debug, info, instrument, warn};

use crate::{
    alias::{LexeChainMonitorType, RouterType},
    esplora::LexeEsplora,
    keys_manager::LexeKeysManager,
    payments::{
        inbound::InboundInvoicePayment,
        manager::PaymentsManager,
        outbound::{OutboundInvoicePayment, OUTBOUND_PAYMENT_RETRY_STRATEGY},
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
    /// receiving a payment over a JIT channel with the LSP.
    UserNode {
        lsp_info: LspInfo,
        scid: Scid,
    },
    Lsp,
}

#[instrument(skip_all, name = "(node-info)")]
pub async fn node_info<CM, PM, PS>(
    channel_manager: CM,
    peer_manager: PM,
    wallet: LexeWallet,
    chain_monitor: Arc<LexeChainMonitorType<PS>>,
) -> anyhow::Result<NodeInfo>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    let node_pk = NodePk(channel_manager.get_our_node_id());

    let channels = channel_manager.list_channels();
    let num_channels = channels.len();
    let num_usable_channels = channels.iter().filter(|c| c.is_usable).count();

    let local_balance_msat = channels.iter().map(|c| c.balance_msat).sum();
    let local_balance = Amount::from_msat(local_balance_msat);
    let num_peers = peer_manager.get_peer_node_ids().len();

    let wallet_balance = wallet.get_balance().await?;

    let pending_monitor_updates = chain_monitor
        .list_pending_monitor_updates()
        .values()
        .map(|v| v.len())
        .sum();

    let info = NodeInfo {
        node_pk,
        num_channels,
        num_usable_channels,
        local_balance,
        num_peers,
        wallet_balance,
        pending_monitor_updates,
    };

    Ok(info)
}

pub fn list_channels<CM, PS>(channel_manager: CM) -> Vec<LxChannelDetails>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    channel_manager
        .list_channels()
        .into_iter()
        .map(LxChannelDetails::from)
        .collect::<Vec<_>>()
}

/// Uses the given `resync_tx` to retrigger BDK and LDK sync.
///
/// This function is intended to be used as a warp handler.
pub fn resync(resync_tx: broadcast::Sender<()>) -> anyhow::Result<()> {
    resync_tx
        .send(())
        .map(|_| ())
        .context("Failed to retrigger sync")
}

#[instrument(skip_all, name = "(create-invoice)")]
pub async fn create_invoice<CM, PS>(
    req: CreateInvoiceRequest,
    channel_manager: CM,
    keys_manager: Arc<LexeKeysManager>,
    payments_manager: PaymentsManager<CM, PS>,
    caller: CreateInvoiceCaller,
    network: Network,
) -> anyhow::Result<LxInvoice>
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
        .map_err(|()| {
            anyhow!("Supplied msat amount > total bitcoin supply!")
        })?;
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
        .description(req.description)                        // D: False -> True
        .payment_hash(sha256_hash)                           // H: False -> True
        .current_timestamp()                                 // T: False -> True
        .min_final_cltv_expiry_delta(u64::from(cltv_expiry)) // C: False -> True
        .payment_secret(secret)                              // S: False -> True
        .basic_mpp()                                         // S: _ -> True
        .expiry_time(expiry_time)
        .payee_pub_key(our_node_pk);
    if let Some(amount) = req.amount {
        builder = builder.amount_milli_satoshis(amount.msat());
    }

    // Add the route hints.
    let route_hints = get_route_hints(channel_manager, caller, req.amount);
    for hint in route_hints {
        builder = builder.private_route(hint);
    }

    // Build, sign, and return the invoice
    let raw_invoice =
        builder.build_raw().context("Could not build raw invoice")?;
    let hr_part_str = raw_invoice.hrp.to_string();
    let data_part_base32 = raw_invoice.data.to_base32();
    let recipient = Recipient::Node;
    let signed_raw_invoice = raw_invoice
        .sign(|_| {
            keys_manager
                .sign_invoice(
                    hr_part_str.as_bytes(),
                    &data_part_base32,
                    recipient,
                )
                .map_err(|()| anyhow!("Failed to sign invoice"))
        })
        .context("Failed to sign invoice")?;
    let invoice = Invoice::from_signed(signed_raw_invoice)
        .map(LxInvoice)
        .context("Invoice was semantically incorrect")?;

    let payment = InboundInvoicePayment::new(
        invoice.clone(),
        hash.into(),
        secret.into(),
        preimage.into(),
    );
    payments_manager
        .new_payment(payment)
        .await
        .context("Could not register new payment")?;

    info!("Success: Generated invoice {invoice}");

    Ok(invoice)
}

#[instrument(skip_all, name = "(pay-invoice)")]
pub async fn pay_invoice<CM, PS>(
    req: PayInvoiceRequest,
    router: Arc<RouterType>,
    channel_manager: CM,
    payments_manager: PaymentsManager<CM, PS>,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Abort and don't even save the payment if the invoice has already expired.
    // BOLT11: "A payer: after the timestamp plus expiry has passed: SHOULD NOT
    // attempt a payment."
    let invoice = req.invoice.0;
    if invoice.is_expired() {
        bail!("Invoice has already expired");
    }

    // Construct a RouteParameters for the payment, modeled after how
    // `lightning_invoice::payment::pay_invoice_using_amount` does it.
    let payer_pubkey = channel_manager.get_our_node_id();
    let payee_pubkey = invoice
        .payee_pub_key()
        .cloned()
        .unwrap_or_else(|| invoice.recover_payee_pub_key());
    let final_value_msat = invoice
        .amount_milli_satoshis()
        .inspect(|_| {
            debug_assert!(
                req.fallback_amount.is_none(),
                "Nit: Fallback should only be provided for amountless invoices",
            )
        })
        .or(req.fallback_amount.map(|amt| amt.msat()))
        .context("Missing fallback amount for amountless invoice")?;
    let final_cltv_expiry_delta =
        u32::try_from(invoice.min_final_cltv_expiry_delta())
            .context("Min final CLTV expiry delta too large to fit in u32")?;
    let expires_at = invoice
        .timestamp()
        .checked_add(invoice.expiry_time())
        .context("Computing expiry time overflowed")?;
    let expires_at_timestamp = expires_at
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("Invalid invoice expiration")?
        .as_secs();
    let mut payment_params =
        PaymentParameters::from_node_id(payee_pubkey, final_cltv_expiry_delta)
            .with_expiry_time(expires_at_timestamp)
            .with_route_hints(invoice.route_hints());
    if let Some(features) = invoice.features().cloned() {
        payment_params = payment_params.with_features(features);
    }
    let route_params = RouteParameters {
        payment_params,
        final_value_msat,
    };

    // Find a Route so we can estimate the fees to be paid. Modeled after
    // `lightning::ln::outbound_payment::OutboundPayments::pay_internal`.
    let usable_channels = channel_manager.list_usable_channels();
    let refs_usable_channels = usable_channels.iter().collect::<Vec<_>>();
    let first_hops = Some(refs_usable_channels.as_slice());
    let in_flight_htlcs = channel_manager.compute_inflight_htlcs();
    let route = router
        .find_route(&payer_pubkey, &route_params, first_hops, &in_flight_htlcs)
        .map_err(|e| anyhow!("Could not find route to recipient: {}", e.err))?;

    // Extract a few more values needed later before we consume the Invoice.
    let payment_hash = PaymentHash(invoice.payment_hash().into_inner());
    let payment_secret = Some(*invoice.payment_secret());
    let payment_id = PaymentId(payment_hash.0);
    let hash = LxPaymentHash::from(payment_hash);

    // Create and register the new payment, checking that it is unique.
    let payment = OutboundInvoicePayment::new(invoice, &route, req.note);
    payments_manager
        .new_payment(payment)
        .await
        .context("Already tried to pay this invoice")?;

    // Send the payment, letting LDK handle payment retries, and match on the
    // result, registering a failure with the payments manager if appropriate.
    match channel_manager.send_payment_with_retry(
        payment_hash,
        &payment_secret,
        payment_id,
        route_params,
        OUTBOUND_PAYMENT_RETRY_STRATEGY,
    ) {
        Ok(()) => Ok(info!(%hash, "Success: OIP initiated immediately")),
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
                .payment_failed(hash)
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
                .payment_failed(hash)
                .await
                .context("(RouteNotFound) Could not register failure")?;
            Err(anyhow!("LDK returned RouteNotFound (OIP {hash})"))
        }
    }
}

#[instrument(skip_all, name = "(send-onchain)")]
pub async fn send_onchain<CM, PS>(
    req: SendOnchainRequest,
    wallet: LexeWallet,
    esplora: Arc<LexeEsplora>,
    payments_manager: PaymentsManager<CM, PS>,
) -> anyhow::Result<LxTxid>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // Create and sign the onchain send tx.
    let onchain_send = wallet
        .create_onchain_send(req)
        .await
        .context("Error while creating outbound tx")?;
    let tx = onchain_send.tx.clone();
    let txid = onchain_send.txid;

    // Register the transaction.
    payments_manager
        .new_payment(onchain_send)
        .await
        .context("Could not register new onchain send")?;

    // Broadcast.
    esplora
        .broadcast_tx(&tx)
        .await
        .context("Failed to broadcast tx")?;

    // Register the successful broadcast.
    payments_manager
        .onchain_send_broadcasted(txid)
        .await
        .context("Could not register broadcast of tx")?;

    // NOTE: The reason why we call into the payments manager twice (and thus
    // persist the payment twice) instead of simply registering the new payment
    // after it has been successfully broadcast is so that we don't end up in a
    // situation where a payment is successfully sent but we have no record of
    // it; i.e. the broadcast succeeds but our registration doesn't. This also
    // ensures that the txid is unique before we broadcast in case there is a
    // txid collision for some reason (e.g. duplicate requests)

    Ok(txid)
}

#[instrument(skip_all, name = "(get-address)")]
pub async fn get_address(wallet: LexeWallet) -> anyhow::Result<Address> {
    wallet.get_address().await
}

/// Given a channel manager and `min_inbound_capacity`, generates a list of
/// [`RouteHint`]s which can be included in an [`Invoice`] to help the sender
/// find a path to us. If LSP information is also provided, a route hint with an
/// intercept scid will be included in the invoice in the case that there are no
/// ready channels with sufficient liquidity to service the payment.
///
/// The main logic was based on `lightning_invoice::utils::filter_channels`, but
/// is expected to diverge from LDK's implementation over time.
// NOTE: If two versions of this function are needed (e.g. one for user node and
// one for LSP), the function can be moved to the LexeChannelManager trait.
fn get_route_hints<CM, PS>(
    channel_manager: CM,
    caller: CreateInvoiceCaller,
    min_inbound_capacity: Option<Amount>,
) -> Vec<RouteHint>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let all_channels = channel_manager.list_channels();
    let num_channels = all_channels.len();
    debug!("Generating route hints, starting with {num_channels} channels");
    let min_inbound_capacity =
        min_inbound_capacity.unwrap_or(Amount::from_msat(0));

    let (lsp_info, scid) = match caller {
        CreateInvoiceCaller::Lsp => {
            // If the LSP is calling create_invoice, include no hints and let
            // the sender route to us by looking at the lightning network graph.
            debug!("create_invoice caller was LSP; returning 0 route hints");
            if !all_channels.iter().any(|channel| channel.is_public) {
                warn!("LSP requested invoice but has no public channels");
            }
            return Vec::new();
        }
        CreateInvoiceCaller::UserNode { lsp_info, scid } => (lsp_info, scid),
    };
    // From this point on, we know that the user node called create_invoice.

    // NOTE on multi-path payments: Eventually, we may want to include route
    // hints for channels that individually do not have sufficient liquidity to
    // route the entire payment but which could forward one portion of the whole
    // payment (under the condition that our total inbound liquidity available
    // across all of our channels is sufficient to service the whole payment).
    // However, the BOLT11 spec does not contain a way to notify the sender of
    // how much liquidity we have in each of our channels; plus, we should be
    // careful about how we expose this info as it could lead to the real-time
    // deanonymization of our current channel balances. Absent a protocol for
    // this, the sender has to blindly split (or not split) their payment across
    // our available channels, leading to frequent payment failures. Thus, for
    // now we elect not to generate route hints for channels that do not have
    // sufficient liquidity to service the entire payment; we can enable this
    // style of multi-path payments once BOLT12 or similar fixes this.
    // https://discord.com/channels/915026692102316113/978829624635195422/1070087544164851763

    // Generate a list of `RouteHint`s which correspond to channels which are
    // ready, sufficiently large, and which have all of the scid / forwarding
    // information required to construct the `RouteHintHop` for the sender.
    let route_hints = all_channels
        .into_iter()
        // Ready channels only. NOTE: We do NOT use `ChannelDetails::is_usable`
        // to prevent a race condition where our freshly-started node needs to
        // generate an invoice but has not yet reconnected to its peer (the LSP)
        .filter(|c| c.is_channel_ready)
        // Channels with sufficient liquidity only
        .filter(|c| c.inbound_capacity_msat >= min_inbound_capacity.msat())
        // Generate a RouteHintHop for the LSP -> us channel
        .filter_map(|c| {
            // scids and forwarding info are required to construct a hop hint
            let short_channel_id = c.get_inbound_payment_scid()?;
            let fwd_info = c.counterparty.forwarding_info?;

            let fees = RoutingFees {
                base_msat: fwd_info.fee_base_msat,
                proportional_millionths: fwd_info.fee_proportional_millionths,
            };

            Some(RouteHintHop {
                src_node_id: c.counterparty.node_id,
                short_channel_id,
                fees,
                cltv_expiry_delta: fwd_info.cltv_expiry_delta,
                htlc_minimum_msat: c.inbound_htlc_minimum_msat,
                htlc_maximum_msat: c.inbound_htlc_maximum_msat,
            })
        })
        // RouteHintHop -> RouteHint
        .map(|hop_hint| RouteHint(vec![hop_hint]))
        .collect::<Vec<RouteHint>>();

    // If we generated any hints, return them.
    let num_route_hints = route_hints.len();
    if num_route_hints > 0 {
        debug!("Included {num_route_hints} route hints in invoice");
        return route_hints;
    }

    // There were no valid routes. Generate a hint with an intercept scid
    // provided by our LSP so that our LSP can open a JIT channel to us.
    debug!("No routes found; including intercept hint in invoice");
    let hop_hint = lsp_info.route_hint_hop(scid);
    vec![RouteHint(vec![hop_hint])]
}
