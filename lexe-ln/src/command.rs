use anyhow::{anyhow, Context};
use bitcoin::hashes::Hash;
use common::api::command::{GetInvoiceRequest, NodeInfo};
use common::cli::Network;
use common::ln::invoice::LxInvoice;
use lightning::ln::PaymentHash;
use lightning_invoice::Currency;

use crate::alias::PaymentInfoStorageType;
use crate::invoice::{HTLCStatus, MillisatAmount, PaymentInfo};
use crate::keys_manager::LexeKeysManager;
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

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
    let node_pk = channel_manager.get_our_node_id();

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
    inbound_payments: PaymentInfoStorageType,
    network: Network,
    req: GetInvoiceRequest,
) -> anyhow::Result<LxInvoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let currency = Currency::from(network);

    // Generate the invoice
    let invoice = lightning_invoice::utils::create_invoice_from_channelmanager(
        &channel_manager,
        keys_manager,
        currency,
        req.amt_msat,
        "lexe-node".to_string(),
        req.expiry_secs,
    )
    .map(LxInvoice)
    .map_err(|e| anyhow!("{e}"))
    .context("Failed to create invoice")?;

    // Save the invoice in our inbound payment storage
    // TODO(max): Is this really needed? `create_invoice_from_channelmanager`
    // docs notes that we don't have to store the payment preimage / secret
    // information
    let payment_hash = PaymentHash(invoice.0.payment_hash().into_inner());
    inbound_payments.lock().expect("Poisoned").insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: Some(*invoice.0.payment_secret()),
            status: HTLCStatus::Pending,
            amt_msat: MillisatAmount(req.amt_msat),
        },
    );

    Ok(invoice)
}
