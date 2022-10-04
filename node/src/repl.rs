#![cfg(not(target_env = "sgx"))]

use std::io;
use std::io::{BufRead, Write};
use std::ops::Deref;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use bitcoin::hashes::sha256::Hash as Sha256;
use bitcoin::hashes::Hash;
use bitcoin::secp256k1::PublicKey;
use common::cli::Network;
use common::hex;
use common::ln::peer::ChannelPeer;
use lexe_ln::alias::{NetworkGraphType, PaymentInfoStorageType};
use lexe_ln::keys_manager::LexeKeysManager;
use lexe_ln::p2p;
use lexe_ln::types::{HTLCStatus, MillisatAmount, PaymentInfo};
use lightning::chain::keysinterface::{KeysInterface, Recipient};
use lightning::ln::{PaymentHash, PaymentPreimage};
use lightning::routing::gossip::NodeId;
use lightning_invoice::payment::PaymentError;
use lightning_invoice::{utils, Currency, Invoice};
use tracing::{error, info};

use crate::alias::InvoicePayerType;
use crate::channel_manager::NodeChannelManager;
use crate::peer_manager::NodePeerManager;
use crate::persister::NodePersister;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn poll_for_user_input(
    invoice_payer: Arc<InvoicePayerType>,
    peer_manager: NodePeerManager,
    channel_manager: NodeChannelManager,
    keys_manager: LexeKeysManager,
    network_graph: Arc<NetworkGraphType>,
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
    persister: NodePersister,
    network: Network,
) {
    info!("LDK startup successful. To view available commands: \"help\".");
    info!(
        "LDK logs are available at <your-supplied-ldk-data-dir-path>/.ldk/logs"
    );
    info!("Local Node ID is {}.", channel_manager.get_our_node_id());
    let stdin = io::stdin();
    let mut line_reader = stdin.lock().lines();
    loop {
        print!("> ");
        io::stdout().flush().unwrap(); // Without flushing, the `>` doesn't print
        let maybe_line = line_reader.next();
        let line = match maybe_line {
            Some(l) => l.unwrap(),
            None => break,
        };
        let mut words = line.split_whitespace();
        if let Some(word) = words.next() {
            match word {
                "help" => help(),
                "openchannel" => {
                    // TODO eventually do this once for all commands
                    let res = open_channel(
                        words,
                        &channel_manager,
                        &peer_manager,
                        &persister,
                    )
                    .await;
                    if let Err(e) = res {
                        // Print the entire error chain on one line
                        info!("{:#}", e);
                    }
                }
                "sendpayment" => {
                    let invoice_str = words.next();
                    if invoice_str.is_none() {
                        info!("ERROR: sendpayment requires an invoice: `sendpayment <invoice>`");
                        continue;
                    }

                    let invoice = match Invoice::from_str(invoice_str.unwrap())
                    {
                        Ok(inv) => inv,
                        Err(e) => {
                            info!("ERROR: invalid invoice: {:?}", e);
                            continue;
                        }
                    };

                    send_payment(
                        &invoice_payer,
                        &invoice,
                        outbound_payments.clone(),
                    );
                }
                "keysend" => {
                    let dest_pk = match words.next() {
                        Some(dest) => match hex_to_compressed_pk(dest) {
                            Some(pk) => pk,
                            None => {
                                info!("ERROR: couldn't parse destination pk");
                                continue;
                            }
                        },
                        None => {
                            info!("ERROR: keysend requires a destination pk: `keysend <dest_pk> <amt_msat>`");
                            continue;
                        }
                    };
                    let amt_msat_str = match words.next() {
                        Some(amt) => amt,
                        None => {
                            info!("ERROR: keysend requires an amount in millisatoshis: `keysend <dest_pk> <amt_msat>`");
                            continue;
                        }
                    };
                    let amt_msat: u64 = match amt_msat_str.parse() {
                        Ok(amt) => amt,
                        Err(e) => {
                            info!("ERROR: couldn't parse amount_msat: {}", e);
                            continue;
                        }
                    };
                    keysend(
                        &invoice_payer,
                        dest_pk,
                        amt_msat,
                        &*keys_manager,
                        outbound_payments.clone(),
                    );
                }
                "getinvoice" => {
                    let amt_str = words.next();
                    if amt_str.is_none() {
                        info!("ERROR: getinvoice requires an amount in millisatoshis");
                        continue;
                    }

                    let amt_msat: Result<u64, _> = amt_str.unwrap().parse();
                    if amt_msat.is_err() {
                        info!("ERROR: getinvoice provided payment amount was not a number");
                        continue;
                    }
                    let expiry_secs_str = words.next();
                    if expiry_secs_str.is_none() {
                        info!(
                            "ERROR: getinvoice requires an expiry in seconds"
                        );
                        continue;
                    }

                    let expiry_secs: Result<u32, _> =
                        expiry_secs_str.unwrap().parse();
                    if expiry_secs.is_err() {
                        info!("ERROR: getinvoice provided expiry was not a number");
                        continue;
                    }

                    get_invoice(
                        amt_msat.unwrap(),
                        inbound_payments.clone(),
                        channel_manager.clone(),
                        keys_manager.clone(),
                        network,
                        expiry_secs.unwrap(),
                    );
                }
                "connectpeer" => {
                    if let Err(e) = connect_peer(words, &peer_manager).await {
                        error!("{e:#}")
                    }
                }
                "listchannels" => {
                    list_channels(&channel_manager, &network_graph)
                }
                "listpayments" => list_payments(
                    inbound_payments.clone(),
                    outbound_payments.clone(),
                ),
                "closechannel" => {
                    let channel_id_str = words.next();
                    if channel_id_str.is_none() {
                        info!("ERROR: closechannel requires a channel ID: `closechannel <channel_id> <peer_pk>`");
                        continue;
                    }
                    let channel_id_vec = hex::decode(channel_id_str.unwrap());
                    if channel_id_vec.is_err()
                        || channel_id_vec.as_ref().unwrap().len() != 32
                    {
                        info!("ERROR: couldn't parse channel_id");
                        continue;
                    }
                    let mut channel_id = [0; 32];
                    channel_id.copy_from_slice(&channel_id_vec.unwrap());

                    let peer_pk_str = words.next();
                    if peer_pk_str.is_none() {
                        info!("ERROR: closechannel requires a peer pk: `closechannel <channel_id> <peer_pk>`");
                        continue;
                    }
                    let peer_pk_vec = match hex::decode(peer_pk_str.unwrap()) {
                        Ok(peer_pk_vec) => peer_pk_vec,
                        Err(err) => {
                            info!("ERROR: couldn't parse peer_pk: {err}");
                            continue;
                        }
                    };
                    let peer_pk = match PublicKey::from_slice(&peer_pk_vec) {
                        Ok(peer_pk) => peer_pk,
                        Err(_) => {
                            info!("ERROR: couldn't parse peer_pk");
                            continue;
                        }
                    };

                    close_channel(channel_id, peer_pk, channel_manager.clone());
                }
                "forceclosechannel" => {
                    let channel_id_str = words.next();
                    if channel_id_str.is_none() {
                        info!("ERROR: forceclosechannel requires a channel ID: `forceclosechannel <channel_id> <peer_pk>`");
                        continue;
                    }
                    let channel_id_vec = hex::decode(channel_id_str.unwrap());
                    if channel_id_vec.is_err()
                        || channel_id_vec.as_ref().unwrap().len() != 32
                    {
                        info!("ERROR: couldn't parse channel_id");
                        continue;
                    }
                    let mut channel_id = [0; 32];
                    channel_id.copy_from_slice(&channel_id_vec.unwrap());

                    let peer_pk_str = words.next();
                    if peer_pk_str.is_none() {
                        info!("ERROR: forceclosechannel requires a peer pk: `forceclosechannel <channel_id> <peer_pk>`");
                        continue;
                    }
                    let peer_pk_vec = match hex::decode(peer_pk_str.unwrap()) {
                        Ok(peer_pk_vec) => peer_pk_vec,
                        Err(err) => {
                            info!("ERROR: couldn't parse peer_pk: {err}");
                            continue;
                        }
                    };
                    let peer_pk = match PublicKey::from_slice(&peer_pk_vec) {
                        Ok(peer_pk) => peer_pk,
                        Err(err) => {
                            info!("ERROR: couldn't parse peer_pk: {err}");
                            continue;
                        }
                    };

                    force_close_channel(
                        channel_id,
                        peer_pk,
                        channel_manager.clone(),
                    );
                }
                "nodeinfo" => node_info(&channel_manager, &peer_manager),
                "listpeers" => list_peers(peer_manager.clone()),
                "signmessage" => {
                    const MSG_STARTPOS: usize = "signmessage".len() + 1;
                    if line.as_bytes().len() <= MSG_STARTPOS {
                        info!("ERROR: signmsg requires a message");
                        continue;
                    }
                    info!(
                        "{:?}",
                        lightning::util::message_signing::sign(
                            &line.as_bytes()[MSG_STARTPOS..],
                            &keys_manager
                                .get_node_secret(Recipient::Node)
                                .unwrap()
                        )
                    );
                }
                _ => info!(
                    "Unknown command. See `\"help\" for available commands."
                ),
            }
        }
    }
}

fn help() {
    info!("openchannel pk@host:port <amt_satoshis>");
    info!("sendpayment <invoice>");
    info!("keysend <dest_pk> <amt_msats>");
    info!("getinvoice <amt_msats> <expiry_secs>");
    info!("connectpeer pk@host:port");
    info!("listchannels");
    info!("listpayments");
    info!("closechannel <channel_id> <peer_pk>");
    info!("forceclosechannel <channel_id> <peer_pk>");
    info!("nodeinfo");
    info!("listpeers");
    info!("signmessage <message>");
}

fn node_info(
    channel_manager: &NodeChannelManager,
    peer_manager: &NodePeerManager,
) {
    info!("\t{{");
    info!("\t\t node_pk: {}", channel_manager.get_our_node_id());
    let chans = channel_manager.list_channels();
    info!("\t\t num_channels: {}", chans.len());
    info!(
        "\t\t num_usable_channels: {}",
        chans.iter().filter(|c| c.is_usable).count()
    );
    let local_balance_msat = chans.iter().map(|c| c.balance_msat).sum::<u64>();
    info!("\t\t local_balance_msat: {}", local_balance_msat);
    info!("\t\t num_peers: {}", peer_manager.get_peer_node_ids().len());
    info!("\t}},");
}

fn list_peers(peer_manager: NodePeerManager) {
    info!("\t{{");
    for pk in peer_manager.get_peer_node_ids() {
        info!("\t\t pk: {}", pk);
    }
    info!("\t}},");
}

fn list_channels(
    channel_manager: &NodeChannelManager,
    network_graph: &Arc<NetworkGraphType>,
) {
    print!("[");
    for chan_info in channel_manager.list_channels() {
        info!("\t{{");
        info!(
            "\t\tchannel_id: {},",
            hex::encode(&chan_info.channel_id[..])
        );
        if let Some(funding_txo) = chan_info.funding_txo {
            info!("\t\tfunding_txid: {},", funding_txo.txid);
        }

        info!(
            "\t\tpeer_pk: {},",
            hex::encode(&chan_info.counterparty.node_id.serialize())
        );
        if let Some(node_info) = network_graph
            .read_only()
            .nodes()
            .get(&NodeId::from_pubkey(&chan_info.counterparty.node_id))
        {
            if let Some(announcement) = &node_info.announcement_info {
                info!("\t\tpeer_alias: {}", announcement.alias);
            }
        }

        if let Some(id) = chan_info.short_channel_id {
            info!("\t\tshort_channel_id: {},", id);
        }
        info!("\t\tis_channel_ready: {},", chan_info.is_channel_ready);
        info!(
            "\t\tchannel_value_satoshis: {},",
            chan_info.channel_value_satoshis
        );
        info!("\t\tlocal_balance_msat: {},", chan_info.balance_msat);
        if chan_info.is_usable {
            info!(
                "\t\tavailable_balance_for_send_msat: {},",
                chan_info.outbound_capacity_msat
            );
            info!(
                "\t\tavailable_balance_for_recv_msat: {},",
                chan_info.inbound_capacity_msat
            );
        }
        info!("\t\tchannel_can_send_payments: {},", chan_info.is_usable);
        info!("\t\tpublic: {},", chan_info.is_public);
        info!("\t}},");
    }
    info!("]");
}

fn list_payments(
    inbound_payments: PaymentInfoStorageType,
    outbound_payments: PaymentInfoStorageType,
) {
    let inbound = inbound_payments.lock().unwrap();
    let inbound = inbound.deref();
    print!("[");
    for (payment_hash, payment_info) in inbound {
        info!("\t{{");
        info!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
        info!("\t\tpayment_hash: {},", hex::encode(&payment_hash.0));
        info!("\t\thtlc_direction: inbound,");
        info!(
            "\t\thtlc_status: {},",
            match payment_info.status {
                HTLCStatus::Pending => "pending",
                HTLCStatus::Succeeded => "succeeded",
                HTLCStatus::Failed => "failed",
            }
        );

        info!("\t}},");
    }

    let outbound = outbound_payments.lock().unwrap();
    let outbound = outbound.deref();
    for (payment_hash, payment_info) in outbound {
        info!("\t{{");
        info!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
        info!("\t\tpayment_hash: {},", hex::encode(&payment_hash.0));
        info!("\t\thtlc_direction: outbound,");
        info!(
            "\t\thtlc_status: {},",
            match payment_info.status {
                HTLCStatus::Pending => "pending",
                HTLCStatus::Succeeded => "succeeded",
                HTLCStatus::Failed => "failed",
            }
        );

        info!("\t}},");
    }
    info!("]");
}

async fn connect_peer<'a, I: Iterator<Item = &'a str>>(
    mut words: I,
    peer_manager: &NodePeerManager,
) -> anyhow::Result<()> {
    let peer_pk_and_ip_addr = words
        .next()
        .context("connectpeer format: `connectpeer <node_pk>@<host>:<port>`")?;
    let channel_peer = ChannelPeer::from_str(peer_pk_and_ip_addr)
        .context("Could not parse ChannelPeer")?;

    p2p::connect_channel_peer_if_necessary(
        peer_manager.arc_inner(),
        channel_peer.clone(),
    )
    .await
    .context("Could not connect to peer")?;

    info!("Success: connected to peer {}", channel_peer.node_pk);

    Ok(())
}

fn send_payment(
    invoice_payer: &InvoicePayerType,
    invoice: &Invoice,
    payment_storage: PaymentInfoStorageType,
) {
    let status = match invoice_payer.pay_invoice(invoice) {
        Ok(_payment_id) => {
            let payee_pk = invoice.recover_payee_pub_key();
            let amt_msat = invoice.amount_milli_satoshis().unwrap();
            info!(
                "EVENT: initiated sending {} msats to {}",
                amt_msat, payee_pk
            );
            print!("> ");
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            info!("ERROR: invalid invoice: {}", e);
            print!("> ");
            return;
        }
        Err(PaymentError::Routing(e)) => {
            info!("ERROR: failed to find route: {}", e.err);
            print!("> ");
            return;
        }
        Err(PaymentError::Sending(e)) => {
            info!("ERROR: failed to send payment: {:?}", e);
            print!("> ");
            HTLCStatus::Failed
        }
    };
    let payment_hash = PaymentHash(invoice.payment_hash().into_inner());
    let payment_secret = Some(*invoice.payment_secret());

    let mut payments = payment_storage.lock().unwrap();
    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: payment_secret,
            status,
            amt_msat: MillisatAmount(invoice.amount_milli_satoshis()),
        },
    );
}

fn keysend<K: KeysInterface>(
    invoice_payer: &InvoicePayerType,
    payee_pk: PublicKey,
    amt_msat: u64,
    keys: &K,
    payment_storage: PaymentInfoStorageType,
) {
    let payment_preimage = keys.get_secure_random_bytes();

    let status = match invoice_payer.pay_pubkey(
        payee_pk,
        PaymentPreimage(payment_preimage),
        amt_msat,
        40,
    ) {
        Ok(_payment_id) => {
            info!(
                "EVENT: initiated sending {} msats to {}",
                amt_msat, payee_pk
            );
            print!("> ");
            HTLCStatus::Pending
        }
        Err(PaymentError::Invoice(e)) => {
            info!("ERROR: invalid payee: {}", e);
            print!("> ");
            return;
        }
        Err(PaymentError::Routing(e)) => {
            info!("ERROR: failed to find route: {}", e.err);
            print!("> ");
            return;
        }
        Err(PaymentError::Sending(e)) => {
            info!("ERROR: failed to send payment: {:?}", e);
            print!("> ");
            HTLCStatus::Failed
        }
    };

    let mut payments = payment_storage.lock().unwrap();
    payments.insert(
        PaymentHash(Sha256::hash(&payment_preimage).into_inner()),
        PaymentInfo {
            preimage: None,
            secret: None,
            status,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
}

fn get_invoice(
    amt_msat: u64,
    payment_storage: PaymentInfoStorageType,
    channel_manager: NodeChannelManager,
    keys_manager: LexeKeysManager,
    network: Network,
    expiry_secs: u32,
) {
    let mut payments = payment_storage.lock().unwrap();
    let currency = Currency::from(network);
    let invoice = match utils::create_invoice_from_channelmanager(
        &channel_manager,
        keys_manager,
        currency,
        Some(amt_msat),
        "lexe-node".to_string(),
        expiry_secs,
    ) {
        Ok(inv) => {
            info!("SUCCESS: generated invoice: {}", inv);
            inv
        }
        Err(e) => {
            info!("ERROR: failed to create invoice: {:?}", e);
            return;
        }
    };

    let payment_hash = PaymentHash(invoice.payment_hash().into_inner());
    payments.insert(
        payment_hash,
        PaymentInfo {
            preimage: None,
            secret: Some(*invoice.payment_secret()),
            status: HTLCStatus::Pending,
            amt_msat: MillisatAmount(Some(amt_msat)),
        },
    );
}

/// Parses the channel peer and channel value and opens a channel.
async fn open_channel<'a, I: Iterator<Item = &'a str>>(
    mut words: I,
    channel_manager: &NodeChannelManager,
    peer_manager: &NodePeerManager,
    persister: &NodePersister,
) -> anyhow::Result<()> {
    let peer_pk_at_addr = words
        .next()
        .context("Missing first argument: pk@host:port")?;
    let channel_value_sat = words
        .next()
        .context("Missing second argument: channel_value_sat")?;

    let channel_peer = ChannelPeer::from_str(peer_pk_at_addr)
        .context("Failed to parse channel peer: pk@host:port")?;
    let channel_value_sat = u64::from_str(channel_value_sat)
        .context("channel_value_sat must be a number")?;

    channel_manager
        .open_channel(peer_manager, persister, channel_peer, channel_value_sat)
        .await
        .context("Could not open channel")
}

fn close_channel(
    channel_id: [u8; 32],
    counterparty_node_id: PublicKey,
    channel_manager: NodeChannelManager,
) {
    match channel_manager.close_channel(&channel_id, &counterparty_node_id) {
        Ok(()) => info!("EVENT: initiating channel close"),
        Err(e) => info!("ERROR: failed to close channel: {:?}", e),
    }
}

fn force_close_channel(
    channel_id: [u8; 32],
    counterparty_node_id: PublicKey,
    channel_manager: NodeChannelManager,
) {
    match channel_manager
        .force_close_broadcasting_latest_txn(&channel_id, &counterparty_node_id)
    {
        Ok(()) => info!("EVENT: initiating channel force-close"),
        Err(e) => info!("ERROR: failed to force-close channel: {:?}", e),
    }
}

fn hex_to_compressed_pk(hex: &str) -> Option<PublicKey> {
    if hex.len() != 33 * 2 {
        return None;
    }
    let data = match hex::decode(&hex[0..33 * 2]) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    match PublicKey::from_slice(&data) {
        Ok(pk) => Some(pk),
        Err(_) => None,
    }
}
