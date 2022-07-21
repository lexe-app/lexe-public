#[cfg(not(target_env = "sgx"))]
pub use not_sgx::*;
#[cfg(target_env = "sgx")]
pub use sgx::*;

#[cfg(target_env = "sgx")]
mod sgx {
    use std::sync::Arc;

    use crate::cli::Network;
    use crate::lexe::channel_manager::LexeChannelManager;
    use crate::lexe::keys_manager::LexeKeysManager;
    use crate::lexe::peer_manager::LexePeerManager;
    use crate::lexe::persister::LexePersister;
    use crate::types::{
        InvoicePayerType, NetworkGraphType, PaymentInfoStorageType,
    };

    #[allow(clippy::too_many_arguments)]
    pub async fn poll_for_user_input(
        _invoice_payer: Arc<InvoicePayerType>,
        _peer_manager: LexePeerManager,
        _channel_manager: LexeChannelManager,
        _keys_manager: LexeKeysManager,
        _network_graph: Arc<NetworkGraphType>,
        _inbound_payments: PaymentInfoStorageType,
        _outbound_payments: PaymentInfoStorageType,
        _persister: LexePersister,
        _network: Network,
    ) {
    }
}

#[cfg(not(target_env = "sgx"))]
mod not_sgx {
    use std::io;
    use std::io::{BufRead, Write};
    use std::net::{SocketAddr, ToSocketAddrs};
    use std::ops::Deref;
    use std::str::FromStr;
    use std::sync::Arc;

    use bitcoin::hashes::sha256::Hash as Sha256;
    use bitcoin::hashes::Hash;
    use bitcoin::secp256k1::PublicKey;
    use common::hex;
    use lightning::chain::keysinterface::{KeysInterface, Recipient};
    use lightning::ln::{PaymentHash, PaymentPreimage};
    use lightning::routing::gossip::NodeId;
    use lightning::util::config::{
        ChannelConfig, ChannelHandshakeLimits, UserConfig,
    };
    use lightning_invoice::payment::PaymentError;
    use lightning_invoice::{utils, Currency, Invoice};

    use crate::cli::{Network, NodeAlias};
    use crate::lexe::channel_manager::LexeChannelManager;
    use crate::lexe::keys_manager::LexeKeysManager;
    use crate::lexe::peer_manager::{self, LexePeerManager};
    use crate::lexe::persister::LexePersister;
    use crate::types::{
        HTLCStatus, InvoicePayerType, MillisatAmount, NetworkGraphType,
        PaymentInfo, PaymentInfoStorageType,
    };

    #[allow(clippy::too_many_arguments)]
    #[cfg(not(target_env = "sgx"))]
    pub async fn poll_for_user_input(
        invoice_payer: Arc<InvoicePayerType>,
        peer_manager: LexePeerManager,
        channel_manager: LexeChannelManager,
        keys_manager: LexeKeysManager,
        network_graph: Arc<NetworkGraphType>,
        inbound_payments: PaymentInfoStorageType,
        outbound_payments: PaymentInfoStorageType,
        persister: LexePersister,
        network: Network,
    ) {
        println!(
            "LDK startup successful. To view available commands: \"help\"."
        );
        println!(
            "LDK logs are available at <your-supplied-ldk-data-dir-path>/.ldk/logs"
        );
        println!("Local Node ID is {}.", channel_manager.get_our_node_id());
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
                        let peer_pubkey_and_ip_addr = words.next();
                        let channel_value_sat = words.next();
                        if peer_pubkey_and_ip_addr.is_none()
                            || channel_value_sat.is_none()
                        {
                            println!("ERROR: openchannel has 2 required arguments: `openchannel pubkey@host:port channel_amt_satoshis` [--public]");
                            continue;
                        }
                        let peer_pubkey_and_ip_addr =
                            peer_pubkey_and_ip_addr.unwrap();
                        let (pubkey, peer_addr) = match parse_peer_info(
                            peer_pubkey_and_ip_addr.to_string(),
                        ) {
                            Ok(info) => info,
                            Err(e) => {
                                println!("{:?}", e.into_inner().unwrap());
                                continue;
                            }
                        };

                        let chan_amt_sat: Result<u64, _> =
                            channel_value_sat.unwrap().parse();
                        if chan_amt_sat.is_err() {
                            println!("ERROR: channel amount must be a number");
                            continue;
                        }

                        if peer_manager::connect_peer_if_necessary(
                            pubkey,
                            peer_addr,
                            peer_manager.clone(),
                        )
                        .await
                        .is_err()
                        {
                            continue;
                        };

                        let announce_channel = match words.next() {
                            Some("--public") | Some("--public=true") => true,
                            Some("--public=false") => false,
                            Some(_) => {
                                println!("ERROR: invalid `--public` command format. Valid formats: `--public`, `--public=true` `--public=false`");
                                continue;
                            }
                            None => false,
                        };

                        if open_channel(
                            pubkey,
                            chan_amt_sat.unwrap(),
                            announce_channel,
                            channel_manager.clone(),
                        )
                        .is_ok()
                        {
                            if let Err(e) = persister
                                .persist_channel_peer(pubkey, peer_addr)
                                .await
                            {
                                println!(
                                    "ERROR Could not persist channel peer: {}",
                                    e
                                );
                            }
                        }
                    }
                    "sendpayment" => {
                        let invoice_str = words.next();
                        if invoice_str.is_none() {
                            println!("ERROR: sendpayment requires an invoice: `sendpayment <invoice>`");
                            continue;
                        }

                        let invoice =
                            match Invoice::from_str(invoice_str.unwrap()) {
                                Ok(inv) => inv,
                                Err(e) => {
                                    println!("ERROR: invalid invoice: {:?}", e);
                                    continue;
                                }
                            };

                        send_payment(
                            &*invoice_payer,
                            &invoice,
                            outbound_payments.clone(),
                        );
                    }
                    "keysend" => {
                        let dest_pubkey = match words.next() {
                            Some(dest) => {
                                match hex_to_compressed_pubkey(dest) {
                                    Some(pk) => pk,
                                    None => {
                                        println!("ERROR: couldn't parse destination pubkey");
                                        continue;
                                    }
                                }
                            }
                            None => {
                                println!("ERROR: keysend requires a destination pubkey: `keysend <dest_pubkey> <amt_msat>`");
                                continue;
                            }
                        };
                        let amt_msat_str = match words.next() {
                            Some(amt) => amt,
                            None => {
                                println!("ERROR: keysend requires an amount in millisatoshis: `keysend <dest_pubkey> <amt_msat>`");
                                continue;
                            }
                        };
                        let amt_msat: u64 = match amt_msat_str.parse() {
                            Ok(amt) => amt,
                            Err(e) => {
                                println!(
                                    "ERROR: couldn't parse amount_msat: {}",
                                    e
                                );
                                continue;
                            }
                        };
                        keysend(
                            &*invoice_payer,
                            dest_pubkey,
                            amt_msat,
                            &*keys_manager,
                            outbound_payments.clone(),
                        );
                    }
                    "getinvoice" => {
                        let amt_str = words.next();
                        if amt_str.is_none() {
                            println!("ERROR: getinvoice requires an amount in millisatoshis");
                            continue;
                        }

                        let amt_msat: Result<u64, _> = amt_str.unwrap().parse();
                        if amt_msat.is_err() {
                            println!("ERROR: getinvoice provided payment amount was not a number");
                            continue;
                        }
                        let expiry_secs_str = words.next();
                        if expiry_secs_str.is_none() {
                            println!(
                                "ERROR: getinvoice requires an expiry in seconds"
                            );
                            continue;
                        }

                        let expiry_secs: Result<u32, _> =
                            expiry_secs_str.unwrap().parse();
                        if expiry_secs.is_err() {
                            println!("ERROR: getinvoice provided expiry was not a number");
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
                        let peer_pubkey_and_ip_addr = words.next();
                        if peer_pubkey_and_ip_addr.is_none() {
                            println!("ERROR: connectpeer requires peer connection info: `connectpeer pubkey@host:port`");
                            continue;
                        }
                        let (pubkey, peer_addr) = match parse_peer_info(
                            peer_pubkey_and_ip_addr.unwrap().to_string(),
                        ) {
                            Ok(info) => info,
                            Err(e) => {
                                println!("{:?}", e.into_inner().unwrap());
                                continue;
                            }
                        };
                        if peer_manager::connect_peer_if_necessary(
                            pubkey,
                            peer_addr,
                            peer_manager.clone(),
                        )
                        .await
                        .is_ok()
                        {
                            println!("SUCCESS: connected to peer {}", pubkey);
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
                            println!("ERROR: closechannel requires a channel ID: `closechannel <channel_id> <peer_pubkey>`");
                            continue;
                        }
                        let channel_id_vec =
                            hex::decode(channel_id_str.unwrap());
                        if channel_id_vec.is_err()
                            || channel_id_vec.as_ref().unwrap().len() != 32
                        {
                            println!("ERROR: couldn't parse channel_id");
                            continue;
                        }
                        let mut channel_id = [0; 32];
                        channel_id.copy_from_slice(&channel_id_vec.unwrap());

                        let peer_pubkey_str = words.next();
                        if peer_pubkey_str.is_none() {
                            println!("ERROR: closechannel requires a peer pubkey: `closechannel <channel_id> <peer_pubkey>`");
                            continue;
                        }
                        let peer_pubkey_vec =
                            match hex::decode(peer_pubkey_str.unwrap()) {
                                Ok(peer_pubkey_vec) => peer_pubkey_vec,
                                Err(err) => {
                                    println!(
                                    "ERROR: couldn't parse peer_pubkey: {err}"
                                );
                                    continue;
                                }
                            };
                        let peer_pubkey =
                            match PublicKey::from_slice(&peer_pubkey_vec) {
                                Ok(peer_pubkey) => peer_pubkey,
                                Err(_) => {
                                    println!(
                                        "ERROR: couldn't parse peer_pubkey"
                                    );
                                    continue;
                                }
                            };

                        close_channel(
                            channel_id,
                            peer_pubkey,
                            channel_manager.clone(),
                        );
                    }
                    "forceclosechannel" => {
                        let channel_id_str = words.next();
                        if channel_id_str.is_none() {
                            println!("ERROR: forceclosechannel requires a channel ID: `forceclosechannel <channel_id> <peer_pubkey>`");
                            continue;
                        }
                        let channel_id_vec =
                            hex::decode(channel_id_str.unwrap());
                        if channel_id_vec.is_err()
                            || channel_id_vec.as_ref().unwrap().len() != 32
                        {
                            println!("ERROR: couldn't parse channel_id");
                            continue;
                        }
                        let mut channel_id = [0; 32];
                        channel_id.copy_from_slice(&channel_id_vec.unwrap());

                        let peer_pubkey_str = words.next();
                        if peer_pubkey_str.is_none() {
                            println!("ERROR: forceclosechannel requires a peer pubkey: `forceclosechannel <channel_id> <peer_pubkey>`");
                            continue;
                        }
                        let peer_pubkey_vec =
                            match hex::decode(peer_pubkey_str.unwrap()) {
                                Ok(peer_pubkey_vec) => peer_pubkey_vec,
                                Err(err) => {
                                    println!(
                                    "ERROR: couldn't parse peer_pubkey: {err}"
                                );
                                    continue;
                                }
                            };
                        let peer_pubkey =
                            match PublicKey::from_slice(&peer_pubkey_vec) {
                                Ok(peer_pubkey) => peer_pubkey,
                                Err(err) => {
                                    println!(
                                    "ERROR: couldn't parse peer_pubkey: {err}"
                                );
                                    continue;
                                }
                            };

                        force_close_channel(
                            channel_id,
                            peer_pubkey,
                            channel_manager.clone(),
                        );
                    }
                    "nodeinfo" => node_info(&channel_manager, &peer_manager),
                    "listpeers" => list_peers(peer_manager.clone()),
                    "signmessage" => {
                        const MSG_STARTPOS: usize = "signmessage".len() + 1;
                        if line.as_bytes().len() <= MSG_STARTPOS {
                            println!("ERROR: signmsg requires a message");
                            continue;
                        }
                        println!(
                            "{:?}",
                            lightning::util::message_signing::sign(
                                &line.as_bytes()[MSG_STARTPOS..],
                                &keys_manager
                                    .get_node_secret(Recipient::Node)
                                    .unwrap()
                            )
                        );
                    }
                    _ => println!(
                        "Unknown command. See `\"help\" for available commands."
                    ),
                }
            }
        }
    }

    fn help() {
        println!("openchannel pubkey@host:port <amt_satoshis>");
        println!("sendpayment <invoice>");
        println!("keysend <dest_pubkey> <amt_msats>");
        println!("getinvoice <amt_msats> <expiry_secs>");
        println!("connectpeer pubkey@host:port");
        println!("listchannels");
        println!("listpayments");
        println!("closechannel <channel_id> <peer_pubkey>");
        println!("forceclosechannel <channel_id> <peer_pubkey>");
        println!("nodeinfo");
        println!("listpeers");
        println!("signmessage <message>");
    }

    fn node_info(
        channel_manager: &LexeChannelManager,
        peer_manager: &LexePeerManager,
    ) {
        println!("\t{{");
        println!("\t\t node_pubkey: {}", channel_manager.get_our_node_id());
        let chans = channel_manager.list_channels();
        println!("\t\t num_channels: {}", chans.len());
        println!(
            "\t\t num_usable_channels: {}",
            chans.iter().filter(|c| c.is_usable).count()
        );
        let local_balance_msat =
            chans.iter().map(|c| c.balance_msat).sum::<u64>();
        println!("\t\t local_balance_msat: {}", local_balance_msat);
        println!("\t\t num_peers: {}", peer_manager.get_peer_node_ids().len());
        println!("\t}},");
    }

    fn list_peers(peer_manager: LexePeerManager) {
        println!("\t{{");
        for pubkey in peer_manager.get_peer_node_ids() {
            println!("\t\t pubkey: {}", pubkey);
        }
        println!("\t}},");
    }

    fn list_channels(
        channel_manager: &LexeChannelManager,
        network_graph: &Arc<NetworkGraphType>,
    ) {
        print!("[");
        for chan_info in channel_manager.list_channels() {
            println!();
            println!("\t{{");
            println!(
                "\t\tchannel_id: {},",
                hex::encode(&chan_info.channel_id[..])
            );
            if let Some(funding_txo) = chan_info.funding_txo {
                println!("\t\tfunding_txid: {},", funding_txo.txid);
            }

            println!(
                "\t\tpeer_pubkey: {},",
                hex::encode(&chan_info.counterparty.node_id.serialize())
            );
            if let Some(node_info) = network_graph
                .read_only()
                .nodes()
                .get(&NodeId::from_pubkey(&chan_info.counterparty.node_id))
            {
                if let Some(announcement) = &node_info.announcement_info {
                    println!(
                        "\t\tpeer_alias: {}",
                        NodeAlias::new(announcement.alias)
                    );
                }
            }

            if let Some(id) = chan_info.short_channel_id {
                println!("\t\tshort_channel_id: {},", id);
            }
            println!("\t\tis_channel_ready: {},", chan_info.is_channel_ready);
            println!(
                "\t\tchannel_value_satoshis: {},",
                chan_info.channel_value_satoshis
            );
            println!("\t\tlocal_balance_msat: {},", chan_info.balance_msat);
            if chan_info.is_usable {
                println!(
                    "\t\tavailable_balance_for_send_msat: {},",
                    chan_info.outbound_capacity_msat
                );
                println!(
                    "\t\tavailable_balance_for_recv_msat: {},",
                    chan_info.inbound_capacity_msat
                );
            }
            println!("\t\tchannel_can_send_payments: {},", chan_info.is_usable);
            println!("\t\tpublic: {},", chan_info.is_public);
            println!("\t}},");
        }
        println!("]");
    }

    fn list_payments(
        inbound_payments: PaymentInfoStorageType,
        outbound_payments: PaymentInfoStorageType,
    ) {
        let inbound = inbound_payments.lock().unwrap();
        let inbound = inbound.deref();
        print!("[");
        for (payment_hash, payment_info) in inbound {
            println!();
            println!("\t{{");
            println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
            println!("\t\tpayment_hash: {},", hex::encode(&payment_hash.0));
            println!("\t\thtlc_direction: inbound,");
            println!(
                "\t\thtlc_status: {},",
                match payment_info.status {
                    HTLCStatus::Pending => "pending",
                    HTLCStatus::Succeeded => "succeeded",
                    HTLCStatus::Failed => "failed",
                }
            );

            println!("\t}},");
        }

        let outbound = outbound_payments.lock().unwrap();
        let outbound = outbound.deref();
        for (payment_hash, payment_info) in outbound {
            println!();
            println!("\t{{");
            println!("\t\tamount_millisatoshis: {},", payment_info.amt_msat);
            println!("\t\tpayment_hash: {},", hex::encode(&payment_hash.0));
            println!("\t\thtlc_direction: outbound,");
            println!(
                "\t\thtlc_status: {},",
                match payment_info.status {
                    HTLCStatus::Pending => "pending",
                    HTLCStatus::Succeeded => "succeeded",
                    HTLCStatus::Failed => "failed",
                }
            );

            println!("\t}},");
        }
        println!("]");
    }

    fn open_channel(
        peer_pubkey: PublicKey,
        channel_amt_sat: u64,
        announced_channel: bool,
        channel_manager: LexeChannelManager,
    ) -> Result<(), ()> {
        let config = UserConfig {
            peer_channel_config_limits: ChannelHandshakeLimits {
                // lnd's max to_self_delay is 2016, so we want to be compatible.
                their_to_self_delay: 2016,
                ..Default::default()
            },
            channel_options: ChannelConfig {
                announced_channel,
                ..Default::default()
            },
            ..Default::default()
        };

        match channel_manager.create_channel(
            peer_pubkey,
            channel_amt_sat,
            0,
            0,
            Some(config),
        ) {
            Ok(_) => {
                println!(
                    "EVENT: initiated channel with peer {}. ",
                    peer_pubkey
                );
                Ok(())
            }
            Err(e) => {
                println!("ERROR: failed to open channel: {:?}", e);
                Err(())
            }
        }
    }

    fn send_payment(
        invoice_payer: &InvoicePayerType,
        invoice: &Invoice,
        payment_storage: PaymentInfoStorageType,
    ) {
        let status = match invoice_payer.pay_invoice(invoice) {
            Ok(_payment_id) => {
                let payee_pubkey = invoice.recover_payee_pub_key();
                let amt_msat = invoice.amount_milli_satoshis().unwrap();
                println!(
                    "EVENT: initiated sending {} msats to {}",
                    amt_msat, payee_pubkey
                );
                print!("> ");
                HTLCStatus::Pending
            }
            Err(PaymentError::Invoice(e)) => {
                println!("ERROR: invalid invoice: {}", e);
                print!("> ");
                return;
            }
            Err(PaymentError::Routing(e)) => {
                println!("ERROR: failed to find route: {}", e.err);
                print!("> ");
                return;
            }
            Err(PaymentError::Sending(e)) => {
                println!("ERROR: failed to send payment: {:?}", e);
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
        payee_pubkey: PublicKey,
        amt_msat: u64,
        keys: &K,
        payment_storage: PaymentInfoStorageType,
    ) {
        let payment_preimage = keys.get_secure_random_bytes();

        let status = match invoice_payer.pay_pubkey(
            payee_pubkey,
            PaymentPreimage(payment_preimage),
            amt_msat,
            40,
        ) {
            Ok(_payment_id) => {
                println!(
                    "EVENT: initiated sending {} msats to {}",
                    amt_msat, payee_pubkey
                );
                print!("> ");
                HTLCStatus::Pending
            }
            Err(PaymentError::Invoice(e)) => {
                println!("ERROR: invalid payee: {}", e);
                print!("> ");
                return;
            }
            Err(PaymentError::Routing(e)) => {
                println!("ERROR: failed to find route: {}", e.err);
                print!("> ");
                return;
            }
            Err(PaymentError::Sending(e)) => {
                println!("ERROR: failed to send payment: {:?}", e);
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
        channel_manager: LexeChannelManager,
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
                println!("SUCCESS: generated invoice: {}", inv);
                inv
            }
            Err(e) => {
                println!("ERROR: failed to create invoice: {:?}", e);
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

    fn close_channel(
        channel_id: [u8; 32],
        counterparty_node_id: PublicKey,
        channel_manager: LexeChannelManager,
    ) {
        match channel_manager.close_channel(&channel_id, &counterparty_node_id)
        {
            Ok(()) => println!("EVENT: initiating channel close"),
            Err(e) => println!("ERROR: failed to close channel: {:?}", e),
        }
    }

    fn force_close_channel(
        channel_id: [u8; 32],
        counterparty_node_id: PublicKey,
        channel_manager: LexeChannelManager,
    ) {
        match channel_manager
            .force_close_channel(&channel_id, &counterparty_node_id)
        {
            Ok(()) => println!("EVENT: initiating channel force-close"),
            Err(e) => println!("ERROR: failed to force-close channel: {:?}", e),
        }
    }

    fn parse_peer_info(
        peer_pubkey_and_ip_addr: String,
    ) -> Result<(PublicKey, SocketAddr), std::io::Error> {
        let mut pubkey_and_addr = peer_pubkey_and_ip_addr.split('@');
        let pubkey = pubkey_and_addr.next();
        let peer_addr_str = pubkey_and_addr.next();
        if peer_addr_str.is_none() || peer_addr_str.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ERROR: incorrectly formatted peer info. Should be formatted as: `pubkey@host:port`",
            ));
        }

        let peer_addr = peer_addr_str
            .unwrap()
            .to_socket_addrs()
            .map(|mut r| r.next());
        if peer_addr.is_err() || peer_addr.as_ref().unwrap().is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ERROR: couldn't parse pubkey@host:port into a socket address",
            ));
        }

        let pubkey = hex_to_compressed_pubkey(pubkey.unwrap());
        if pubkey.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "ERROR: unable to parse given pubkey for node",
            ));
        }

        Ok((pubkey.unwrap(), peer_addr.unwrap().unwrap()))
    }

    fn hex_to_compressed_pubkey(hex: &str) -> Option<PublicKey> {
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
}
