use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{ensure, Context};
use bitcoin::blockdata::block::Block;
use bitcoin::blockdata::transaction::Transaction;
use bitcoin::consensus::encode;
use bitcoin::hash_types::{BlockHash, Txid};
use bitcoin::util::address::Address;
use common::cli::{BitcoindRpcInfo, Network};
use lightning::chain::chaininterface::{
    BroadcasterInterface, ConfirmationTarget, FeeEstimator,
};
use lightning_block_sync::http::HttpEndpoint;
use lightning_block_sync::rpc::RpcClient;
use lightning_block_sync::{
    AsyncBlockSourceResult, BlockHeaderData, BlockSource,
};
use tokio::runtime::Handle;
use tokio::time;
use tracing::{debug, error};

mod types;

pub use types::*;

const POLL_FEE_ESTIMATE_INTERVAL: Duration = Duration::from_secs(60);

pub struct LexeBitcoind {
    bitcoind_rpc_client: Arc<RpcClient>,
    host: String,
    port: u16,
    rpc_user: String,
    rpc_password: String,
    background_fees: Arc<AtomicU32>,
    normal_fees: Arc<AtomicU32>,
    high_prio_fees: Arc<AtomicU32>,
    handle: Handle,
}

#[derive(Clone, Eq, Hash, PartialEq)]
pub enum Target {
    Background,
    Normal,
    HighPriority,
}

impl BlockSource for &LexeBitcoind {
    fn get_header<'a>(
        &'a self,
        header_hash: &'a BlockHash,
        height_hint: Option<u32>,
    ) -> AsyncBlockSourceResult<'a, BlockHeaderData> {
        Box::pin(async move {
            self.bitcoind_rpc_client
                .get_header(header_hash, height_hint)
                .await
        })
    }

    fn get_block<'a>(
        &'a self,
        header_hash: &'a BlockHash,
    ) -> AsyncBlockSourceResult<'a, Block> {
        Box::pin(async move {
            self.bitcoind_rpc_client.get_block(header_hash).await
        })
    }

    fn get_best_block(
        &self,
    ) -> AsyncBlockSourceResult<(BlockHash, Option<u32>)> {
        Box::pin(async move { self.bitcoind_rpc_client.get_best_block().await })
    }
}

/// The minimum feerate we are allowed to send, as specify by LDK.
const MIN_FEERATE: u32 = 253;

impl LexeBitcoind {
    pub async fn init(
        bitcoind_rpc: BitcoindRpcInfo,
        network: Network,
    ) -> anyhow::Result<Arc<Self>> {
        println!("Initializing bitcoind client");
        let client = LexeBitcoind::new(
            bitcoind_rpc.host,
            bitcoind_rpc.port,
            bitcoind_rpc.username,
            bitcoind_rpc.password,
            Handle::current(),
        )
        .await
        .context("Failed to connect to bitcoind client")?;
        let client = Arc::new(client);

        // Check that the bitcoind we've connected to is running the network we
        // expect
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

        println!("    bitcoind client done.");
        Ok(client)
    }

    // A runtime handle has to be passed in explicitly, otherwise these fns may
    // panic when called from the (non-Tokio) background processor thread
    async fn new(
        host: String,
        port: u16,
        rpc_user: String,
        rpc_password: String,
        handle: Handle,
    ) -> std::io::Result<Self> {
        let http_endpoint =
            HttpEndpoint::for_host(host.clone()).with_port(port);
        let rpc_credentials = base64::encode(format!(
            "{}:{}",
            rpc_user.clone(),
            rpc_password.clone()
        ));
        let bitcoind_rpc_client =
            RpcClient::new(&rpc_credentials, http_endpoint)?;
        let _dummy = bitcoind_rpc_client
            .call_method::<BlockchainInfo>("getblockchaininfo", &[])
            .await
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::PermissionDenied,
                "Failed to make initial call to bitcoind - please check your RPC user/password and access settings")
            })?;

        let background_fees = Arc::new(AtomicU32::new(MIN_FEERATE));
        let normal_fees = Arc::new(AtomicU32::new(2000));
        let high_prio_fees = Arc::new(AtomicU32::new(5000));

        let client = Self {
            bitcoind_rpc_client: Arc::new(bitcoind_rpc_client),
            host,
            port,
            rpc_user,
            rpc_password,
            background_fees,
            normal_fees,
            high_prio_fees,
            handle: handle.clone(),
        };
        client.poll_for_fee_estimates(
            client.background_fees.clone(),
            client.normal_fees.clone(),
            client.high_prio_fees.clone(),
            client.bitcoind_rpc_client.clone(),
        );
        Ok(client)
    }

    fn poll_for_fee_estimates(
        &self,
        background_fees: Arc<AtomicU32>,
        normal_fees: Arc<AtomicU32>,
        high_prio_fees: Arc<AtomicU32>,
        rpc_client: Arc<RpcClient>,
    ) {
        self.handle.spawn(async move {
            let mut poll_interval = time::interval(POLL_FEE_ESTIMATE_INTERVAL);

            loop {
                poll_interval.tick().await;

                let poll_res = Self::poll_for_fee_estimates_fallible(
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
        });
    }

    async fn poll_for_fee_estimates_fallible(
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

    pub fn get_new_rpc_client(&self) -> std::io::Result<RpcClient> {
        let http_endpoint =
            HttpEndpoint::for_host(self.host.clone()).with_port(self.port);
        let rpc_credentials = base64::encode(format!(
            "{}:{}",
            self.rpc_user.clone(),
            self.rpc_password.clone()
        ));
        RpcClient::new(&rpc_credentials, http_endpoint)
    }

    pub async fn create_raw_transaction(
        &self,
        outputs: Vec<HashMap<String, f64>>,
    ) -> anyhow::Result<RawTx> {
        let outputs_json = serde_json::json!(outputs);
        self.bitcoind_rpc_client
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
        self.bitcoind_rpc_client
            .call_method("fundrawtransaction", &[raw_tx_json, options])
            .await
            .context("fundrawtransaction RPC call failed")
    }

    pub async fn send_raw_transaction(
        &self,
        raw_tx: RawTx,
    ) -> anyhow::Result<Txid> {
        let raw_tx_json = serde_json::json!(raw_tx.0);
        self.bitcoind_rpc_client
            .call_method::<Txid>("sendrawtransaction", &[raw_tx_json])
            .await
            .context("sesndrawtransaction RPC call failed")
    }

    pub async fn sign_raw_transaction_with_wallet(
        &self,
        tx_hex: String,
    ) -> anyhow::Result<SignedTx> {
        let tx_hex_json = serde_json::json!(tx_hex);
        self.bitcoind_rpc_client
            .call_method("signrawtransactionwithwallet", &[tx_hex_json])
            .await
            .context("signrawtransactionwithwallet RPC call failed")
    }

    pub async fn get_new_address(&self) -> anyhow::Result<Address> {
        let addr_args = vec![serde_json::json!("LDK output address")];
        let addr = self
            .bitcoind_rpc_client
            .call_method::<NewAddress>("getnewaddress", &addr_args)
            .await
            .context("getnewaddress RPC call failed")?;
        Address::from_str(addr.0.as_str())
            .context("Could not parse address from string")
    }

    pub async fn get_blockchain_info(&self) -> anyhow::Result<BlockchainInfo> {
        self.bitcoind_rpc_client
            .call_method::<BlockchainInfo>("getblockchaininfo", &[])
            .await
            .context("getblockchaininfo RPC call failed")
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
        let bitcoind_rpc_client = self.bitcoind_rpc_client.clone();
        let tx_serialized = serde_json::json!(encode::serialize_hex(tx));
        self.handle.spawn(async move {
            // This may error due to RL calling `broadcast_transaction` with the
            // same transaction multiple times, but the error is
            // safe to ignore.
            match bitcoind_rpc_client
                .call_method::<Txid>("sendrawtransaction", &[tx_serialized])
                .await
            {
                Ok(_) => {}
                Err(e) => error!("Error broadcasting transaction: {:?}", e),
            }
        });
    }
}
