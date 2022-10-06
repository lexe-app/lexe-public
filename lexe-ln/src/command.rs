use anyhow::{anyhow, Context};
use bitcoin::hashes::Hash;
use common::api::node::NodeInfo;
use common::cli::Network;
use lightning::ln::PaymentHash;
use lightning_invoice::{Currency, Invoice};

use crate::alias::PaymentInfoStorageType;
use crate::invoice::{HTLCStatus, MillisatAmount, PaymentInfo};
use crate::keys_manager::LexeKeysManager;
use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

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
    channel_manager: &CM,
    keys_manager: LexeKeysManager,
    inbound_payments: PaymentInfoStorageType,
    network: Network,
    amt_msat: Option<u64>,
    expiry_secs: u32,
) -> anyhow::Result<Invoice>
where
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
{
    let currency = Currency::from(network);

    // Generate the invoice
    let invoice = lightning_invoice::utils::create_invoice_from_channelmanager(
        channel_manager,
        keys_manager,
        currency,
        amt_msat,
        "lexe-node".to_string(),
        expiry_secs,
    )
    .map_err(|e| anyhow!("{e}"))
    .context("Failed to create invoice")?;

    // Save the invoice in our inbound payment storage
    // TODO: Is this really needed? `create_invoice_from_channelmanager` docs
    // notes that we don't have to store the payment preimage / secret
    // information
    let payment_hash = PaymentHash(invoice.payment_hash().into_inner());
    inbound_payments.lock().expect("Poisoned").insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: Some(*invoice.payment_secret()),
            status: HTLCStatus::Pending,
            amt_msat: MillisatAmount(amt_msat),
        },
    );

    Ok(invoice)
}
