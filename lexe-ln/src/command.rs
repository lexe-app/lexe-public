use std::time::Duration;

use anyhow::{anyhow, Context};
use bitcoin::bech32::ToBase32;
use bitcoin::hashes::{sha256, Hash};
use common::api::command::{GetInvoiceRequest, NodeInfo};
use common::api::NodePk;
use common::cli::{LspInfo, Network};
use common::ln::invoice::LxInvoice;
use common::notify;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::channelmanager::{Retry, MIN_FINAL_CLTV_EXPIRY_DELTA};
use lightning::ln::PaymentHash;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder};
use tracing::{debug, info};

use crate::alias::PaymentInfoStorageType;
use crate::invoice::{HTLCStatus, LxPaymentError, PaymentInfo};
use crate::keys_manager::LexeKeysManager;
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

/// The number of times to retry a failed payment in `send_payment`.
const PAYMENT_RETRY_ATTEMPTS: usize = 3;

/// Specifies whether it is the user node or the LSP calling the [`get_invoice`]
/// fn. There are some differences between how the user node and LSP
/// generate invoices which this tiny enum makes clearer.
#[derive(Clone)]
pub enum GetInvoiceCaller {
    /// When a user node calls [`get_invoice`], it must provide an [`LspInfo`],
    /// which is required for generating a [`RouteHintHop`] for receiving a
    /// payment over a JIT channel with the LSP.
    UserNode {
        lsp_info: LspInfo,
    },
    Lsp,
}

pub fn node_info<CM, PM, PS>(channel_manager: CM, peer_manager: PM) -> NodeInfo
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
    let num_peers = peer_manager.get_peer_node_ids().len();

    NodeInfo {
        node_pk,
        num_channels,
        num_usable_channels,
        local_balance_msat,
        num_peers,
    }
}

pub fn get_invoice<CM, PS>(
    channel_manager: CM,
    keys_manager: LexeKeysManager,
    caller: GetInvoiceCaller,
    network: Network,
    req: GetInvoiceRequest,
) -> anyhow::Result<LxInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let amt_msat = &req.amt_msat;
    let cltv_expiry = MIN_FINAL_CLTV_EXPIRY_DELTA;
    info!("Handling get_invoice command for {amt_msat:?} msats");

    // We use ChannelManager::create_inbound_payment because this method allows
    // the channel manager to store the hash and preimage for us, instead of
    // having to manage a separate inbound payments storage outside of LDK.
    // NOTE that `handle_payment_claimable` will panic if the payment preimage
    // is not known by (and therefore cannot be provided by) LDK.
    let (payment_hash, payment_secret) = channel_manager
        .create_inbound_payment(
            req.amt_msat,
            req.expiry_secs,
            Some(cltv_expiry),
        )
        .map_err(|()| {
            anyhow!("Supplied msat amount > total bitcoin supply!")
        })?;

    let currency = Currency::from(network);
    let payment_hash = sha256::Hash::from_slice(&payment_hash.0)
        .expect("Should never fail with [u8;32]");
    let expiry_time = Duration::from_secs(u64::from(req.expiry_secs));
    let our_node_pk = channel_manager.get_our_node_id();

    // Add most parts of the invoice, except for the route hints.
    // This is modeled after lightning_invoice's internal utility function
    // _create_invoice_from_channelmanager_and_duration_since_epoch_with_payment_hash
    #[rustfmt::skip] // Nicer for the generic annotations to be aligned
    let mut builder = InvoiceBuilder::new(currency)          // <D, H, T, C, S>
        .description(req.description)                        // D: False -> True
        .payment_hash(payment_hash)                          // H: False -> True
        .current_timestamp()                                 // T: False -> True
        .min_final_cltv_expiry_delta(u64::from(cltv_expiry)) // C: False -> True
        .payment_secret(payment_secret)                      // S: False -> True
        .basic_mpp()                                         // S: _ -> True
        .expiry_time(expiry_time)
        .payee_pub_key(our_node_pk);
    if let Some(amt_msat) = req.amt_msat {
        builder = builder.amount_milli_satoshis(amt_msat);
    }

    // Add the route hints.
    let route_hints = get_route_hints(channel_manager, caller, req.amt_msat);
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

    info!("Success: Generated invoice {invoice}");

    Ok(invoice)
}

pub fn send_payment<CM, PS>(
    invoice: LxInvoice,
    channel_manager: CM,
    outbound_payments: PaymentInfoStorageType,
    process_events_tx: notify::Sender,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let retry = Retry::Attempts(PAYMENT_RETRY_ATTEMPTS);
    let payment_result = lightning_invoice::payment::pay_invoice(
        &invoice.0,
        retry,
        channel_manager.deref(),
    )
    .map_err(LxPaymentError::from);

    // Store the payment in our outbound payments storage as pending or failed
    // depending on the payment result
    let payment_hash = PaymentHash(invoice.0.payment_hash().into_inner());
    let preimage = None;
    let secret = Some(*invoice.0.payment_secret());
    let amt_msat = invoice.0.amount_milli_satoshis();
    let status = if payment_result.is_ok() {
        HTLCStatus::Pending
    } else {
        HTLCStatus::Failed
    };
    outbound_payments.lock().expect("Poisoned").insert(
        payment_hash,
        PaymentInfo {
            preimage,
            secret,
            amt_msat,
            status,
        },
    );

    let _payment_id = payment_result.context("Couldn't initiate payment")?;
    let payee_pk = invoice.0.recover_payee_pub_key();
    info!("Success: Initiated payment of {amt_msat:?} msats to {payee_pk}");
    process_events_tx.send();

    Ok(())
}

/// Given a channel manager and `min_inbound_capacity_msat`, generates a list of
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
    _caller: GetInvoiceCaller,
    min_inbound_capacity_msat: Option<u64>,
) -> Vec<RouteHint>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let all_channels = channel_manager.list_channels();
    let num_channels = all_channels.len();
    debug!("Generating route hints, starting with {num_channels} channels");
    let min_inbound_capacity_msat = min_inbound_capacity_msat.unwrap_or(0);

    // If *any* channel is public, include no hints and let the sender route to
    // us by looking at the lightning network graph. This helps prevent a public
    // node from accidentally exposing its private relationships.
    if all_channels.iter().any(|channel| channel.is_public) {
        debug!("Found public channel; returning 0 route hints");
        return Vec::new();
    }
    // At this point, we know that all channels are private.

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
    all_channels
        .into_iter()
        // Ready channels only. NOTE: We do NOT use `ChannelDetails::is_usable`
        // to prevent a race condition where our freshly-started node needs to
        // generate an invoice but has not yet reconnected to its peer (the LSP)
        .filter(|c| c.is_channel_ready)
        // Channels with sufficient liquidity only
        .filter(|c| c.inbound_capacity_msat >= min_inbound_capacity_msat)
        // Generate a RouteHintHop for the counterparty -> us channel
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
        .collect::<Vec<RouteHint>>()

    // TODO(max): The following implementation is wrong; the scid needs to be
    // generated by the *LSP*'s channel manager, not the user node's, which
    // requires a ton of coordination that we don't support yet.

    /*
    // If we generated any hints, return them.
    let num_route_hints = route_hints.len();
    if num_route_hints > 0 {
        debug!("Included {num_route_hints} route hints in invoice");
        return route_hints;
    }

    // There were no valid routes. If we have our LSP's NodePk, generate a hint
    // with an intercept scid so that our LSP can open a JIT channel to us.
    match caller {
        GetInvoiceCaller::UserNode { lsp_info } => {
            debug!("Included intercept hint in invoice");
            let short_channel_id = channel_manager.get_intercept_scid();
            let hop_hint = lsp_info.route_hint_hop(short_channel_id);
            vec![RouteHint(vec![hop_hint])]
        }
        Lsp => {
            warn!(
                "LSP did not generate any route hints: payment amt msat was \
                {min_inbound_capacity_msat:?}"
            );
            Vec::new()
        }
    }
    */
}
