use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Context};
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::encode;
use bitcoin::hash_types::{BlockHash, Txid};
use bitcoin::util::address::Address;
use common::cli::{BitcoindRpcInfo, Network};
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
};
use lightning_block_sync::http::HttpEndpoint;
use lightning_block_sync::rpc::RpcClient;
use lightning_block_sync::{
    AsyncBlockSourceResult, BlockData, BlockHeaderData, BlockSource,
};
use tokio::time;
use tracing::{debug, error};

mod types;

pub use types::*;

const POLL_FEE_ESTIMATE_INTERVAL: Duration = Duration::from_secs(60);
/// The minimum feerate we are allowed to send, as specified by LDK.
const MIN_FEERATE: u32 = 253;

pub struct LexeBitcoind {
    rpc_client: Arc<RpcClient>,
    background_fees: Arc<AtomicU32>,
    normal_fees: Arc<AtomicU32>,
    high_prio_fees: Arc<AtomicU32>,
    shutdown: ShutdownChannel,
}

impl LexeBitcoind {
    pub async fn init(
        bitcoind_rpc: BitcoindRpcInfo,
        network: Network,
        shutdown: ShutdownChannel,
    ) -> anyhow::Result<Self> {
        debug!(%network, "Initializing bitcoind client");

        let credentials = bitcoind_rpc.base64_credentials();
        let http_endpoint = HttpEndpoint::for_host(bitcoind_rpc.host)
            .with_port(bitcoind_rpc.port);
        let rpc_client = RpcClient::new(&credentials, http_endpoint)
            .context("Could not initialize RPC client")?;
        let rpc_client = Arc::new(rpc_client);

        let background_fees = Arc::new(AtomicU32::new(MIN_FEERATE));
        let normal_fees = Arc::new(AtomicU32::new(2000));
        let high_prio_fees = Arc::new(AtomicU32::new(5000));

        let client = Self {
            rpc_client,
            background_fees,
            normal_fees,
            high_prio_fees,
            shutdown,
        };

        // Make an initial test call to check that the RPC client is working
        // correctly, and also check that the bitcoind we've connected to is
        // running the network we expect
        let bitcoind_chain = client
            .get_blockchain_info()
            .await
            .context("Could not get blockchain info")?
            .chain;
        // getblockchaininfo truncates mainnet and testnet. Change these to
        // match the cli::Network FromStr / Display impls.
        let bitcoind_chain = match bitcoind_chain.as_str() {
            "main" => "bitcoin",
            "test" => "testnet",
            other => other,
        };
        let chain_str = network.to_str();
        ensure!(
            bitcoind_chain == chain_str,
            "Chain argument ({}) didn't match bitcoind chain ({})",
            chain_str,
            bitcoind_chain,
        );

        debug!("bitcoind client init done");
        Ok(client)
    }

    pub fn spawn_refresh_fees_task(&self) -> LxTask<()> {
        let rpc_client = self.rpc_client.clone();
        let background_fees = self.background_fees.clone();
        let normal_fees = self.normal_fees.clone();
        let high_prio_fees = self.high_prio_fees.clone();
        let mut shutdown = self.shutdown.clone();

        // TODO(max): Instrument with shutdown
        LxTask::spawn_named("refresh fees", async move {
            let mut poll_interval = time::interval(POLL_FEE_ESTIMATE_INTERVAL);

            loop {
                tokio::select! {
                    _ = poll_interval.tick() => {}
                    () = shutdown.recv() => break,
                }

                let poll_res = Self::refresh_fees(
                    background_fees.as_ref(),
                    normal_fees.as_ref(),
                    high_prio_fees.as_ref(),
                    rpc_client.as_ref(),
                )
                .await;

                match poll_res {
                    Ok(()) => {}
                    Err(e) => {
                        error!("Error while polling fee estimates: {:#}", e);
                    }
                }
            }
        })
    }

    async fn refresh_fees(
        background_fees: &AtomicU32,
        normal_fees: &AtomicU32,
        high_prio_fees: &AtomicU32,
        rpc_client: &RpcClient,
    ) -> anyhow::Result<()> {
        let background_estimate = {
            let background_conf_target = serde_json::json!(144);
            let background_estimate_mode = serde_json::json!("ECONOMICAL");
            let resp = rpc_client
                .call_method::<FeeResponse>(
                    "estimatesmartfee",
                    &[background_conf_target, background_estimate_mode],
                )
                .await
                .context("Failed to get background estimate")?;
            match resp.feerate_sat_per_kw {
                Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
                None => MIN_FEERATE,
            }
        };

        let normal_estimate = {
            let normal_conf_target = serde_json::json!(18);
            let normal_estimate_mode = serde_json::json!("ECONOMICAL");
            let resp = rpc_client
                .call_method::<FeeResponse>(
                    "estimatesmartfee",
                    &[normal_conf_target, normal_estimate_mode],
                )
                .await
                .context("Failed to get normal estimate")?;
            match resp.feerate_sat_per_kw {
                Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
                None => 2000,
            }
        };

        let high_prio_estimate = {
            let high_prio_conf_target = serde_json::json!(6);
            let high_prio_estimate_mode = serde_json::json!("CONSERVATIVE");
            let resp = rpc_client
                .call_method::<FeeResponse>(
                    "estimatesmartfee",
                    &[high_prio_conf_target, high_prio_estimate_mode],
                )
                .await
                .context("Failed to get high priority estimate")?;

            match resp.feerate_sat_per_kw {
                Some(feerate) => std::cmp::max(feerate, MIN_FEERATE),
                None => 5000,
            }
        };

        background_fees.store(background_estimate, Ordering::Release);
        normal_fees.store(normal_estimate, Ordering::Release);
        high_prio_fees.store(high_prio_estimate, Ordering::Release);

        Ok(())
    }

    pub async fn create_raw_transaction(
        &self,
        outputs: Vec<HashMap<String, f64>>,
    ) -> anyhow::Result<RawTx> {
        let outputs_json = serde_json::json!(outputs);
        self.rpc_client
            .call_method::<RawTx>(
                "createrawtransaction",
                &[serde_json::json!([]), outputs_json],
            )
            .await
            .context("createrawtransaction RPC call failed")
    }

    pub async fn fund_raw_transaction(
        &self,
        raw_tx: RawTx,
    ) -> anyhow::Result<FundedTx> {
        let raw_tx_json = serde_json::json!(raw_tx.0);
        let options = serde_json::json!({
            // LDK gives us feerates in satoshis per KW but Bitcoin Core here
            // expects fees denominated in satoshis per vB. First we need to
            // multiply by 4 to convert weight units to virtual bytes, then
            // divide by 1000 to convert KvB to vB.
            "fee_rate": self.get_est_sat_per_1000_weight(ConfirmationTarget::Normal) as f64 / 250.0,
            // While users could "cancel" a channel open by RBF-bumping and
            // paying back to themselves, we don't allow it here as its easy to
            // have users accidentally RBF bump and pay to the channel funding
            // address, which results in loss of funds. Real LDK-based
            // applications should enable RBF bumping and RBF bump either to a
            // local change address or to a new channel output negotiated with
            // the same node.
            "replaceable": false,
        });
        self.rpc_client
            .call_method("fundrawtransaction", &[raw_tx_json, options])
            .await
            .context("fundrawtransaction RPC call failed")
    }

    pub async fn send_raw_transaction(
        &self,
        raw_tx: RawTx,
    ) -> anyhow::Result<Txid> {
        let raw_tx_json = serde_json::json!(raw_tx.0);
        self.rpc_client
            .call_method::<Txid>("sendrawtransaction", &[raw_tx_json])
            .await
            .context("sesndrawtransaction RPC call failed")
    }

    pub async fn sign_raw_transaction_with_wallet(
        &self,
        tx_hex: String,
    ) -> anyhow::Result<SignedTx> {
        let tx_hex_json = serde_json::json!(tx_hex);
        self.rpc_client
            .call_method("signrawtransactionwithwallet", &[tx_hex_json])
            .await
            .context("signrawtransactionwithwallet RPC call failed")
    }

    pub async fn get_new_address(&self) -> anyhow::Result<Address> {
        let addr_args = vec![serde_json::json!("LDK output address")];
        let addr = self
            .rpc_client
            .call_method::<NewAddress>("getnewaddress", &addr_args)
            .await
            .context("getnewaddress RPC call failed")?;
        Address::from_str(addr.0.as_str())
            .context("Could not parse address from string")
    }

    pub async fn get_blockchain_info(&self) -> anyhow::Result<BlockchainInfo> {
        self.rpc_client
            .call_method::<BlockchainInfo>("getblockchaininfo", &[])
            .await
            .context("getblockchaininfo RPC call failed")
    }
}

impl BlockSource for LexeBitcoind {
    fn get_header<'a>(
        &'a self,
        header_hash: &'a BlockHash,
        height_hint: Option<u32>,
    ) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
        debug!("get_header() called for {header_hash} ({height_hint:?})");
        Box::pin(async move {
            self.rpc_client.get_header(header_hash, height_hint).await
        })
    }

    fn get_block<'a>(
        &'a self,
        header_hash: &'a BlockHash,
    ) -> AsyncBlockSourceResult<'a, BlockData> {
        debug!("get_block() called for {header_hash}");
        Box::pin(async move { self.rpc_client.get_block(header_hash).await })
    }

    fn get_best_block(
        &self,
    ) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
        debug!("get_best_block() called");
        Box::pin(async move { self.rpc_client.get_best_block().await })
    }
}

impl FeeEstimator for LexeBitcoind {
    fn get_est_sat_per_1000_weight(
        &self,
        confirmation_target: ConfirmationTarget,
    ) -> u32 {
        match confirmation_target {
            ConfirmationTarget::Background => {
                self.background_fees.load(Ordering::Acquire)
            }
            ConfirmationTarget::Normal => {
                self.normal_fees.load(Ordering::Acquire)
            }
            ConfirmationTarget::HighPriority => {
                self.high_prio_fees.load(Ordering::Acquire)
            }
        }
    }
}

impl BroadcasterInterface for LexeBitcoind {
    fn broadcast_transaction(&self, tx: &Transaction) {
        debug!("Broadcasting transaction");
        let rpc_client = self.rpc_client.clone();
        let tx_serialized = serde_json::json!(encode::serialize_hex(tx));
        let _ = LxTask::spawn(async move {
            // This may error due to RL calling `broadcast_transaction` with the
            // same transaction multiple times, but the error is
            // safe to ignore.
            match rpc_client
                .call_method::<Txid>("sendrawtransaction", &[tx_serialized])
                .await
            {
                Ok(_) => {}
                Err(e) => error!("Error broadcasting transaction: {:?}", e),
            }
        });
    }
}
