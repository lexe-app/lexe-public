//! Legacy BDK wallet sweep functionality.
//!
//! This module handles sweeping funds from legacy BDK wallets (created <=
//! node-v0.9.1, using non-BIP39-compatible derivation) to the new
//! BIP39-compatible wallet.
//!
//! ## Dust handling
//!
//! If the legacy wallet balance is too small to cover transaction fees, BDK's
//! `drain_wallet()` will fail to build a transaction. In this case, the sweep
//! fails and we log an error. The dust will remain in the legacy wallet until
//! fees drop or more funds are deposited. This is acceptable since dust amounts
//! are economically insignificant.

use std::{sync::Arc, time::Instant};

use anyhow::Context;
use bitcoin::bip32::Xpriv;
use common::{
    ln::{amount::Amount, network::LxNetwork, priority::ConfirmationPriority},
    rng::SysRng,
};
use lexe_api::{
    models::command::PayOnchainRequest,
    types::payments::{ClientPaymentId, PaymentKind},
};
use lexe_tokio::{notify, task::LxTask};
use tracing::{debug, error, instrument};

use crate::{
    esplora::{FeeEstimates, LexeEsplora},
    payments::{manager::PaymentsManager, onchain::OnchainSendV2},
    persister::LexePersisterMethods,
    traits::{LexeChannelManager, LexePersister},
    tx_broadcaster::TxBroadcaster,
    wallet::{LexeCoinSelector, OnchainWallet},
};

/// The legacy wallet changeset filename (for wallets created <= node-v0.9.1).
const WALLET_CHANGESET_LEGACY_FILENAME: &str = "bdk_wallet_changeset";

/// Context required for legacy wallet sweep.
pub struct LegacySweepCtx<CM: LexeChannelManager<PS>, PS: LexePersister> {
    /// The legacy (pre-BIP39-compatible) master extended private key.
    pub legacy_master_xprv: Xpriv,
    pub network: LxNetwork,
    pub esplora: Arc<LexeEsplora>,
    pub fee_estimates: Arc<FeeEstimates>,
    pub coin_selector: LexeCoinSelector,
    pub tx_broadcaster: TxBroadcaster,
    pub payments_manager: PaymentsManager<CM, PS>,
    pub persister: PS,
    /// The new BIP39-compatible wallet to sweep funds into.
    pub new_wallet: OnchainWallet,
}

/// Spawn the legacy wallet sweep task.
///
/// This task runs in the background and does not block node startup.
/// On failure, it logs an error and the sweep will be retried on next startup.
pub fn spawn_legacy_sweep_task<
    CM: LexeChannelManager<PS>,
    PS: LexePersister,
>(
    ctx: LegacySweepCtx<CM, PS>,
) -> LxTask<()> {
    LxTask::spawn("legacy-wallet-sweep", do_legacy_sweep(ctx))
}

#[instrument(skip_all, name = "(legacy-sweep)")]
async fn do_legacy_sweep<CM: LexeChannelManager<PS>, PS: LexePersister>(
    ctx: LegacySweepCtx<CM, PS>,
) {
    // Read the legacy wallet changeset from the persister.
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

    // Initialize the legacy wallet. If no changeset exists, init will
    // full_sync to discover any funds.
    let (wallet_persister_tx, mut wallet_persister_rx) = notify::channel();
    let legacy_wallet_res = OnchainWallet::init(
        ctx.legacy_master_xprv,
        ctx.network,
        &ctx.esplora,
        ctx.fee_estimates.clone(),
        ctx.coin_selector,
        maybe_legacy_changeset,
        wallet_persister_tx,
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
    if !wallet_persister_rx.try_recv() {
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
async fn sync_and_sweep<CM: LexeChannelManager<PS>, PS: LexePersister>(
    ctx: &LegacySweepCtx<CM, PS>,
    legacy_wallet: &OnchainWallet,
) -> anyhow::Result<()> {
    // Sync the legacy wallet to get latest state. (init already does
    // full_sync if changeset was None, but we do an incremental sync here in
    // case there's a previously-persisted changeset)
    let start = Instant::now();
    let sync_stats = legacy_wallet
        .sync(&ctx.esplora)
        .await
        .context("Failed to sync")?;

    let is_legacy = true;
    let elapsed_ms = start.elapsed().as_millis();
    sync_stats.log_sync_complete(is_legacy, elapsed_ms);

    // Check if there's any balance to sweep
    let balance = legacy_wallet.get_balance();
    let total = Amount::try_from(
        balance.confirmed + balance.trusted_pending + balance.untrusted_pending,
    )
    .context("Bad legacy wallet balance")?;

    if total == Amount::ZERO {
        debug!("Legacy wallet has no balance to sweep");
        return Ok(());
    }

    // Get destination address from new wallet (internal/change address)
    let dest_address = ctx.new_wallet.get_internal_address();

    // Create sweep onchain send (draining all funds to new wallet)
    let priority = ConfirmationPriority::Background;
    let (tx, fee) = legacy_wallet
        .create_sweep_tx(&dest_address, priority)
        .context("Failed to create tx")?;
    let req = PayOnchainRequest {
        cid: ClientPaymentId::from_rng(&mut SysRng::new()),
        address: dest_address.into_unchecked(),
        // This is a little funny, but I think this makes the most sense since
        // we're effectively paying ourselves. We'll only see this entry in our
        // payments list, since we're paying to an internal address in the new
        // wallet, so no OnchainReceive payment will be produced.
        amount: Amount::ZERO,
        priority,
        note: Some("Sweep to BIP39-compatible on-chain wallet".to_owned()),
    };
    let oswm = OnchainSendV2::new(tx, req, PaymentKind::Onchain, fee)
        .context("Failed to create onchain send")?;

    let tx = oswm.payment.tx.clone();
    let id = oswm.payment.id();
    let txid = oswm.payment.txid;
    let pwm = oswm.into_enum();

    // Broadcast the sweep transaction
    ctx.tx_broadcaster
        .broadcast_transaction(tx.as_ref().clone())
        .await
        .context("Failed to broadcast")?;

    // Register the broadcasted tx in legacy wallet so it can track it
    legacy_wallet.transaction_broadcasted(tx.as_ref().clone());

    // NOTE(phlip9): this is a little out-of-order from normal, but I'd rather
    // we only register if the broadcast was successful.

    // Register the tx w/ the payments manager
    let _created_index = ctx
        .payments_manager
        .new_payment(pwm)
        .await
        .context("Failed to register new onchain send")?;

    // Register the successful broadcast
    ctx.payments_manager
        .onchain_send_broadcasted(&id, &txid)
        .await
        .context("Could not register broadcast")?;

    Ok(())
}
