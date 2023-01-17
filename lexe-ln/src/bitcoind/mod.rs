use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{ensure, Context};
use bitcoin::hash_types::Txid;
use common::cli::{BitcoindRpcInfo, Network};
use lightning::chain::chaininterface::{ConfirmationTarget, FeeEstimator};
use lightning_block_sync::http::HttpEndpoint;
use lightning_block_sync::rpc::RpcClient;
use tracing::debug;

use crate::esplora::LexeEsplora;

mod types;

pub use types::*;

pub struct LexeBitcoind {
    rpc_client: Arc<RpcClient>,
}

impl LexeBitcoind {
    pub async fn init(
        bitcoind_rpc: BitcoindRpcInfo,
        network: Network,
    ) -> anyhow::Result<Self> {
        debug!(%network, "Initializing bitcoind client");

        let credentials = bitcoind_rpc.base64_credentials();
        let http_endpoint = HttpEndpoint::for_host(bitcoind_rpc.host)
            .with_port(bitcoind_rpc.port);
        let rpc_client = RpcClient::new(&credentials, http_endpoint)
            .context("Could not initialize RPC client")?;
        let rpc_client = Arc::new(rpc_client);

        let client = Self { rpc_client };

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
        esplora: &LexeEsplora,
    ) -> anyhow::Result<FundedTx> {
        let raw_tx_json = serde_json::json!(raw_tx.0);
        let fee_rate = esplora
            .get_est_sat_per_1000_weight(ConfirmationTarget::Normal)
            as f64
            / 250.0;
        let options = serde_json::json!({
            // LDK gives us feerates in satoshis per KW but Bitcoin Core here
            // expects fees denominated in satoshis per vB. First we need to
            // multiply by 4 to convert weight units to virtual bytes, then
            // divide by 1000 to convert KvB to vB.
            "fee_rate": fee_rate,
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

    pub async fn get_blockchain_info(&self) -> anyhow::Result<BlockchainInfo> {
        self.rpc_client
            .call_method::<BlockchainInfo>("getblockchaininfo", &[])
            .await
            .context("getblockchaininfo RPC call failed")
    }
}
