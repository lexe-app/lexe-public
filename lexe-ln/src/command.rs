use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use bitcoin::bech32::ToBase32;
use bitcoin::hashes::{sha256, Hash};
use common::api::command::{GetInvoiceRequest, NodeInfo};
use common::api::NodePk;
use common::cli::Network;
use common::ln::invoice::LxInvoice;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::channelmanager::MIN_FINAL_CLTV_EXPIRY;
use lightning::ln::PaymentHash;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder};
use tracing::{info, warn};

use crate::alias::{LexeInvoicePayerType, PaymentInfoStorageType};
use crate::invoice::{HTLCStatus, LxPaymentError, PaymentInfo};
use crate::keys_manager::LexeKeysManager;
use crate::traits::{
    LexeChannelManager, LexeEventHandler, LexePeerManager, LexePersister,
};

// TODO(max): Should these fns take e.g. &CM i.e. &Arc<impl LexeChannelManager>
// when possible? It can avoid the atomic operation in some cases, but in
// addition to requiring more indirection from node::command::server, it's a
// weird way to use Arc<T>s. Taking &T doesn't seem possible though without an
// invasive (translated: painful) overhaul of the Lexe trait aliases.

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
    maybe_lsp_node_pk: Option<NodePk>,
    network: Network,
    req: GetInvoiceRequest,
) -> anyhow::Result<LxInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    // We use ChannelManager::create_inbound_payment because this method allows
    // the channel manager to store the hash and preimage for us, instead of
    // having to manage a separate inbound payments storage outside of LDK.
    // NOTE that `handle_payment_claimable` will panic if the payment preimage
    // is not known by (and therefore cannot be provided by) LDK.
    let (payment_hash, payment_secret) = channel_manager
        .create_inbound_payment(req.amt_msat, req.expiry_secs)
        .map_err(|()| {
            anyhow!("Supplied msat amount > total bitcoin supply!")
        })?;

    let currency = Currency::from(network);
    let payment_hash = sha256::Hash::from_slice(&payment_hash.0)
        .expect("Should never fail with [u8;32]");
    let cltv_expiry = u64::from(MIN_FINAL_CLTV_EXPIRY);
    let expiry_time = Duration::from_secs(u64::from(req.expiry_secs));
    let our_node_pk = channel_manager.get_our_node_id();

    // Add most parts of the invoice, except for the route hints.
    // This is modeled after lightning_invoice's internal utility function
    // _create_invoice_from_channelmanager_and_duration_since_epoch_with_payment_hash
    #[rustfmt::skip] // Nicer for the generic annotations to be aligned
    let mut builder = InvoiceBuilder::new(currency) // <D, H, T, C, S>
        .description(req.description)               // D: False -> True
        .payment_hash(payment_hash)                 // H: False -> True
        .current_timestamp()                        // T: False -> True
        .min_final_cltv_expiry(cltv_expiry)         // C: False -> True
        .payment_secret(payment_secret)             // S: False -> True
        .basic_mpp()                                // S: _ -> True
        .expiry_time(expiry_time)
        .payee_pub_key(our_node_pk);
    if let Some(amt_msat) = req.amt_msat {
        builder = builder.amount_milli_satoshis(amt_msat);
    }

    // Add the route hints.
    let route_hints =
        get_route_hints(channel_manager, maybe_lsp_node_pk, req.amt_msat);
    for hint in route_hints {
        builder = builder.private_route(hint);
    }

    // TODO(max): Generate route hint with a fake scid for JIT channels

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

pub fn send_payment<CM, PS, EH>(
    invoice_payer: Arc<LexeInvoicePayerType<CM, EH>>,
    outbound_payments: PaymentInfoStorageType,
    invoice: LxInvoice,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
    EH: LexeEventHandler,
{
    let payment_result = invoice_payer
        .pay_invoice(&invoice.0)
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

    Ok(())
}

/// Given a channel manager and `min_inbound_capacity_msat`, generates a list of
/// [`RouteHint`]s which can be included in an [`Invoice`] to help the sender
/// find a path to us.
///
/// The main logic is based on `lightning_invoice::utils::filter_channels`, but
/// is expected to diverge from LDK's implementation over time.
// NOTE: If two versions of this function are needed (e.g. one for user node and
// one for LSP), the function can be moved to the LexeChannelManager trait.
fn get_route_hints<CM, PS>(
    channel_manager: CM,
    maybe_lsp_node_pk: Option<NodePk>,
    min_inbound_capacity_msat: Option<u64>,
) -> Vec<RouteHint>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let all_channels = channel_manager.list_channels();
    let min_inbound_capacity_msat = min_inbound_capacity_msat.unwrap_or(0);

    // If *any* channel is public, include no hints and let the sender route to
    // us by looking at the lightning network graph. This helps prevent a public
    // node from accidentally exposing its private relationships.
    if all_channels.iter().any(|channel| channel.is_public) {
        return Vec::new();
    }
    // At this point, we know that all channels are private.

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
        .collect::<Vec<RouteHint>>();

    // If we generated any hints, return them.
    if !route_hints.is_empty() {
        return route_hints;
    }

    // There were no valid routes. If we have our LSP's NodePk, generate a hint
    // with an intercept scid so that our LSP can open a JIT channel to us.
    match maybe_lsp_node_pk {
        Some(lsp_node_pk) => {
            let short_channel_id = channel_manager.get_intercept_scid();
            let hop_hint = RouteHintHop {
                src_node_id: lsp_node_pk.0,
                short_channel_id,

                // NOTE: Hack; these values are copied from the LSP's
                // UserConfig. These should be passed in via CLI args, but this
                // requires a chain of changes going all the way up to the
                // runner's CLI args, which need to be cleaned up first.
                // TODO(max): Populate these values via CLI arg
                fees: RoutingFees {
                    base_msat: 0,
                    proportional_millionths: 3000,
                },
                cltv_expiry_delta: 72,
                htlc_minimum_msat: Some(1),
                htlc_maximum_msat: Some(u64::MAX),
            };

            vec![RouteHint(vec![hop_hint])]
        }
        None => {
            warn!(
                "Did not generate any route hints: `maybe_lsp_node_pk`  was \
                None and payment amt msat was {min_inbound_capacity_msat:?}"
            );
            Vec::new()
        }
    }
}
