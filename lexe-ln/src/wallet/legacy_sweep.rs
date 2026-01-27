//! Legacy BDK wallet sweep functionality.
//!
//! This module handles sweeping funds from legacy BDK wallets (created before
//! node-v0.8.12, using non-BIP39-compatible derivation) to the new
//! BIP39-compatible wallet.
//!
//! ## Dust handling
//!
//! If the legacy wallet balance is too small to cover transaction fees, BDK's
//! `drain_wallet()` will fail to build a transaction. In this case, the sweep
//! fails and we log an error. The dust will remain in the legacy wallet until
//! fees drop or more funds are deposited. This is acceptable since dust amounts
//! are economically insignificant.

use std::sync::Arc;

use anyhow::Context;
use bitcoin::{Amount, Transaction, bip32::Xpriv};
use common::ln::{network::LxNetwork, priority::ConfirmationPriority};
use lexe_tokio::{notify, task::LxTask};
use tracing::{debug, error, instrument};

use crate::{
    esplora::{FeeEstimates, LexeEsplora},
    persister::LexePersisterMethods,
    traits::LexePersister,
    tx_broadcaster::TxBroadcaster,
    wallet::{LexeCoinSelector, OnchainWallet},
};

/// The legacy wallet changeset filename (pre-v0.8.12).
const WALLET_CHANGESET_LEGACY_FILENAME: &str = "bdk_wallet_changeset";

/// Context required for legacy wallet sweep.
pub struct LegacySweepCtx<PS: LexePersister> {
    /// The legacy (pre-BIP39-compatible) master extended private key.
    pub legacy_master_xprv: Xpriv,
    pub network: LxNetwork,
    pub esplora: Arc<LexeEsplora>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub coin_selector: LexeCoinSelector,
    pub tx_broadcaster: TxBroadcaster,
    pub persister: PS,
    /// The new BIP39-compatible wallet to sweep funds into.
    pub new_wallet: OnchainWallet,
}

/// Spawn the legacy wallet sweep task.
///
/// This task runs in the background and does not block node startup.
/// On failure, it logs an error and the sweep will be retried on next startup.
pub fn spawn_legacy_sweep_task<PS: LexePersister>(
    ctx: LegacySweepCtx<PS>,
) -> LxTask<()> {
    LxTask::spawn("legacy-wallet-sweep", do_legacy_sweep(ctx))
}

#[instrument(skip_all, name = "(legacy-sweep)")]
async fn do_legacy_sweep<PS: LexePersister>(ctx: LegacySweepCtx<PS>) {
    // 1. Read the legacy wallet changeset from the persister.
    let maybe_legacy_changeset_res = ctx
        .persister
        .read_wallet_changeset_legacy()
        .await
        .context("Failed to read legacy changeset");
    let maybe_legacy_changeset = match maybe_legacy_changeset_res {
        Ok(changeset) => changeset,
        Err(err) => {
            error!("{err:#}");
            return;
        }
    };

    // 2. Initialize the legacy wallet. If no changeset exists, init will
    //    full_sync to discover any funds.
    let (persist_tx, mut persist_rx) = notify::channel();
    let legacy_wallet_res = OnchainWallet::init(
        ctx.legacy_master_xprv,
        ctx.network,
        &ctx.esplora,
        ctx.fee_estimates.clone(),
        ctx.coin_selector,
        maybe_legacy_changeset,
        persist_tx,
    )
    .await
    .context("Failed to init legacy wallet");
    let legacy_wallet = match legacy_wallet_res {
        Ok(wallet) => wallet,
        Err(err) => {
            error!("{err:#}");
            return;
        }
    };

    // Sync and attempt to sweep the legacy wallet
    if let Err(err) = sync_and_sweep(&ctx, &legacy_wallet).await {
        // Log error but don't panic - sweep will retry on next startup
        error!("Legacy wallet sweep failed: {err:#}");
    }

    // See if we need to persist the legacy wallet changeset
    if !persist_rx.try_recv() {
        return;
    }

    // Persist the legacy wallet changeset
    super::do_wallet_persist(
        &ctx.persister,
        &legacy_wallet,
        WALLET_CHANGESET_LEGACY_FILENAME,
    )
    .await;
}

/// Attempts to sync and then sweep the legacy wallet.
async fn sync_and_sweep<PS: LexePersister>(
    ctx: &LegacySweepCtx<PS>,
    legacy_wallet: &OnchainWallet,
) -> anyhow::Result<()> {
    // 3. Sync the legacy wallet to get latest state. (init already does
    //    full_sync if changeset was None, but we do an incremental sync here in
    //    case there's a previously-persisted changeset)
    legacy_wallet
        .sync(&ctx.esplora)
        .await
        .context("Failed to sync legacy wallet")?;

    // 4. Check if there's any balance to sweep
    let balance = legacy_wallet.get_balance();
    let total =
        balance.confirmed + balance.trusted_pending + balance.untrusted_pending;

    if total == Amount::ZERO {
        debug!("Legacy wallet has no balance to sweep");
        return Ok(());
    } else {
        debug!("Legacy wallet has funds to sweep");
    }

    // 5. Get destination address from new wallet (internal/change address)
    let dest_address = ctx.new_wallet.get_internal_address();
    let dest_script = dest_address.script_pubkey();

    // 6. Create sweep transaction (drain all funds to new wallet)
    let sweep_tx =
        create_sweep_tx(legacy_wallet, dest_script, &ctx.fee_estimates)
            .context("Failed to create sweep tx")?;

    // 7. Broadcast the sweep transaction
    ctx.tx_broadcaster
        .broadcast_transaction(sweep_tx.clone())
        .await
        .context("Failed to broadcast sweep tx")?;

    // 8. Register the broadcasted tx in legacy wallet so it can track it
    legacy_wallet.transaction_broadcasted(sweep_tx);
    Ok(())
}

/// Create a sweep transaction that drains all UTXOs to the destination.
/// Returns the signed transaction and its txid.
fn create_sweep_tx(
    legacy_wallet: &OnchainWallet,
    dest_script: bitcoin::ScriptBuf,
    fee_estimates: &FeeEstimates,
) -> anyhow::Result<Transaction> {
    let feerate =
        fee_estimates.conf_prio_to_feerate(ConfirmationPriority::Background);

    let mut locked_wallet = legacy_wallet.write();

    // Build drain transaction using BDK's drain_wallet + drain_to.
    // This will fail if the balance is dust (can't cover fees).
    let mut tx_builder = locked_wallet.build_tx();
    tx_builder
        .drain_wallet()
        .drain_to(dest_script)
        .fee_rate(feerate);

    let mut psbt = tx_builder.finish().context("Failed to build sweep tx")?;

    // Sign the transaction
    let sign_opts = bdk_wallet::SignOptions::default();
    let finalized = locked_wallet
        .sign(&mut psbt, sign_opts)
        .context("Failed to sign sweep tx")?;
    anyhow::ensure!(finalized, "Sweep tx signing did not finalize all inputs");

    let tx = psbt
        .extract_tx()
        .context("Failed to extract signed sweep tx")?;
    Ok(tx)
}
