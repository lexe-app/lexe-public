use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::io::Write;
use std::ops::Deref;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use bitcoin::blockdata::constants::genesis_block;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::encode;
use bitcoin::network::constants::Network;
use bitcoin::secp256k1::PublicKey;
use bitcoin::secp256k1::Secp256k1;
use bitcoin_bech32::WitnessProgram;
use lightning::chain;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
};
use lightning::chain::chainmonitor;
use lightning::chain::keysinterface::{
    InMemorySigner, KeysInterface, KeysManager, Recipient,
};
use lightning::chain::{BestBlock, Filter, Watch};
use lightning::ln::channelmanager;
use lightning::ln::channelmanager::{ChainParameters, SimpleArcChannelManager};
use lightning::ln::peer_handler::{
    IgnoringMessageHandler, MessageHandler, SimpleArcPeerManager,
};
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning::routing::gossip::{NetworkGraph, NodeId, P2PGossipSync};
use lightning::routing::scoring::ProbabilisticScorer;
use lightning::util::config::UserConfig;
use lightning::util::events::{Event, PaymentPurpose};
use lightning::util::persist::Persister;
use lightning_background_processor::BackgroundProcessor;
use lightning_block_sync::init;
use lightning_block_sync::poll;
use lightning_block_sync::SpvClient;
use lightning_block_sync::UnboundedCache;
use lightning_invoice::payment;
use lightning_invoice::utils::DefaultRouter;
use lightning_net_tokio::SocketDescriptor;
use lightning_rapid_gossip_sync::RapidGossipSync;

use anyhow::{bail, ensure, Context};
use rand::{thread_rng, Rng};
use reqwest::Client;

use crate::api::{Instance, Node};
use crate::bitcoind_client::BitcoindClient;
use crate::logger::StdOutLogger;
use crate::persister::PostgresPersister;

mod api;
pub mod bitcoind_client;
mod cli;
mod convert;
mod hex_utils;
mod logger;
mod persister;

enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

struct MillisatAmount(Option<u64>);

impl fmt::Display for MillisatAmount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            Some(amt) => write!(f, "{}", amt),
            None => write!(f, "unknown"),
        }
    }
}

struct PaymentInfo {
    preimage: Option<PaymentPreimage>,
    secret: Option<PaymentSecret>,
    status: HTLCStatus,
    amt_msat: MillisatAmount,
}

type PaymentInfoStorageType = Arc<Mutex<HashMap<PaymentHash, PaymentInfo>>>;

type ChainMonitorType = chainmonitor::ChainMonitor<
    InMemorySigner,
    Arc<dyn Filter + Send + Sync>,
    Arc<BitcoindClient>,
    Arc<BitcoindClient>,
    Arc<StdOutLogger>,
    Arc<PostgresPersister>,
>;

type PeerManagerType = SimpleArcPeerManager<
    SocketDescriptor,
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    dyn chain::Access + Send + Sync,
    StdOutLogger,
>;

type ChannelManagerType = SimpleArcChannelManager<
    ChainMonitorType,
    BitcoindClient,
    BitcoindClient,
    StdOutLogger,
>;

type InvoicePayerType<E> = payment::InvoicePayer<
    Arc<ChannelManagerType>,
    RouterType,
    Arc<Mutex<ProbabilisticScorerType>>,
    Arc<StdOutLogger>,
    E,
>;

type ProbabilisticScorerType =
    ProbabilisticScorer<Arc<NetworkGraphType>, LoggerType>;

type RouterType = DefaultRouter<Arc<NetworkGraphType>, LoggerType>;

type GossipSync<P, G, A, L> = lightning_background_processor::GossipSync<
    P,
    Arc<RapidGossipSync<G, L>>,
    G,
    A,
    L,
>;

type NetworkGraphType = NetworkGraph<LoggerType>;

type LoggerType = Arc<StdOutLogger>;

struct NodeAlias<'a>(&'a [u8; 32]);

impl fmt::Display for NodeAlias<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let alias = self
            .0
            .iter()
            .map(|b| *b as char)
            .take_while(|c| *c != '\0')
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect::<String>();
        write!(f, "{}", alias)
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_ldk_events(
    channel_manager: &Arc<ChannelManagerType>,
    bitcoind_client: &BitcoindClient,
    network_graph: &NetworkGraphType,
    keys_manager: &KeysManager,
    inbound_payments: &PaymentInfoStorageType,
    outbound_payments: &PaymentInfoStorageType,
    network: Network,
    event: &Event,
) {
    match event {
        Event::FundingGenerationReady {
            temporary_channel_id,
            counterparty_node_id,
            channel_value_satoshis,
            output_script,
            ..
        } => {
            // Construct the raw transaction with one output, that is paid the
            // amount of the channel.
            let addr = WitnessProgram::from_scriptpubkey(
                &output_script[..],
                match network {
                    Network::Bitcoin => {
                        bitcoin_bech32::constants::Network::Bitcoin
                    }
                    Network::Testnet => {
                        bitcoin_bech32::constants::Network::Testnet
                    }
                    Network::Regtest => {
                        bitcoin_bech32::constants::Network::Regtest
                    }
                    Network::Signet => {
                        bitcoin_bech32::constants::Network::Signet
                    }
                },
            )
            .expect("Lightning funding tx should always be to a SegWit output")
            .to_address();
            let mut outputs = vec![HashMap::with_capacity(1)];
            outputs[0]
                .insert(addr, *channel_value_satoshis as f64 / 100_000_000.0);
            let raw_tx = bitcoind_client.create_raw_transaction(outputs).await;

            // Have your wallet put the inputs into the transaction such that
            // the output is satisfied.
            let funded_tx = bitcoind_client.fund_raw_transaction(raw_tx).await;

            // Sign the final funding transaction and broadcast it.
            let signed_tx = bitcoind_client
                .sign_raw_transaction_with_wallet(funded_tx.hex)
                .await;
            assert!(signed_tx.complete);
            let final_tx: Transaction = encode::deserialize(
                &hex_utils::to_vec(&signed_tx.hex).unwrap(),
            )
            .unwrap();
            // Give the funding transaction back to LDK for opening the channel.
            if channel_manager
                .funding_transaction_generated(
                    temporary_channel_id,
                    counterparty_node_id,
                    final_tx,
                )
                .is_err()
            {
                println!(
					"\nERROR: Channel went away before we could fund it. The peer disconnected or refused the channel.");
                print!("> ");
                io::stdout().flush().unwrap();
            }
        }
        Event::PaymentReceived {
            payment_hash,
            purpose,
            amount_msat,
        } => {
            println!(
				"\nEVENT: received payment from payment hash {} of {} millisatoshis",
				hex_utils::hex_str(&payment_hash.0),
				amount_msat,
			);
            print!("> ");
            io::stdout().flush().unwrap();
            let payment_preimage = match purpose {
                PaymentPurpose::InvoicePayment {
                    payment_preimage, ..
                } => *payment_preimage,
                PaymentPurpose::SpontaneousPayment(preimage) => Some(*preimage),
            };
            channel_manager.claim_funds(payment_preimage.unwrap());
        }
        Event::PaymentClaimed {
            payment_hash,
            purpose,
            amount_msat,
        } => {
            println!(
				"\nEVENT: claimed payment from payment hash {} of {} millisatoshis",
				hex_utils::hex_str(&payment_hash.0),
				amount_msat,
			);
            print!("> ");
            io::stdout().flush().unwrap();
            let (payment_preimage, payment_secret) = match purpose {
                PaymentPurpose::InvoicePayment {
                    payment_preimage,
                    payment_secret,
                    ..
                } => (*payment_preimage, Some(*payment_secret)),
                PaymentPurpose::SpontaneousPayment(preimage) => {
                    (Some(*preimage), None)
                }
            };
            let mut payments = inbound_payments.lock().unwrap();
            match payments.entry(*payment_hash) {
                Entry::Occupied(mut e) => {
                    let payment = e.get_mut();
                    payment.status = HTLCStatus::Succeeded;
                    payment.preimage = payment_preimage;
                    payment.secret = payment_secret;
                }
                Entry::Vacant(e) => {
                    e.insert(PaymentInfo {
                        preimage: payment_preimage,
                        secret: payment_secret,
                        status: HTLCStatus::Succeeded,
                        amt_msat: MillisatAmount(Some(*amount_msat)),
                    });
                }
            }
        }
        Event::PaymentSent {
            payment_preimage,
            payment_hash,
            fee_paid_msat,
            ..
        } => {
            let mut payments = outbound_payments.lock().unwrap();
            for (hash, payment) in payments.iter_mut() {
                if *hash == *payment_hash {
                    payment.preimage = Some(*payment_preimage);
                    payment.status = HTLCStatus::Succeeded;
                    println!(
						"\nEVENT: successfully sent payment of {} millisatoshis{} from \
								 payment hash {:?} with preimage {:?}",
						payment.amt_msat,
						if let Some(fee) = fee_paid_msat {
							format!(" (fee {} msat)", fee)
						} else {
							"".to_string()
						},
						hex_utils::hex_str(&payment_hash.0),
						hex_utils::hex_str(&payment_preimage.0)
					);
                    print!("> ");
                    io::stdout().flush().unwrap();
                }
            }
        }
        Event::OpenChannelRequest { .. } => {
            // Unreachable, we don't set manually_accept_inbound_channels
        }
        Event::PaymentPathSuccessful { .. } => {}
        Event::PaymentPathFailed { .. } => {}
        Event::PaymentFailed { payment_hash, .. } => {
            print!(
				"\nEVENT: Failed to send payment to payment hash {:?}: exhausted payment retry attempts",
				hex_utils::hex_str(&payment_hash.0)
			);
            print!("> ");
            io::stdout().flush().unwrap();

            let mut payments = outbound_payments.lock().unwrap();
            if payments.contains_key(payment_hash) {
                let payment = payments.get_mut(payment_hash).unwrap();
                payment.status = HTLCStatus::Failed;
            }
        }
        Event::PaymentForwarded {
            prev_channel_id,
            next_channel_id,
            fee_earned_msat,
            claim_from_onchain_tx,
        } => {
            let read_only_network_graph = network_graph.read_only();
            let nodes = read_only_network_graph.nodes();
            let channels = channel_manager.list_channels();

            let node_str = |channel_id: &Option<[u8; 32]>| match channel_id {
                None => String::new(),
                Some(channel_id) => {
                    match channels.iter().find(|c| c.channel_id == *channel_id)
                    {
                        None => String::new(),
                        Some(channel) => {
                            match nodes.get(&NodeId::from_pubkey(
                                &channel.counterparty.node_id,
                            )) {
                                None => " from private node".to_string(),
                                Some(node) => match &node.announcement_info {
                                    None => " from unnamed node".to_string(),
                                    Some(announcement) => {
                                        format!(
                                            " from node {}",
                                            NodeAlias(&announcement.alias)
                                        )
                                    }
                                },
                            }
                        }
                    }
                }
            };
            let channel_str = |channel_id: &Option<[u8; 32]>| {
                channel_id
                    .map(|channel_id| {
                        format!(
                            " with channel {}",
                            hex_utils::hex_str(&channel_id)
                        )
                    })
                    .unwrap_or_default()
            };
            let from_prev_str = format!(
                "{}{}",
                node_str(prev_channel_id),
                channel_str(prev_channel_id)
            );
            let to_next_str = format!(
                "{}{}",
                node_str(next_channel_id),
                channel_str(next_channel_id)
            );

            let from_onchain_str = if *claim_from_onchain_tx {
                "from onchain downstream claim"
            } else {
                "from HTLC fulfill message"
            };
            if let Some(fee_earned) = fee_earned_msat {
                println!(
                    "\nEVENT: Forwarded payment{}{}, earning {} msat {}",
                    from_prev_str, to_next_str, fee_earned, from_onchain_str
                );
            } else {
                println!(
                    "\nEVENT: Forwarded payment{}{}, claiming onchain {}",
                    from_prev_str, to_next_str, from_onchain_str
                );
            }
            print!("> ");
            io::stdout().flush().unwrap();
        }
        Event::PendingHTLCsForwardable { time_forwardable } => {
            let forwarding_channel_manager = channel_manager.clone();
            let min = time_forwardable.as_millis() as u64;
            tokio::spawn(async move {
                let millis_to_sleep =
                    thread_rng().gen_range(min, min * 5) as u64;
                tokio::time::sleep(Duration::from_millis(millis_to_sleep))
                    .await;
                forwarding_channel_manager.process_pending_htlc_forwards();
            });
        }
        Event::SpendableOutputs { outputs } => {
            let destination_address = bitcoind_client.get_new_address().await;
            let output_descriptors = &outputs.iter().collect::<Vec<_>>();
            let tx_feerate = bitcoind_client
                .get_est_sat_per_1000_weight(ConfirmationTarget::Normal);
            let spending_tx = keys_manager
                .spend_spendable_outputs(
                    output_descriptors,
                    Vec::new(),
                    destination_address.script_pubkey(),
                    tx_feerate,
                    &Secp256k1::new(),
                )
                .unwrap();
            bitcoind_client.broadcast_transaction(&spending_tx);
        }
        Event::ChannelClosed {
            channel_id,
            reason,
            user_channel_id: _,
        } => {
            println!(
                "\nEVENT: Channel {} closed due to: {:?}",
                hex_utils::hex_str(channel_id),
                reason
            );
            print!("> ");
            io::stdout().flush().unwrap();
        }
        Event::DiscardFunding { .. } => {
            // A "real" node should probably "lock" the UTXOs spent in funding
            // transactions until the funding transaction either confirms, or
            // this event is generated.
        }
    }
}

async fn start_ldk() -> anyhow::Result<()> {
    let args = match cli::parse_startup_args() {
        Ok(user_args) => user_args,
        Err(()) => bail!("Could not parse startup args"),
    };

    // Initialize our bitcoind client.
    let bitcoind_client = match BitcoindClient::new(
        args.bitcoind_rpc_host.clone(),
        args.bitcoind_rpc_port,
        args.bitcoind_rpc_username.clone(),
        args.bitcoind_rpc_password.clone(),
        tokio::runtime::Handle::current(),
    )
    .await
    {
        Ok(client) => Arc::new(client),
        Err(e) => {
            bail!("Failed to connect to bitcoind client: {}", e);
        }
    };

    // Check that the bitcoind we've connected to is running the network we
    // expect
    let bitcoind_chain = bitcoind_client.get_blockchain_info().await.chain;
    if bitcoind_chain
        != match args.network {
            bitcoin::Network::Bitcoin => "main",
            bitcoin::Network::Testnet => "test",
            bitcoin::Network::Regtest => "regtest",
            bitcoin::Network::Signet => "signet",
        }
    {
        bail!(
            "Chain argument ({}) didn't match bitcoind chain ({})",
            args.network,
            bitcoind_chain
        );
    }

    // ## Setup
    // Step 1: Initialize the FeeEstimator

    // BitcoindClient implements the FeeEstimator trait, so it'll act as our fee
    // estimator.
    let fee_estimator = bitcoind_client.clone();

    // Step 2: Initialize the Logger
    let logger = Arc::new(StdOutLogger {});

    // Step 3: Initialize the BroadcasterInterface

    // BitcoindClient implements the BroadcasterInterface trait, so it'll act as
    // our transaction broadcaster.
    let broadcaster = bitcoind_client.clone();

    // Step 4: Initialize the KeysManager

    // Fetch our node pubkey and instance data from the data store

    // TODO take in the user id from program args
    let client = Client::new();
    let user_id = 1;
    // TODO(sgx) Insert this enclave's measurement
    let measurement = String::from("measurement");
    let (node_res, instance_res) = tokio::join!(
        api::get_node(&client, user_id),
        api::get_instance(&client, user_id, measurement.clone()),
    );
    let node_opt = node_res.context("Error while fetching node")?;
    let instance_opt = instance_res.context("Error while fetching instance")?;

    let (pubkey, keys_manager) = match (node_opt, instance_opt) {
        (Some(node), Some(instance)) => {
            println!("Found existing instance in DB");

            // TODO(decrypt): Decrypt under enclave sealing key to get the seed
            let seed = instance.seed;

            // Validate the seed
            ensure!(seed.len() == 32, "Incorrect seed length");
            let mut seed_buf = [0; 32];
            seed_buf.copy_from_slice(&seed);

            // Derive the node pubkey from the seed
            let keys_manager = init_key_manager(&seed_buf);
            let derived_pubkey = convert::get_pubkey(&keys_manager)
                .context("Could not get derive our pubkey from seed")?;

            // Validate the pubkey returned from the DB against the derived one
            let given_pubkey = PublicKey::from_str(&node.public_key)
                .context("Could not deserialize PublicKey from LowerHex")?;
            ensure!(
                given_pubkey == derived_pubkey,
                "Derived pubkey doesn't match the pubkey returned from the DB"
            );

            // Check the hex encodings as well
            let given_pubkey_hex = &node.public_key;
            let derived_pubkey_hex = &format!("{:x}", derived_pubkey);
            ensure!(
                given_pubkey_hex == derived_pubkey_hex,
                "Derived pubkey string doesn't match given pubkey string"
            );

            (derived_pubkey, keys_manager)
        }
        (None, Some(_instance)) => {
            // This should never happen; the instance table is (non-null)
            // foreign keyed to the node table.
            bail!("Existing instances should always have existing nodes")
        }
        (Some(node), None) => {
            // If this is the second startup for a completely new user,
            //
            // If this instance was bootstrapped from an old instance, the old
            // instance should have created the DB entry for this new instance,
            // which contains the encrypted seed (sealed under this instance's
            // enclave measurement) required for recovering the Lightning node.
            //
            // If a previous run of this instance generated a seed + node (e.g.
            // for a completely new user), the Node and Instance should have
            // been persisted together. Like above, we cannot recover the node
            // because we do not have access to the encrypted seed.
            bail!("Missing Instance data for node {}", node.public_key)
        }
        (None, None) => {
            // No node exists yet, create a new one
            println!("Generating new seed");
            let mut new_seed = [0; 32];
            rand::thread_rng().fill_bytes(&mut new_seed);

            // Derive pubkey
            let keys_manager = init_key_manager(&new_seed);
            let pubkey = convert::get_pubkey(&keys_manager)
                .context("Could not get derive our pubkey from seed")?;
            let pubkey_hex = format!("{:x}", pubkey);

            // Build API structs for persisting the new node + instance
            let node = Node {
                public_key: pubkey_hex.clone(),
                user_id,
            };
            // TODO(crypto) id derivation scheme;
            // probably hash(pubkey || measurement)
            let id = format!("{}_{}", pubkey_hex, measurement);
            let instance = Instance {
                id,
                measurement,
                node_public_key: pubkey_hex,
                // TODO (sgx): Seal seed under this enclave's pubkey
                seed: new_seed.to_vec(),
            };

            // Persist the node along with the instance
            // TODO query create_node_and_instance endpoint
            api::create_node(&client, node)
                .await
                .context("Could not persist newly created node")?;
            api::create_instance(&client, instance)
                .await
                .context("Could not persist newly created instance")?;

            (pubkey, keys_manager)
        }
    };
    let keys_manager = Arc::new(keys_manager);

    // Step 5: Initialize Persister
    let persister = Arc::new(PostgresPersister::new(&client, pubkey));

    // Step 6: Initialize the ChainMonitor
    let chain_monitor: Arc<ChainMonitorType> =
        Arc::new(chainmonitor::ChainMonitor::new(
            None,
            broadcaster.clone(),
            logger.clone(),
            fee_estimator.clone(),
            persister.clone(),
        ));

    // Step 7: Retrieve ChannelMonitor state from DB
    let mut channelmonitors = persister
        .read_channel_monitors(keys_manager.clone())
        .await
        .context("Could not read channel monitors")?;

    // Step 8: Initialize the ChannelManager
    let mut user_config = UserConfig::default();
    user_config
        .peer_channel_config_limits
        .force_announced_channel_preference = false;
    let mut restarting_node = true;
    let channel_manager_opt = persister
        .read_channel_manager(
            &mut channelmonitors,
            keys_manager.clone(),
            fee_estimator.clone(),
            chain_monitor.clone(),
            broadcaster.clone(),
            logger.clone(),
            user_config,
        )
        .await
        .context("Could not read ChannelManager from DB")?;
    let (channel_manager_blockhash, channel_manager) = match channel_manager_opt
    {
        Some((blockhash, mgr)) => (blockhash, mgr),
        None => {
            // We're starting a fresh node.
            restarting_node = false;
            let getinfo_resp = bitcoind_client.get_blockchain_info().await;

            let chain_params = ChainParameters {
                network: args.network,
                best_block: BestBlock::new(
                    getinfo_resp.latest_blockhash,
                    getinfo_resp.latest_height as u32,
                ),
            };
            let fresh_channel_manager = channelmanager::ChannelManager::new(
                fee_estimator.clone(),
                chain_monitor.clone(),
                broadcaster.clone(),
                logger.clone(),
                keys_manager.clone(),
                user_config,
                chain_params,
            );
            (getinfo_resp.latest_blockhash, fresh_channel_manager)
        }
    };

    // Step 9: Sync ChannelMonitors and ChannelManager to chain tip
    let mut chain_listener_channel_monitors = Vec::new();
    let mut cache = UnboundedCache::new();
    let mut chain_tip: Option<poll::ValidatedBlockHeader> = None;
    if restarting_node {
        let mut chain_listeners = vec![(
            channel_manager_blockhash,
            &channel_manager as &dyn chain::Listen,
        )];

        for (blockhash, channel_monitor) in channelmonitors.drain(..) {
            let outpoint = channel_monitor.get_funding_txo().0;
            chain_listener_channel_monitors.push((
                blockhash,
                (
                    channel_monitor,
                    broadcaster.clone(),
                    fee_estimator.clone(),
                    logger.clone(),
                ),
                outpoint,
            ));
        }

        for monitor_listener_info in chain_listener_channel_monitors.iter_mut()
        {
            chain_listeners.push((
                monitor_listener_info.0,
                &monitor_listener_info.1 as &dyn chain::Listen,
            ));
        }
        chain_tip = Some(
            init::synchronize_listeners(
                &mut bitcoind_client.deref(),
                args.network,
                &mut cache,
                chain_listeners,
            )
            .await
            .unwrap(),
        );
    }

    // Step 10: Give ChannelMonitors to ChainMonitor
    for item in chain_listener_channel_monitors.drain(..) {
        let channel_monitor = item.1 .0;
        let funding_outpoint = item.2;
        chain_monitor
            .watch_channel(funding_outpoint, channel_monitor)
            .unwrap();
    }

    // Step 11: Optional: Initialize the P2PGossipSync
    let genesis = genesis_block(args.network).header.block_hash();
    let network_graph = persister
        .read_network_graph(genesis, logger.clone())
        .await
        .context("Could not read network graph")?;
    let network_graph = Arc::new(network_graph);
    let gossip_sync = Arc::new(P2PGossipSync::new(
        Arc::clone(&network_graph),
        None::<Arc<dyn chain::Access + Send + Sync>>,
        logger.clone(),
    ));

    // Step 12: Initialize the PeerManager
    let channel_manager: Arc<ChannelManagerType> = Arc::new(channel_manager);
    let mut ephemeral_bytes = [0; 32];
    rand::thread_rng().fill_bytes(&mut ephemeral_bytes);
    let lightning_msg_handler = MessageHandler {
        chan_handler: channel_manager.clone(),
        route_handler: gossip_sync.clone(),
    };
    let peer_manager: Arc<PeerManagerType> = Arc::new(PeerManagerType::new(
        lightning_msg_handler,
        keys_manager.get_node_secret(Recipient::Node).unwrap(),
        &ephemeral_bytes,
        logger.clone(),
        Arc::new(IgnoringMessageHandler {}),
    ));

    // ## Running LDK
    // Step 13: Initialize networking

    let peer_manager_connection_handler = peer_manager.clone();
    let listening_port = args.ldk_peer_listening_port;
    let stop_listen_connect = Arc::new(AtomicBool::new(false));
    let stop_listen = Arc::clone(&stop_listen_connect);
    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", listening_port))
			.await
			.expect("Failed to bind to listen port - is something else already listening on it?");
        loop {
            let peer_mgr = peer_manager_connection_handler.clone();
            let tcp_stream = listener.accept().await.unwrap().0;
            if stop_listen.load(Ordering::Acquire) {
                return;
            }
            tokio::spawn(async move {
                lightning_net_tokio::setup_inbound(
                    peer_mgr.clone(),
                    tcp_stream,
                )
                .await;
            });
        }
    });

    // Step 14: Connect and Disconnect Blocks
    if chain_tip.is_none() {
        chain_tip = Some(
            init::validate_best_block_header(&mut bitcoind_client.deref())
                .await
                .unwrap(),
        );
    }
    let channel_manager_listener = channel_manager.clone();
    let chain_monitor_listener = chain_monitor.clone();
    let bitcoind_block_source = bitcoind_client.clone();
    let network = args.network;
    tokio::spawn(async move {
        let mut derefed = bitcoind_block_source.deref();
        let chain_poller = poll::ChainPoller::new(&mut derefed, network);
        let chain_listener = (chain_monitor_listener, channel_manager_listener);
        let mut spv_client = SpvClient::new(
            chain_tip.unwrap(),
            chain_poller,
            &mut cache,
            &chain_listener,
        );
        loop {
            spv_client.poll_best_tip().await.unwrap();
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Step 15: Handle LDK Events
    let channel_manager_event_listener = channel_manager.clone();
    let keys_manager_listener = keys_manager.clone();
    // TODO: persist payment info to disk
    let inbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let outbound_payments: PaymentInfoStorageType =
        Arc::new(Mutex::new(HashMap::new()));
    let inbound_pmts_for_events = inbound_payments.clone();
    let outbound_pmts_for_events = outbound_payments.clone();
    let network = args.network;
    let bitcoind_rpc = bitcoind_client.clone();
    let network_graph_events = network_graph.clone();
    let handle = tokio::runtime::Handle::current();
    let event_handler = move |event: &Event| {
        handle.block_on(handle_ldk_events(
            &channel_manager_event_listener,
            &bitcoind_rpc,
            &network_graph_events,
            &keys_manager_listener,
            &inbound_pmts_for_events,
            &outbound_pmts_for_events,
            network,
            event,
        ));
    };

    // Step 16: Initialize routing ProbabilisticScorer
    let scorer = persister
        .read_probabilistic_scorer(
            Arc::clone(&network_graph),
            Arc::clone(&logger),
        )
        .await
        .context("Could not read probabilistic scorer")?;
    let scorer = Arc::new(Mutex::new(scorer));
    let inner_scorer = Arc::clone(&scorer);
    let inner_persister = Arc::clone(&persister);
    tokio::spawn(async move {
        // TODO This isn't realistic for the Lexe usecase
        let mut interval = tokio::time::interval(Duration::from_secs(600));

        loop {
            interval.tick().await;

            let _ =
                inner_persister.persist_scorer(&inner_scorer).map_err(|e| {
                    println!("Warning: Failed to persist scorer: {:#}", e)
                });
        }
    });

    // Step 17: Create InvoicePayer
    let router = DefaultRouter::new(
        network_graph.clone(),
        logger.clone(),
        keys_manager.get_secure_random_bytes(),
    );
    let invoice_payer = Arc::new(InvoicePayerType::new(
        channel_manager.clone(),
        router,
        scorer.clone(),
        logger.clone(),
        event_handler,
        payment::Retry::Timeout(Duration::from_secs(10)),
    ));

    // Step 18: Background Processing
    let background_processor = BackgroundProcessor::start(
        Arc::clone(&persister),
        invoice_payer.clone(),
        chain_monitor.clone(),
        channel_manager.clone(),
        GossipSync::P2P(gossip_sync.clone()),
        peer_manager.clone(),
        logger.clone(),
        Some(scorer.clone()),
    );

    // Regularly reconnect to channel peers.
    let connect_channel_manager = Arc::clone(&channel_manager);
    let connect_peer_manager = Arc::clone(&peer_manager);
    let stop_connect = Arc::clone(&stop_listen_connect);
    let connect_persister = Arc::clone(&persister);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            match connect_persister.read_channel_peers().await {
                Ok(cp_vec) => {
                    let peers = connect_peer_manager.get_peer_node_ids();
                    for node_id in connect_channel_manager
                        .list_channels()
                        .iter()
                        .map(|chan| chan.counterparty.node_id)
                        .filter(|id| !peers.contains(id))
                    {
                        if stop_connect.load(Ordering::Acquire) {
                            return;
                        }
                        for (pubkey, peer_addr) in cp_vec.iter() {
                            if *pubkey == node_id {
                                let _ = cli::do_connect_peer(
                                    *pubkey,
                                    *peer_addr,
                                    Arc::clone(&connect_peer_manager),
                                )
                                .await;
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("ERROR: Could not read channel peers: {}", e)
                }
            }
        }
    });

    // Regularly broadcast our node_announcement. This is only required (or
    // possible) if we have some public channels, and is only useful if we have
    // public listen address(es) to announce. In a production environment, this
    // should occur only after the announcement of new channels to avoid churn
    // in the global network graph.
    let chan_manager = Arc::clone(&channel_manager);
    let network = args.network;
    if !args.ldk_announced_listen_addr.is_empty() {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                chan_manager.broadcast_node_announcement(
                    [0; 3],
                    args.ldk_announced_node_name,
                    args.ldk_announced_listen_addr.clone(),
                );
            }
        });
    }

    // Start the CLI.
    cli::poll_for_user_input(
        Arc::clone(&invoice_payer),
        Arc::clone(&peer_manager),
        Arc::clone(&channel_manager),
        Arc::clone(&keys_manager),
        Arc::clone(&network_graph),
        inbound_payments,
        outbound_payments,
        persister.clone(),
        network,
    )
    .await;

    // Disconnect our peers and stop accepting new connections. This ensures we
    // don't continue updating our channel data after we've stopped the
    // background processor.
    stop_listen_connect.store(true, Ordering::Release);
    peer_manager.disconnect_all_peers();

    // Stop the background processor.
    background_processor.stop().unwrap();

    Ok(())
}

#[tokio::main]
pub async fn main() {
    match start_ldk().await {
        Ok(()) => {}
        Err(e) => println!("Error: {:#}", e),
    }
}

/// Securely initializes a KeyManager from a given seed
fn init_key_manager(seed: &[u8; 32]) -> KeysManager {
    // FIXME(randomness): KeysManager::new() MUST be given a unique
    // `starting_time_secs` and `starting_time_nanos` for security. Since secure
    // timekeeping within an enclave is difficult, we should just take a
    // (securely) random u64, u32 instead. See KeysManager::new() for details.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Literally 1984");

    KeysManager::new(seed, now.as_secs(), now.subsec_nanos())
}
