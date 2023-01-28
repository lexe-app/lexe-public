use std::collections::{hash_map, HashMap};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context};
use bitcoin::bech32::ToBase32;
use bitcoin::hashes::{sha256, Hash};
use bitcoin::secp256k1::PublicKey;
use common::api::command::{GetInvoiceRequest, NodeInfo};
use common::api::NodePk;
use common::cli::Network;
use common::ln::invoice::LxInvoice;
use lightning::chain::keysinterface::{NodeSigner, Recipient};
use lightning::ln::channelmanager::{ChannelDetails, MIN_FINAL_CLTV_EXPIRY};
use lightning::ln::PaymentHash;
use lightning::routing::gossip::RoutingFees;
use lightning::routing::router::{RouteHint, RouteHintHop};
use lightning_invoice::{Currency, Invoice, InvoiceBuilder};
use tracing::info;

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
    let channels = channel_manager.list_channels();
    let route_hints = self::filter_channels(channels, req.amt_msat);
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

/// Filters the output returned by [`ChannelManager::list_channels`] to generate
/// [`RouteHint`]s. Based on `lightning_invoice::utils::filter_channels`.
///
/// [`ChannelManager::list_channels`]: lightning::ln::channelmanager::ChannelManager::list_channels
// Currently, this is just a copy of lightning_invoice::utils::filter_channels
// with lints fixed and logging ripped out. TODO(max): Adapt this to our needs
pub(super) fn filter_channels(
    channels: Vec<ChannelDetails>,
    min_inbound_capacity_msat: Option<u64>,
) -> Vec<RouteHint> {
    let mut filtered_channels = HashMap::<PublicKey, ChannelDetails>::new();
    let min_inbound_capacity = min_inbound_capacity_msat.unwrap_or(0);
    let mut min_capacity_channel_exists = false;
    let mut online_channel_exists = false;
    let mut online_min_capacity_channel_exists = false;

    for channel in channels.into_iter().filter(|chan| chan.is_channel_ready) {
        if channel.get_inbound_payment_scid().is_none()
            || channel.counterparty.forwarding_info.is_none()
        {
            continue;
        }

        if channel.is_public {
            // If any public channel exists, return no hints and let the
            // sender look at the public channels instead.
            return vec![];
        }

        if channel.inbound_capacity_msat >= min_inbound_capacity {
            if !min_capacity_channel_exists {
                min_capacity_channel_exists = true;
            }

            if channel.is_usable {
                online_min_capacity_channel_exists = true;
            }
        }

        if channel.is_usable && !online_channel_exists {
            online_channel_exists = true;
        }

        match filtered_channels.entry(channel.counterparty.node_id) {
            hash_map::Entry::Occupied(mut entry) => {
                let current_max_capacity = entry.get().inbound_capacity_msat;
                if channel.inbound_capacity_msat < current_max_capacity {
                    continue;
                }
                entry.insert(channel);
            }
            hash_map::Entry::Vacant(entry) => {
                entry.insert(channel);
            }
        }
    }

    let route_hint_from_channel = |channel: ChannelDetails| {
        let forwarding_info =
            channel.counterparty.forwarding_info.as_ref().unwrap();
        RouteHint(vec![RouteHintHop {
            src_node_id: channel.counterparty.node_id,
            short_channel_id: channel.get_inbound_payment_scid().unwrap(),
            fees: RoutingFees {
                base_msat: forwarding_info.fee_base_msat,
                proportional_millionths: forwarding_info
                    .fee_proportional_millionths,
            },
            cltv_expiry_delta: forwarding_info.cltv_expiry_delta,
            htlc_minimum_msat: channel.inbound_htlc_minimum_msat,
            htlc_maximum_msat: channel.inbound_htlc_maximum_msat,
        }])
    };
    // If all channels are private, prefer to return route hints which have
    // a higher capacity than the payment value and where we're
    // currently connected to the channel counterparty. Even if we
    // cannot satisfy both goals, always ensure we include *some* hints,
    // preferring those which meet at least one criteria.
    filtered_channels
        .into_values()
        .filter(|channel| {
            let has_enough_capacity =
                channel.inbound_capacity_msat >= min_inbound_capacity;
            #[allow(clippy::if_same_then_else)]
            let include_channel = if online_min_capacity_channel_exists {
                has_enough_capacity && channel.is_usable
            } else if min_capacity_channel_exists && online_channel_exists {
                // If there are some online channels and some min_capacity
                // channels, but no
                // online-and-min_capacity channels, just include the min
                // capacity ones and ignore online-ness.
                has_enough_capacity
            } else if min_capacity_channel_exists {
                has_enough_capacity
            } else if online_channel_exists {
                channel.is_usable
            } else {
                true
            };

            include_channel
        })
        .map(route_hint_from_channel)
        .collect::<Vec<RouteHint>>()
}
