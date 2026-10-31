//! # A note on [`ChangeSet`]s and wallet persistence
//!
//! [`bdk_wallet::ChangeSet`] is the top-level data struct given to us by BDK,
//! and is the main thing that need to be persisted. It implements [`Serialize`]
//! / [`Deserialize`], and [`bdk_chain::Merge`], which allows changesets to be
//! merged together. The [`ChangeSet`]s may be persisted in aggregated form, or
//! they can be persisted separately and reaggregated when (re-)initializing our
//! [`Wallet`].
//!
//! Our [`OnchainWallet`] uses a write-back model. Changes are:
//!
//! 1) Staged inside [`bdk_wallet::Wallet`],
//! 2) Merged into a total [`ChangeSet`] cached in the [`OnchainWallet`], then
//! 3) The total [`ChangeSet`] is (re-)persisted to the DB by the wallet
//!    persister task whenever it receives a notification.
//!
//! NOTE: This persistence model means we need to manually call
//! [`OnchainWallet::trigger_persist`] anytime we mutate the BDK [`Wallet`].
//!
//! NOTE: It is possible that we'll lose some data if the node crashes before
//! any staged changes are persisted, but this should be OK because all data in
//! the [`ChangeSet`] can be re-derived with a full sync. The exception is a
//! single-use reveal, which a crash would re-issue, so
//! [`OnchainWallet::get_exclusive_address`] awaits the persist first.
//!
//! NOTE: a key design requirement is that we should be able to recover all BDK
//! on-chain wallet funds from just the [`RootSeed`]. In other words, if we blow
//! away the persisted [`ChangeSet`]s for all on-chain BDK wallets, we can still
//! recover all funds by full-syncing.
//!
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize
//! [`ChangeSet`]: bdk_wallet::ChangeSet
//! [`Wallet`]: bdk_wallet::Wallet
//! [`OnchainWallet`]: crate::wallet::OnchainWallet
//! [`OnchainWallet::trigger_persist`]: crate::wallet::OnchainWallet::trigger_persist
//! [`OnchainWallet::get_exclusive_address`]: crate::wallet::OnchainWallet::get_exclusive_address
//! [`RootSeed`]: lexe_common::root_seed::RootSeed

use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{Context, anyhow, ensure};
use bdk_chain::{
    CanonicalizationParams, Merge, TxUpdate,
    spk_client::{
        FullScanRequest, FullScanResponse, SyncRequest, SyncResponse,
    },
};
use bdk_esplora::EsploraAsyncExt;
pub use bdk_wallet::ChangeSet;
use bdk_wallet::{
    CreateParams, KeychainKind, LoadParams, SignOptions, TxBuilder, TxDetails,
    Wallet, WeightedUtxo,
    coin_selection::{
        CoinSelectionAlgorithm, CoinSelectionResult, InsufficientFunds,
    },
    descriptor::IntoWalletDescriptor,
    miniscript,
    template::Bip84,
};
use bitcoin::{
    Psbt, Transaction,
    bip32::{ChildNumber, Xpub},
    hashes::Hash as _,
};
use lexe_api::{
    models::command::{
        FeeEstimate, OnchainDescriptors, PayOnchainPreflightRequest,
        PayOnchainPreflightResponse, PayOnchainRequest,
    },
    types::{payments::PaymentKind, retries::Retries},
    vfs::{
        SINGLETON_DIRECTORY, Vfs, VfsFileId, WALLET_CHANGESET_LEGACY_FILENAME,
        WALLET_CHANGESET_V2_FILENAME,
    },
};
#[cfg(test)]
use lexe_common::ln::channel::OutPoint;
use lexe_common::{
    ln::{
        amount::Amount, balance::OnchainBalance, hashes::Txid,
        network::Network, priority::ConfirmationPriority,
    },
    secp256k1_ctx::SECP256K1,
    time::TimestampMs,
};
use lexe_crypto::{ed25519, rng::RngCore};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::util::wallet_utils;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, instrument, warn};

use crate::{
    esplora::{FeeEstimates, LexeEsplora},
    payments::{PaymentWithMetadata, onchain::OnchainSendV2},
    traits::LexePersister,
};

/// Sweeping legacy on-chain wallets to new BIP39-compatible wallets
pub mod legacy_sweep;

/// Lexe's on-chain wallet, wrapping BDK's [`Wallet`]. Comes in two forms:
///
/// - [`OnchainWallet::init`]: the live wallet, with External (receive),
///   Internal (change), and SingleUse keychains.
/// - [`OnchainWallet::init_legacy`]: the pre-node-v0.9.1 wallet swept by
///   [`legacy_sweep`], with only the External and Internal keychains and its
///   own changeset file.
#[derive(Clone)]
pub struct OnchainWallet {
    inner: Arc<std::sync::RwLock<Wallet>>,
    /// NOTE: This is the full, *aggregated* changeset, not an intermediate
    /// state diff, contrary to what the name of "[`ChangeSet`]" might suggest.
    changeset: Arc<std::sync::Mutex<ChangeSet>>,
    /// The VFS filename the aggregated changeset is persisted to.
    ///
    /// This field only exists so [`OnchainWallet`] can be initialized as a
    /// legacy wallet for the [`legacy_sweep`] task.
    changeset_filename: &'static str,
    scan_params: AddressScanParams,
    fee_estimates: Arc<FeeEstimates>,
    coin_selector: LexeCoinSelector,
    wallet_persister_tx: mpsc::Sender<WalletPersistRequest>,
}

/// A request to the wallet persister task. `Some(ack_tx)` asks for the
/// persist result back once the persist completes.
pub type WalletPersistRequest = Option<oneshot::Sender<anyhow::Result<()>>>;

/// How many script pubkeys BDK derives and scans beyond those known to be used.
#[derive(Copy, Clone)]
pub struct AddressScanParams {
    /// "`stop_gap` is the maximum number of consecutive unused addresses. For
    /// example, with a `stop_gap` of  3, `full_scan` will keep scanning until
    /// it encounters 3 consecutive script pubkeys with no associated
    /// transactions."
    ///
    /// From: [`EsploraAsyncExt::full_scan`]
    pub full_scan_stop_gap: usize,
    /// "The lookahead defines a number of script pubkeys to derive over and
    /// above the last revealed index."
    pub lookahead: u32,
}

impl Default for AddressScanParams {
    fn default() -> Self {
        Self {
            // Single-use addresses may be paid in any order, or never, so a
            // restore from seed must tolerate runs of unpaid addresses.
            full_scan_stop_gap: 50,
            // We only reveal unused addresses. However, with write-back
            // persistence, a crash before the update persists can make us
            // forget that we revealed an index. Thus, a lookahead of 1
            // should be sufficient.
            lookahead: 1,
        }
    }
}

/// Counts the total, confirmed, and unconfirmed UTXOs tracked by BDK.
#[derive(Copy, Clone, Debug, Default)]
pub struct UtxoCounts {
    pub total: usize,
    pub confirmed: usize,
    pub unconfirmed: usize,
}

/// Various stats collected about an on-chain BDK wallet sync
#[derive(Default)]
pub struct SyncStats {
    /// Whether this sync was a full sync or an incremental sync
    pub is_full_sync: bool,
    /// The total number of (revealed) external spks
    pub total_external: u32,
    /// The number of external spks we want to sync (incremental only)
    pub syncing_external: u32,
    /// The total number of (revealed) internal spks
    pub total_internal: u32,
    /// The number of internal spks we want to sync (incremental only)
    pub syncing_internal: u32,
    /// The total number of (revealed) single-use spks
    pub total_single_use: u32,
    /// The number of single-use spks we want to sync (incremental only)
    pub syncing_single_use: u32,
    /// The number of transactions synced
    pub txs: u32,
    /// The number of transaction outputs synced
    pub txouts: u32,
    /// The number of chain anchors (relevant block headers) synced
    pub anchors: u32,
    /// The number of relevant transactions seen in the mempool
    pub seen: u32,
    /// The number of relevant transactions discovered evicted from the mempool
    pub evicted: u32,
}

// --- impl OnchainWallet --- //

impl OnchainWallet {
    /// Number of parallel requests BDK is permitted to use.
    const BDK_CONCURRENCY: usize = 24;

    /// The [`ConfirmationPriority`] for new open_channel funding transactions.
    ///
    /// See: [`OnchainWallet::create_and_sign_funding_tx`]
    ///  and [`OnchainWallet::preflight_channel_funding_tx`].
    const CHANNEL_FUNDING_CONF_PRIO: ConfirmationPriority =
        ConfirmationPriority::Normal;

    /// The number of confirmations required to consider a transaction
    /// finalized. This determines when we'll stop syncing an internal spk
    /// (script pubkey) because it has no more pending spends.
    const CONFS_TO_FINALIZE: u32 = 6;

    /// Size of the wallet persister channel. When full,
    /// [`Self::trigger_persist`] drops its request, which the queued requests
    /// already cover, while [`Self::persist_and_wait`] waits for a free slot.
    const PERSISTER_CHANNEL_SIZE: usize = 16;

    /// Init a [`OnchainWallet`] from a master extended private key and
    /// [`ChangeSet`]. Wallet addresses are generated according to the
    /// [BIP 84] standard. See also [BIP 44].
    ///
    /// The master xprv is normally from [`RootSeed::derive_bip32_master_xprv`].
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    /// [`RootSeed::derive_bip32_master_xprv`]: lexe_common::root_seed::RootSeed::derive_bip32_master_xprv
    pub async fn init(
        master_xprv: bitcoin::bip32::Xpriv,
        network: Network,
        esplora: &LexeEsplora,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        maybe_changeset: Option<ChangeSet>,
        scan_params: AddressScanParams,
    ) -> anyhow::Result<(Self, mpsc::Receiver<WalletPersistRequest>)> {
        let is_legacy = false;
        Self::init_inner(
            master_xprv,
            network,
            esplora,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            scan_params,
            is_legacy,
        )
        .await
    }

    /// Init the legacy (<= node-v0.9.1) on-chain wallet for [`legacy_sweep`].
    /// Persists to the legacy changeset file. Has no single-use keychain and
    /// no persister task: [`legacy_sweep`] persists it directly.
    pub async fn init_legacy(
        legacy_master_xprv: bitcoin::bip32::Xpriv,
        network: Network,
        esplora: &LexeEsplora,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        maybe_changeset: Option<ChangeSet>,
        scan_params: AddressScanParams,
    ) -> anyhow::Result<Self> {
        let is_legacy = true;
        let (wallet, _wallet_persister_rx) = Self::init_inner(
            legacy_master_xprv,
            network,
            esplora,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            scan_params,
            is_legacy,
        )
        .await?;
        Ok(wallet)
    }

    #[instrument(skip_all, name = "(wallet-init)")]
    async fn init_inner(
        master_xprv: bitcoin::bip32::Xpriv,
        network: Network,
        esplora: &LexeEsplora,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        maybe_changeset: Option<ChangeSet>,
        scan_params: AddressScanParams,
        is_legacy: bool,
    ) -> anyhow::Result<(Self, mpsc::Receiver<WalletPersistRequest>)> {
        let (wallet_persister_tx, wallet_persister_rx) =
            mpsc::channel(Self::PERSISTER_CHANNEL_SIZE);
        let (lexe_wallet, wallet_created) = Self::new(
            master_xprv,
            network,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            scan_params,
            wallet_persister_tx,
            is_legacy,
        )?;

        if wallet_created {
            lexe_wallet
                .full_sync(esplora)
                .await
                .context("Failed to conduct initial full sync")?;
        }

        Ok((lexe_wallet, wallet_persister_rx))
    }

    fn new(
        master_xprv: bitcoin::bip32::Xpriv,
        network: Network,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        mut maybe_changeset: Option<ChangeSet>,
        scan_params: AddressScanParams,
        wallet_persister_tx: mpsc::Sender<WalletPersistRequest>,
        is_legacy: bool,
    ) -> anyhow::Result<(Self, bool)> {
        let network = network.to_bitcoin();

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let internal = Bip84(master_xprv, KeychainKind::Internal);
        // Descriptor for single-use addresses: `m/84h/{0,1}h/0h/2/*`
        let maybe_single_use =
            (!is_legacy).then_some(Bip84(master_xprv, KeychainKind::SingleUse));

        // compat: Changesets persisted before node-v0.10.9 lack the single-use
        // descriptor, which `Wallet::load_with_params` would reject as a
        // mismatch, so fill it in. BDK treats an empty changeset as "create a
        // new wallet", so leave those alone.
        if let (Some(single_use), Some(changeset)) =
            (&maybe_single_use, &mut maybe_changeset)
            && !changeset.is_empty()
            && changeset.single_use_descriptor.is_none()
        {
            let (descriptor, _keymap) = single_use
                .clone()
                .into_wallet_descriptor(&SECP256K1, network)
                .context("Failed to derive single-use descriptor")?;
            changeset.single_use_descriptor = Some(descriptor);
        }

        // Creates a new wallet.
        let mut wallet_created = false;
        let mut create_wallet = || {
            let mut create_params =
                CreateParams::new(external.clone(), internal.clone())
                    .network(network)
                    // TODO(max): Wire through specific testnet3/testnet4 hash
                    // .genesis_hash(genesis_hash)
                    .lookahead(scan_params.lookahead);
            if let Some(single_use) = maybe_single_use.clone() {
                create_params = create_params.single_use_descriptor(single_use);
            }
            // NOTE: This call stages a non-empty `ChangeSet`.
            let wallet = Wallet::create_with_params(create_params)
                .context("Wallet::create_with_params failed")?;
            wallet_created = true;
            Ok::<Wallet, anyhow::Error>(wallet)
        };

        // Loads a wallet from an existing changeset, or creates it if it the
        // changeset was empty (which generally shouldn't be the case)
        let mut load_or_create_wallet = |changeset: ChangeSet| {
            let load_params = LoadParams::new()
                // NOTE: If we ever change our descriptors, we'll need to
                // remove these checks for compatibility.
                .descriptor(KeychainKind::External, Some(external.clone()))
                .descriptor(KeychainKind::Internal, Some(internal.clone()))
                .descriptor(KeychainKind::SingleUse, maybe_single_use.clone())
                // Extract private keys from these descriptors so we can
                // actually sign txs.
                .extract_keys()
                // TODO(max): Might want to check testnet3/testnet4 hash
                // .check_genesis_hash(genesis_hash)
                .check_network(network)
                .lookahead(scan_params.lookahead);
            let maybe_wallet = Wallet::load_with_params(changeset, load_params)
                .context("Wallet::load_with_params failed")?;
            if maybe_wallet.is_none() {
                warn!(
                    "Wallet::load_with_params returned no wallet. \
                     Did we somehow persist an empty ChangeSet?"
                );
            }
            let wallet = match maybe_wallet {
                Some(mut w) => {
                    // We're loading an existing wallet.
                    //
                    // Since we persist the bdk wallet out-of-line, we can
                    // potentially reveal+use an spk and then crash before the
                    // BDK wallet persists. Without any mitigation, we would
                    // restart and fail to sync the spk since it appears
                    // unrevealed.
                    //
                    // To mitigate this, we'll reveal an spk per keychain if
                    // there are no unused spks in that keychain. Since our
                    // incremental sync always syncs any unused spks, if we
                    // always have one unused spk after init we'll be able to
                    // catch one used-before-crash spk.
                    //
                    // Technically we can also reveal and use multiple spks
                    // before crashing, but we're only going to try to handle
                    // one spk used-before-crash per keychain for now.
                    //
                    // NOTE: Not SingleUse: its reveals persist before the
                    // address is handed out, so a crash can't lose one, and
                    // an extra reveal would burn an address.
                    w.reveal_spk_if_no_unused(KeychainKind::External);
                    w.reveal_spk_if_no_unused(KeychainKind::Internal);
                    w
                }
                None => create_wallet()?,
            };

            Ok::<Wallet, anyhow::Error>(wallet)
        };

        let mut wallet = match maybe_changeset.clone() {
            Some(changeset) => load_or_create_wallet(changeset)?,
            None => create_wallet()?,
        };

        let initial_changeset = maybe_changeset
            .or_else(|| wallet.take_staged())
            .unwrap_or_default();

        // Sanity check: BDK wallet should pick up our signers.
        let has_signers =
            |keychain| !wallet.get_signers(keychain).ids().is_empty();
        assert!(
            has_signers(KeychainKind::External)
                && has_signers(KeychainKind::Internal),
            "BDK wallet must have at least one External and one Internal signer"
        );
        assert!(
            maybe_single_use.is_none() || has_signers(KeychainKind::SingleUse),
            "BDK wallet must have at least one SingleUse signer"
        );

        let changeset_filename = if is_legacy {
            WALLET_CHANGESET_LEGACY_FILENAME
        } else {
            WALLET_CHANGESET_V2_FILENAME
        };
        Ok((
            Self {
                inner: Arc::new(std::sync::RwLock::new(wallet)),
                changeset: Arc::new(std::sync::Mutex::new(initial_changeset)),
                changeset_filename,
                scan_params,
                fee_estimates,
                coin_selector,
                wallet_persister_tx,
            },
            wallet_created,
        ))
    }

    /// Constructs a dummy [`OnchainWallet`] useful for tests. Also returns the
    /// receiving end of the acked persist channel, whose requests must be
    /// acked for single-use reveals to complete.
    #[cfg(test)]
    pub(crate) fn dummy(
        master_xprv: bitcoin::bip32::Xpriv,
        maybe_changeset: Option<ChangeSet>,
    ) -> (Self, mpsc::Receiver<WalletPersistRequest>) {
        let fee_estimates = FeeEstimates::dummy();
        let coin_selector = LexeCoinSelector::default();
        let network = Network::Regtest;
        let (wallet_persister_tx, wallet_persister_rx) =
            mpsc::channel(Self::PERSISTER_CHANNEL_SIZE);
        let is_legacy = false;
        let (wallet, _wallet_created) = OnchainWallet::new(
            master_xprv,
            network,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            AddressScanParams::default(),
            wallet_persister_tx,
            is_legacy,
        )
        .unwrap();

        (wallet, wallet_persister_rx)
    }

    /// Returns a read lock on the inner [`Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    pub fn read(&self) -> RwLockReadGuard<'_, Wallet> {
        self.inner.read().unwrap()
    }

    /// Returns a write lock on the inner [`Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    /// NOTE: You should call [`OnchainWallet::trigger_persist`] after you are
    /// done writing to ensure that any changes you make are persisted.
    pub fn write(&self) -> RwLockWriteGuard<'_, Wallet> {
        self.inner.write().unwrap()
    }

    /// Notifies the wallet persister task to persist any changes to the wallet.
    pub fn trigger_persist(&self) {
        let _ = self.wallet_persister_tx.try_send(None);
    }

    /// Persists any changes to the wallet via the wallet persister task,
    /// returning once they are on disk.
    pub async fn persist_and_wait(&self) -> anyhow::Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.wallet_persister_tx
            .send(Some(ack_tx))
            .await
            .map_err(|_| anyhow!("Wallet persister task disconnected"))?;
        ack_rx
            .await
            .context("Wallet persister task dropped our ack")?
    }

    /// Merges any staged changes into the aggregated [`ChangeSet`] and persists
    /// it. Skips the persist if nothing was staged, unless `force`d.
    async fn persist<PS: LexePersister>(
        &self,
        persister: &PS,
        force: bool,
    ) -> anyhow::Result<()> {
        match self.write().take_staged() {
            Some(new_changes) =>
                self.changeset.lock().unwrap().merge(new_changes),
            None if !force => {
                debug!("Skipping persist: no new changes");
                return Ok(());
            }
            None => (),
        }

        let file = {
            let locked_changeset = self.changeset.lock().unwrap();
            let file_id =
                VfsFileId::new(SINGLETON_DIRECTORY, self.changeset_filename);
            persister.encrypt_json(file_id, &*locked_changeset)
        };
        let result = persister
            .persist_file(file, Retries::IMPORTANT_PERSISTS)
            .await
            .context("Failed to persist wallet changeset");
        match &result {
            Ok(()) => debug!("Success: persisted wallet db"),
            Err(e) => warn!("Wallet DB persist error: {e:#}"),
        }
        result
    }

    /// Syncs the [`Wallet`] using a remote Esplora backend.
    #[instrument(skip_all, name = "(bdk-sync)")]
    pub async fn sync(
        &self,
        esplora: &LexeEsplora,
    ) -> anyhow::Result<SyncStats> {
        // Build a SyncRequest with everything we're interested in syncing.
        let now = TimestampMs::now();
        let (sync_request, mut sync_stats) = self.build_sync_request_at(now);

        // Check for updates on everything we specified in the SyncRequest.
        let sync_result = esplora
            .client()
            .sync(sync_request, Self::BDK_CONCURRENCY)
            .await
            .context("`EsploraAsyncExt::sync` failed")?;
        sync_stats.with_sync_response(&sync_result);

        // TODO(phlip9): if we observe the +1 lookahead spk in the sync
        // results, fallback to a full sync or extra lookahead sync, since our
        // BDK wallet appears to have failed / missed an earlier persist.

        // Apply the update to the wallet.
        {
            let mut locked_wallet = self.write();
            locked_wallet
                .apply_update(sync_result)
                .context("Could not apply sync update to wallet")?;
        }
        self.trigger_persist();

        Ok(sync_stats)
    }

    /// Build an incremental sync request to efficiently sync our local BDK
    /// wallet state with a remote esplora REST API blockchain data source.
    ///
    /// The sync request is a collection of spks (script pubkeys) we want to
    /// query for relevant chain+mempool transaction updates.
    ///
    /// This incremental sync tries to be efficient in that our sync request
    /// fetches `O(revealed external spks + pending internal spks)` spk
    /// histories. This is in contrast to the default BDK spk sync, which
    /// fetches `O(revealed external spks + revealed internal spks)` spk
    /// histories. For our LSP, we have a large (# users), ever-growing set
    /// of revealed internal spks, vs a relatively small (# UTXOs) set of
    /// pending internal spks.
    ///
    /// The main idea is that in the happy path, spks from our
    /// `Internal` keychain only ever recv once and spend once, so if a used
    /// internal spk has only one recv and one (sufficiently confirmed) spend,
    /// we can skip syncing it.
    ///
    /// The actual sync request built here is a little more conservative, in
    /// that we want to handle various edge cases such as reorgs, replaced txs,
    /// race-y accidental re-use, etc. So we will skip syncing an internal spk
    /// if the txs in and txs out are all balanced and finalized (or there are
    /// no txs out).
    ///
    /// OTOH we can't guarantee much about external keychain spks, so we have to
    /// sync every revealed external spk in perpetuity. In the future, we may
    /// optimize this so that ancient external spks are synced less frequently.
    fn build_sync_request_at(
        &self,
        synced_at: TimestampMs,
    ) -> (SyncRequest<(KeychainKind, u32)>, SyncStats) {
        let locked_wallet = self.read();

        let keychains = locked_wallet.spk_index();
        let keychains_inner = keychains.inner();
        let tx_graph = locked_wallet.tx_graph();
        let local_chain = locked_wallet.local_chain();
        let chain_tip = local_chain.tip();

        enum SpkInfo {
            External {
                keychain_index: (KeychainKind, u32),
            },
            Internal {
                keychain_index: (KeychainKind, u32),
                num_canonical_spends_to_spk: u32,
                num_finalized_spends_from_spk: u32,
            },
            SingleUse {
                keychain_index: (KeychainKind, u32),
                num_canonical_spends_to_spk: u32,
                num_finalized_spends_from_spk: u32,
            },
        }

        impl SpkInfo {
            fn keychain_index(&self) -> (KeychainKind, u32) {
                match self {
                    SpkInfo::External { keychain_index } => *keychain_index,
                    SpkInfo::Internal { keychain_index, .. }
                    | SpkInfo::SingleUse { keychain_index, .. } =>
                        *keychain_index,
                }
            }

            fn needs_sync(&self) -> bool {
                match self {
                    // Sync all external spks
                    SpkInfo::External { .. } => true,
                    // Sync all (1) unused and (2) used internal and single-use
                    // spks that have pending spends or have no finalized
                    // spends at all.
                    SpkInfo::Internal {
                        num_canonical_spends_to_spk,
                        num_finalized_spends_from_spk,
                        ..
                    }
                    | SpkInfo::SingleUse {
                        num_canonical_spends_to_spk,
                        num_finalized_spends_from_spk,
                        ..
                    } =>
                        num_canonical_spends_to_spk
                            > num_finalized_spends_from_spk
                            || *num_finalized_spends_from_spk == 0,
                }
            }
        }

        // Collect all confirmed or unconfirmed txs deemed to be part of the
        // canonical chain history. Also compute whether each canonical tx is
        // finalized or not, i.e., has enough confirmations.
        let canonical_txs: HashMap<bitcoin::Txid, (Arc<Transaction>, bool)> =
            tx_graph
                .list_canonical_txs(
                    local_chain,
                    chain_tip.block_id(),
                    CanonicalizationParams::default(),
                )
                .map(|c_tx| {
                    // We'll consider a tx finalized if it has enough confs.
                    let conf_height =
                        c_tx.chain_position.confirmation_height_upper_bound();
                    let is_finalized = match conf_height {
                        Some(height) => {
                            let confs =
                                (chain_tip.height() + 1).saturating_sub(height);
                            confs >= Self::CONFS_TO_FINALIZE
                        }
                        // unconfirmed => not finalized
                        None => false,
                    };
                    (c_tx.tx_node.txid, (c_tx.tx_node.tx.clone(), is_finalized))
                })
                .collect();

        // All txids relevant to each spk we want to sync. Ok to reference spks
        // that we don't actually sync.
        let mut expected_spk_txids =
            HashSet::<(&bitcoin::ScriptBuf, bitcoin::Txid)>::new();

        // Collect info about every revealed spk.
        let mut spk_infos: HashMap<&bitcoin::ScriptBuf, SpkInfo> = keychains
            .inner()
            .all_spks() // all spks we've ever revealed
            .iter()
            .map(|(&keychain_index, spk)| {
                // Number of canonical outputs that spend to this spk (only used
                // for internal and single-use spks)
                let mut num_canonical_spends_to_spk = 0;

                let outputs_to_spk = keychains_inner
                    .outputs_in_range(keychain_index..=keychain_index);
                for (_keychain_index, outpoint) in outputs_to_spk {
                    if canonical_txs.contains_key(&outpoint.txid) {
                        num_canonical_spends_to_spk += 1;
                        expected_spk_txids.insert((spk, outpoint.txid));
                    }
                }

                // We need to sync all revealed external spks.
                // TODO(phlip9): optimize this so we sync ancient external spks
                // less frequently.
                let (keychain_kind, _index) = keychain_index;
                let spk_info = match keychain_kind {
                    KeychainKind::External =>
                        SpkInfo::External { keychain_index },
                    KeychainKind::Internal => SpkInfo::Internal {
                        keychain_index,
                        num_canonical_spends_to_spk,
                        // We need to compute this in the next pass
                        num_finalized_spends_from_spk: 0,
                    },
                    KeychainKind::SingleUse => SpkInfo::SingleUse {
                        keychain_index,
                        num_canonical_spends_to_spk,
                        num_finalized_spends_from_spk: 0,
                    },
                };
                (spk, spk_info)
            })
            .collect();

        // 1. Collect all canonical txs that spend from an spk
        // 2. Count the number of finalized txs that spend from each internal
        //    spks.
        for (txid, (tx, is_finalized)) in canonical_txs {
            for input in &tx.input {
                if let Some((_keychain_index, txout)) =
                    keychains_inner.txout(input.previous_output)
                {
                    // Record another canonical tx that spends from this spk.
                    let spk = &txout.script_pubkey;
                    expected_spk_txids.insert((spk, txid));

                    // Only care about finalized txs now
                    if !is_finalized {
                        continue;
                    }

                    // If this finalized tx input spends from an internal or
                    // single-use spk, record that
                    if let Some(
                        SpkInfo::Internal {
                            num_finalized_spends_from_spk,
                            ..
                        }
                        | SpkInfo::SingleUse {
                            num_finalized_spends_from_spk,
                            ..
                        },
                    ) = spk_infos.get_mut(spk)
                    {
                        *num_finalized_spends_from_spk += 1;
                    }
                }
            }
        }

        // Collect some basic stats on how many spks for each keychain we're
        // going to sync.
        let (syncing_external, syncing_internal, syncing_single_use) = {
            let (mut external, mut internal, mut single_use) = (0, 0, 0);
            for spk_info in spk_infos.values().filter(|info| info.needs_sync())
            {
                let (keychain_kind, _index) = spk_info.keychain_index();
                match keychain_kind {
                    KeychainKind::External => external += 1,
                    KeychainKind::Internal => internal += 1,
                    KeychainKind::SingleUse => single_use += 1,
                }
            }
            (external, internal, single_use)
        };
        let sync_stats = SyncStats {
            is_full_sync: false,
            total_external: locked_wallet
                .num_revealed_spks(KeychainKind::External),
            syncing_external,
            total_internal: locked_wallet
                .num_revealed_spks(KeychainKind::Internal),
            syncing_internal,
            total_single_use: locked_wallet
                .num_revealed_spks(KeychainKind::SingleUse),
            syncing_single_use,
            ..SyncStats::default()
        };

        let spks_to_sync = spk_infos
            .into_iter()
            .filter(|(_spk, spk_info)| spk_info.needs_sync())
            .map(|(spk, spk_info)| (spk_info.keychain_index(), spk.clone()));

        let expected_spk_txids = expected_spk_txids
            .into_iter()
            .map(|(spk, txid)| (spk.clone(), txid));

        let sync_req =
            SyncRequest::<(KeychainKind, u32)>::builder_at(synced_at.to_secs())
                .chain_tip(chain_tip)
                .spks_with_indexes(spks_to_sync)
                // All relevant txids for the spks we want to sync, so BDK can
                // determine if a tx was evicted.
                .expected_spk_txids(expected_spk_txids)
                .build();

        (sync_req, sync_stats)
    }

    /// Conducts a full sync of all script pubkeys derived from all of our
    /// wallet descriptors, until a stop gap is hit on each of our keychains.
    ///
    /// This should be done rarely, i.e. only when creating the wallet or if we
    /// need to restore from a existing seed. See BDK's examples for more info.
    #[instrument(skip_all, name = "(bdk-full-sync)")]
    pub async fn full_sync(
        &self,
        esplora: &LexeEsplora,
    ) -> anyhow::Result<SyncStats> {
        let now = TimestampMs::now();
        let full_scan_request = self.build_full_scan_request_at(now);

        // Scan the blockchain for our keychain script pubkeys until we hit the
        // `stop_gap`.
        let full_scan_result = esplora
            .client()
            .full_scan::<KeychainKind, _>(
                full_scan_request,
                self.scan_params.full_scan_stop_gap,
                Self::BDK_CONCURRENCY,
            )
            .await
            .context("EsploraAsyncExt::full_scan failed")?;

        let mut sync_stats = SyncStats {
            is_full_sync: true,
            ..SyncStats::default()
        };
        sync_stats.with_full_scan_response(&full_scan_result);

        // Apply the combined update to the wallet.
        {
            let mut locked_wallet = self.write();
            locked_wallet
                .apply_update(full_scan_result)
                .context("Could not apply full scan result to wallet")?;
        }

        self.trigger_persist();

        Ok(sync_stats)
    }

    fn build_full_scan_request_at(
        &self,
        synced_at: TimestampMs,
    ) -> FullScanRequest<KeychainKind> {
        let locked_wallet = self.read();
        locked_wallet
            .start_full_scan_at(synced_at.to_secs())
            .build()
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    pub fn get_balance(&self) -> OnchainBalance {
        let balance = self.read().balance();

        // Convert bdk_wallet::Balance to lexe_common::ln::balance::Balance.
        // Not using a From impl bc we don't want `lexe-common` to depend on
        // BDK.
        let bdk_wallet::Balance {
            immature,
            trusted_pending,
            untrusted_pending,
            confirmed,
        } = balance;

        OnchainBalance {
            immature,
            trusted_pending,
            untrusted_pending,
            confirmed,
        }
    }

    /// Get a [`UtxoCounts`] for the UTXOs tracked by BDK.
    pub fn get_utxo_counts(&self) -> UtxoCounts {
        self.read().list_unspent().fold(
            UtxoCounts::default(),
            |mut acc, utxo| {
                acc.total += 1;
                if utxo.chain_position.is_confirmed() {
                    acc.confirmed += 1;
                } else {
                    acc.unconfirmed += 1;
                }
                acc
            },
        )
    }

    /// List all unspent transaction outputs.
    pub fn get_utxos(&self) -> Vec<bdk_wallet::LocalOutput> {
        let locked_wallet = self.read();
        locked_wallet.list_unspent().collect()
    }

    /// Get a [`TxDetails`] for given [`Txid`] inside of the wallet.
    /// If not found, returns `None`.
    pub fn get_tx_details(&self, txid: Txid) -> Option<TxDetails> {
        let locked_wallet = self.read();
        locked_wallet.tx_details(txid.0)
    }

    /// Return the next unused address from the external descriptor. If there
    /// are no unused addresses, this will reveal and return the next one.
    ///
    /// We do this because it prevents a DoS attack where a
    /// `get_next_unused_address` that returns `reveal_next_address` is
    /// called repeatedly. This would reveal an unbounded number of external
    /// spks and make our transaction sync extremely expensive, as sync
    /// requires ~one API call per external spk ever revealed.
    ///
    /// NOTE: an address is "used" from BDK's perspective if we've broadcasted a
    /// relevant transaction or we've seen a relevant transaction on-chain or
    /// in the mempool.
    ///
    /// NOTE: If a user tries to send two on-chain txs to their wallet in quick
    /// succession, the second call to `get_next_unused_address` will return the
    /// same address as the first if the wallet has not yet detected the
    /// first transaction. If the user wishes to avoid address reuse, they
    /// should wait for their wallet to sync before sending the second
    /// transaction (or simply avoid this scenario in the first place).
    pub fn get_next_unused_address(&self) -> bitcoin::Address {
        let address = self
            .write()
            .next_unused_address(KeychainKind::External)
            .address;
        self.trigger_persist();
        address
    }

    /// Reveal, persist, and return a fresh address from the single-use
    /// keychain.
    ///
    /// Unlike [`Self::get_next_unused_address`], every call reveals a new
    /// address.
    pub async fn get_exclusive_address(
        &self,
    ) -> anyhow::Result<bitcoin::Address> {
        // Reveal the next single-use address.
        let address = {
            let mut locked_wallet = self.write();
            // Without a SingleUse keychain, BDK would reveal an External
            // address instead.
            let has_single_use = locked_wallet
                .keychains()
                .any(|(keychain, _)| keychain == KeychainKind::SingleUse);
            ensure!(has_single_use, "Wallet has no single-use keychain");

            locked_wallet
                .reveal_next_address(KeychainKind::SingleUse)
                .address
        };

        // Await the write-through persist before handing out the address.
        self.persist_and_wait()
            .await
            .context("Failed to persist single-use reveal")?;

        Ok(address)
    }

    /// Return the next unused address from the internal (change) descriptor.
    /// If there are no unused addresses, this will reveal and return the next
    /// one.
    ///
    /// This method should be preferred over `get_next_unused_address` when the
    /// address will never be exposed to the user in any way, e.g. protocol
    /// transactions, change outputs, etc.... It allows our [`sync`]
    /// implementation to avoid checking the address for updates after it
    /// has been finalized, i.e., outputs in and inputs out are both
    /// balanced and all input spends have at least `Self::CONFS_TO_FINALIZE`
    /// confs.
    ///
    /// NOTE: an address is "used" from BDK's perspective if we've broadcasted a
    /// relevant transaction or we've seen a relevant transaction on-chain or
    /// in the mempool.
    ///
    /// NOTE: If a user somehow sees this address and sends funds to it after it
    /// has been finalized, their funds will not show up in the wallet, because
    /// it won't be synced! If this happens somehow, we will need to trigger a
    /// [`Self::full_sync`] to pick up the funds.
    ///
    /// [`sync`]: Self::sync
    pub fn get_internal_address(&self) -> bitcoin::Address {
        let address = self
            .write()
            .next_unused_address(KeychainKind::Internal)
            .address;
        self.trigger_persist();
        address
    }

    /// Returns the scriptpubkey that we should receive channel force-close
    /// outputs to.
    ///
    /// We now send all time-locked, contestible channel force-close outputs to
    /// the same external address (the one at index 0).
    ///
    /// We previously returned `get_internal_address` here (next unused), but
    /// that was unsafe because we have to commit to this spk upfront at channel
    /// open, and other txs or channel close txs could end up using the same
    /// internal address but close at very different times. This could cause the
    /// internal spk to finalize and prevent us from detecting funds from a
    /// force-close tx broadcasted by our counterparty.
    ///
    /// If we returned e.g. `reveal_next` here, we would "leak" an internal
    /// address (i.e., it would stay unused and get synced forever) in the
    /// normal case where a channel is never force-closed.
    ///
    /// Returning a fixed external address is extremely simple and ensures we
    /// always pick up force close outputs, at the cost of rare address reuse.
    pub(crate) fn get_destination_script(&self) -> bitcoin::ScriptBuf {
        let spk = self.write().external_spk_0();
        self.trigger_persist();
        spk
    }

    /// Notifies the BDK wallet that transactions created by us were
    /// successfully broadcasted and exist in the mempool. This avoids the need
    /// to resync the wallet post-broadcast just to observe transactions that we
    /// already know are in the mempool.
    ///
    /// NOTE: This function should be called after every successful broadcast,
    /// otherwise BDK may double-spend the outputs spent by these transactions,
    /// which usually results in the second transaction failing to be broadcast
    /// due to not meeting RBF requirements.
    ///
    /// TODO(max): If the transactions never confirm, their spent outputs might
    /// be locked forever. BDK is working on a fix.
    ///
    /// - Main issue: <https://github.com/bitcoindevkit/bdk/issues/1748>
    /// - Explanation of 'inserted' vs 'unbroadcasted' vs other states: <https://github.com/bitcoindevkit/bdk/issues/1642#issuecomment-2399575535>
    /// - tnull opened an issue which applies to our current approach: <https://github.com/bitcoindevkit/bdk/issues/1666#issue-2621291151>
    pub(crate) fn transactions_broadcasted(
        &self,
        transactions: Vec<Transaction>,
    ) {
        let now = TimestampMs::now();
        self.transactions_broadcasted_at(now, transactions);
    }

    fn transactions_broadcasted_at(
        &self,
        broadcasted_at: TimestampMs,
        transactions: Vec<Transaction>,
    ) {
        let broadcasted_at_secs = broadcasted_at.to_duration().as_secs();
        let transactions =
            transactions.into_iter().map(|tx| (tx, broadcasted_at_secs));
        self.write().apply_unconfirmed_txs(transactions);
        self.trigger_persist();
    }

    /// Try to evict an _unconfirmed_ UTXO from BDK's UTXO index. This
    /// effectively tells BDK that an unconfirmed UTXO was evicted from the
    /// mempool.
    #[cfg(test)]
    fn unconfirmed_utxo_evicted_at(
        &self,
        evicted_at: TimestampMs,
        outpoint: OutPoint,
    ) -> anyhow::Result<()> {
        let mut locked_wallet = self.write();
        let outpoint = bitcoin::OutPoint::from(outpoint);
        let utxo = locked_wallet
            .get_utxo(outpoint)
            .context("No UTXO with this outpoint")?;
        ensure!(
            utxo.chain_position.is_unconfirmed(),
            "UTXO is already confirmed"
        );
        let evicted_at_secs = evicted_at.to_duration().as_secs();
        locked_wallet.apply_evicted_txs(std::iter::once((
            outpoint.txid,
            evicted_at_secs,
        )));
        drop(locked_wallet);
        self.trigger_persist();
        Ok(())
    }

    /// Mark an unconfirmed transaction as evicted from the mempool at a
    /// timestamp. Has no effect if the transaction is already confirmed or is
    /// unknown to the wallet.
    #[cfg(test)]
    fn unconfirmed_transaction_evicted_at(
        &self,
        evicted_at: TimestampMs,
        txid: lexe_common::ln::hashes::Txid,
    ) {
        let evicted_at_secs = evicted_at.to_duration().as_secs();
        self.write()
            .apply_evicted_txs(std::iter::once((txid.0, evicted_at_secs)));
        self.trigger_persist();
    }

    /// Preflight a potential channel open.
    ///
    /// Determines if we have enough on-chain balance for a potential channel
    /// funding tx of this `channel_value_sats`. If so, return the estimated
    /// on-chain fees.
    pub(crate) fn preflight_channel_funding_tx(
        &self,
        channel_value: Amount,
    ) -> anyhow::Result<Amount> {
        // TODO(phlip9): need more correct approach here. Ultimately, we can't
        // exactly predict the final output since that would require the
        // actual channel negotiation. But we should probably account for our
        // `UserConfig` at least?
        //
        // Experimentally determined output script length for LSP<->User node:
        // output_script = [
        //   OP_0
        //   OP_PUSHBYTES_32
        //   <32-byte hash>
        // ]
        // => len == 34 bytes
        let fake_output_script = bitcoin::ScriptBuf::from_bytes(vec![0x69; 34]);

        let mut locked_wallet = self.write();

        // Build
        let conf_prio = Self::CHANNEL_FUNDING_CONF_PRIO;
        let feerate = self.fee_estimates.conf_prio_to_feerate(conf_prio);
        let mut tx_builder = Self::default_tx_builder(
            &mut locked_wallet,
            self.coin_selector,
            feerate,
        );
        tx_builder
            .add_recipient(fake_output_script, channel_value.into())
            // We're just estimating fees, use a fake drain script to prevent
            // creating and tracking new internal change outputs.
            .drain_to(helpers::fake_drain_script());
        let psbt = tx_builder
            // This possibly 'uses' a change address.
            .finish()
            .context("Could not build channel funding tx")?;
        // This unmarks the change address that was just 'used'.
        locked_wallet.cancel_tx(&psbt.unsigned_tx);

        let fee: bitcoin::Amount = psbt.fee().context("Bad PSBT fee")?;
        let fee_amount = Amount::try_from(fee).context("Bad fee amount")?;

        Ok(fee_amount)
    }

    /// Create and sign a funding tx given an output script, channel value, and
    /// confirmation target. Intended to be called downstream of an
    /// [`FundingGenerationReady`] event
    ///
    /// [`FundingGenerationReady`]: lightning::events::Event::FundingGenerationReady
    pub(crate) fn create_and_sign_funding_tx(
        &self,
        output_script: bitcoin::ScriptBuf,
        channel_value: bitcoin::Amount,
    ) -> anyhow::Result<Transaction> {
        let mut locked_wallet = self.write();

        // Build
        let conf_prio = Self::CHANNEL_FUNDING_CONF_PRIO;
        let feerate = self.fee_estimates.conf_prio_to_feerate(conf_prio);
        let mut tx_builder = Self::default_tx_builder(
            &mut locked_wallet,
            self.coin_selector,
            feerate,
        );
        tx_builder.add_recipient(output_script, channel_value);
        let mut psbt = tx_builder
            .finish()
            .context("Could not build funding PSBT")?;
        self.trigger_persist();

        // Sign
        Self::default_sign_psbt(&locked_wallet, &mut psbt)
            .context("Could not sign funding PSBT")?;
        let tx = psbt.extract_tx().context("Could not extract tx")?;

        Ok(tx)
    }

    /// Create and sign a transaction which sends the given amount to the given
    /// address, returning a new [`PaymentWithMetadata<OnchainSendV2>`].
    pub(crate) fn create_onchain_send(
        &self,
        req: PayOnchainRequest,
        network: Network,
        client_pk: Option<ed25519::PublicKey>,
    ) -> anyhow::Result<PaymentWithMetadata<OnchainSendV2>> {
        let (tx, onchain_fee) = {
            let mut locked_wallet = self.write();

            let address = req
                .address
                .clone()
                .require_network(network.into())
                .context("Invalid network")?;

            // Build unsigned tx
            let feerate = self.fee_estimates.conf_prio_to_feerate(req.priority);
            let mut tx_builder = Self::default_tx_builder(
                &mut locked_wallet,
                self.coin_selector,
                feerate,
            );
            tx_builder
                .add_recipient(address.script_pubkey(), req.amount.into());
            let mut psbt = tx_builder
                .finish()
                .context("Failed to build onchain send tx")?;

            // Extract fees
            let bitcoin_fee = psbt.fee().context("Bad PSBT fee")?;
            let fee = Amount::try_from_sats_u64(bitcoin_fee.to_sat())
                .context("Bad fee amount")?;

            // Sign tx
            Self::default_sign_psbt(&locked_wallet, &mut psbt)
                .context("Could not sign outbound tx")?;
            let tx = psbt.extract_tx().context("Could not extract tx")?;

            (tx, fee)
        };
        self.trigger_persist();

        let kind = PaymentKind::Onchain;
        OnchainSendV2::new(tx, req, kind, onchain_fee, client_pk)
            .context("Failed to create payment")
    }

    /// Create a sweep transaction that drains all funds from the wallet to
    /// `dest_address`.
    pub fn create_sweep_tx(
        &self,
        dest_address: &bitcoin::Address,
        priority: ConfirmationPriority,
    ) -> anyhow::Result<(Transaction, Amount)> {
        let dest_script = dest_address.script_pubkey();
        let feerate = self.fee_estimates.conf_prio_to_feerate(priority);

        let mut locked_wallet = self.write();

        // Build drain transaction using BDK's drain_wallet + drain_to. This
        // will fail if the balance is dust (can't cover fees).
        let mut tx_builder = locked_wallet.build_tx();
        tx_builder
            .drain_wallet()
            .drain_to(dest_script)
            .fee_rate(feerate);

        let mut psbt =
            tx_builder.finish().context("Failed to build sweep tx")?;

        // Get the sweep tx fee
        let fee = psbt.fee().context("Bad sweep fee")?;
        let fee = Amount::try_from(fee).context("Bad sweep fee amount")?;

        // Sign the transaction
        let sign_opts = SignOptions::default();
        let finalized = locked_wallet
            .sign(&mut psbt, sign_opts)
            .context("Failed to sign sweep tx")?;
        ensure!(finalized, "Sweep tx signing did not finalize all inputs");
        let tx = psbt
            .extract_tx()
            .context("Failed to extract signed sweep tx")?;

        // NOTE(phlip9): don't need to trigger_persist(), as we don't need to
        // e.g. reveal a change address when sweeping the wallet.

        Ok((tx, fee))
    }

    /// Estimate the network fee for a potential onchain send payment. We return
    /// estimates for each [`ConfirmationPriority`] preset.
    ///
    /// This fn deliberately avoids modifying the wallet state. We don't want to
    /// generate unnecessary addresses that we need to watch and sync.
    pub(crate) fn pay_onchain_preflight(
        &self,
        req: PayOnchainPreflightRequest,
        network: Network,
    ) -> anyhow::Result<PayOnchainPreflightResponse> {
        let high_prio = ConfirmationPriority::High;
        let normal_prio = ConfirmationPriority::Normal;
        let background_prio = ConfirmationPriority::Background;

        let high_feerate = self.fee_estimates.conf_prio_to_feerate(high_prio);
        let normal_feerate =
            self.fee_estimates.conf_prio_to_feerate(normal_prio);
        let background_feerate =
            self.fee_estimates.conf_prio_to_feerate(background_prio);

        let mut locked_wallet = self.write();

        // We _require_ a tx to at least be able to use normal fee rate.
        let address = req.address.require_network(network.into())?;
        let normal_fee = Self::pay_onchain_preflight_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            normal_feerate,
        )?;
        let background_fee = Self::pay_onchain_preflight_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            background_feerate,
        )?;

        // The high fee rate tx is allowed to fail with insufficient balance.
        let high_fee = Self::pay_onchain_preflight_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            high_feerate,
        )
        .ok();

        Ok(PayOnchainPreflightResponse {
            high: high_fee,
            normal: normal_fee,
            background: background_fee,
        })
    }

    fn pay_onchain_preflight_inner(
        wallet: &mut Wallet,
        coin_selector: LexeCoinSelector,
        address: &bitcoin::Address,
        amount: Amount,
        feerate: bitcoin::FeeRate,
    ) -> anyhow::Result<FeeEstimate> {
        let mut tx_builder =
            Self::default_tx_builder(wallet, coin_selector, feerate);
        tx_builder
            .add_recipient(address.script_pubkey(), amount.into())
            // We're just estimating fees, use a fake drain script to prevent
            // creating and tracking new internal change outputs.
            .drain_to(helpers::fake_drain_script());
        let psbt = tx_builder
            .finish()
            .context("Failed to build onchain send tx")?;
        // This currently does ~nothing as of 1.0.0-beta.5, but will eventually
        // free up any UTXOs that were "reserved" by the preflight tx.
        wallet.cancel_tx(&psbt.unsigned_tx);
        let fee = psbt.fee().context("Bad PSBT fee")?;
        let amount = Amount::try_from_sats_u64(fee.to_sat())
            .context("Bad fee amount")?;
        Ok(FeeEstimate { amount })
    }

    /// Get a [`TxBuilder`] which has some defaults prepopulated.
    fn default_tx_builder(
        wallet: &mut Wallet,
        coin_selector: LexeCoinSelector,
        feerate: bitcoin::FeeRate,
    ) -> TxBuilder<'_, LexeCoinSelector> {
        let mut tx_builder = wallet.build_tx().coin_selection(coin_selector);
        // Set the feerate. RBF is already enabled by default.
        tx_builder.fee_rate(feerate);
        tx_builder
    }

    /// Sign a [`Psbt`] in the default way.
    fn default_sign_psbt(
        wallet: &Wallet,
        psbt: &mut Psbt,
    ) -> anyhow::Result<()> {
        let options = SignOptions::default();
        let finalized = wallet.sign(psbt, options)?;
        ensure!(finalized, "Failed to sign all PSBT inputs");
        Ok(())
    }

    /// See: LDK [WalletSource::list_confirmed_utxos][lightning::util::wallet_utils::WalletSource::list_confirmed_utxos]
    fn list_confirmed_utxos_inner(
        &self,
    ) -> Result<Vec<wallet_utils::Utxo>, ()> {
        self.read()
            .list_unspent()
            .filter_map(|output| {
                if !output.chain_position.is_confirmed() {
                    return None;
                }
                helpers::bdk_utxo_to_ldk_utxo(&output).transpose()
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|err| {
                error!("list_confirmed_utxos: {err:#}");
            })
    }

    /// See: LDK [WalletSource::get_prevtx][lightning::util::wallet_utils::WalletSource::get_prevtx]
    fn get_prevtx_inner(
        &self,
        outpoint: bitcoin::OutPoint,
    ) -> Result<bitcoin::Transaction, ()> {
        self.read()
            .get_tx(outpoint.txid)
            .map(|tx| tx.tx_node.tx.as_ref().clone())
            .ok_or(())
    }

    /// See: LDK [WalletSource::get_change_script][lightning::util::wallet_utils::WalletSource::get_change_script]
    fn get_change_script_inner(&self) -> Result<bitcoin::ScriptBuf, ()> {
        Ok(self.get_internal_address().script_pubkey())
    }

    /// See: LDK [WalletSource::sign_psbt][lightning::util::wallet_utils::WalletSource::sign_psbt]
    fn sign_psbt_inner(
        &self,
        mut psbt: bitcoin::Psbt,
    ) -> Result<bitcoin::Transaction, ()> {
        let sign_options = SignOptions {
            // While BDK populates both `witness_utxo` and `non_witness_utxo`
            // fields, LDK does not. As BDK by default doesn't trust the witness
            // UTXO to account for the Segwit bug, we must disable it here as
            // otherwise we fail to sign.
            trust_witness_utxo: true,
            ..SignOptions::default()
        };

        // LDK signs its own inputs (e.g. the anchor input) after us, so an
        // unfinalized PSBT is fine.
        self.read()
            .sign(&mut psbt, sign_options)
            .context("Failed to sign psbt")
            .and_then(|_finalized| {
                psbt.extract_tx().context("Failed to extract tx from psbt")
            })
            .map_err(|err| {
                error!("sign_psbt: {err:#}");
            })
    }
}

// LDK uses this interface into our on-chain wallet to select confirmed UTXOs
// for fee bumping and RBF. We need this to support anchor channels.
#[allow(clippy::manual_async_fn)] // Lint suggestion breaks compilation
impl wallet_utils::WalletSource for OnchainWallet {
    fn list_confirmed_utxos(
        &self,
    ) -> impl Future<Output = Result<Vec<wallet_utils::Utxo>, ()>> + Send + '_
    {
        async move { self.list_confirmed_utxos_inner() }
    }

    fn get_prevtx(
        &self,
        outpoint: bitcoin::OutPoint,
    ) -> impl Future<Output = Result<Transaction, ()>> + Send + '_ {
        async move { self.get_prevtx_inner(outpoint) }
    }

    fn get_change_script(
        &self,
    ) -> impl Future<Output = Result<bitcoin::ScriptBuf, ()>> + Send + '_ {
        async move { self.get_change_script_inner() }
    }

    fn sign_psbt(
        &self,
        psbt: Psbt,
    ) -> impl Future<Output = Result<Transaction, ()>> + Send + '_ {
        async move { self.sign_psbt_inner(psbt) }
    }
}

// --- Wallet persister task --- //

/// Spawns a task that (re-)persists the total wallet [`ChangeSet`] whenever
/// it receives a [`WalletPersistRequest`].
pub fn spawn_wallet_persister_task<PS: LexePersister>(
    persister: PS,
    wallet: OnchainWallet,
    mut wallet_persister_rx: mpsc::Receiver<WalletPersistRequest>,
    // TODO(phlip9): only shutdown persisters after "activity" generating tasks
    // shutdown.
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
    LxTask::spawn("wallet persister", async move {
        loop {
            tokio::select! {
                biased;
                Some(request) = wallet_persister_rx.recv() => {
                    // Drain queued requests into one persist.
                    let mut ack_txs = Vec::from_iter(request);
                    while let Ok(request) = wallet_persister_rx.try_recv() {
                        ack_txs.extend(request);
                    }

                    // An earlier trigger may have merged a requester's staged
                    // changes and then failed to persist them, leaving nothing
                    // staged. So a batch with any ack persists even when
                    // nothing is staged, and the ack proves the changes are on
                    // disk.
                    let force = !ack_txs.is_empty();
                    let result = wallet.persist(&persister, force).await;
                    for ack_tx in ack_txs {
                        let ack = result
                            .as_ref()
                            .map(|&()| ())
                            .map_err(|e| anyhow!("{e:#}"));
                        let _ = ack_tx.send(ack);
                    }
                }
                () = shutdown.recv() => break,
            }
        }
        info!("wallet db persister task shutdown");
    })
}

// --- Descriptor derivation --- //

/// Derives BIP84 wpkh output descriptors from a master xpriv.
///
/// Returns four descriptors (BIP389 multipath, external, internal,
/// single-use) and the account-level xpub at `m/84'/{coin}'/0'`.
///
/// Where `coin` is 0 for mainnet, 1 for testnet/signet/regtest.
pub fn derive_bip84_descriptors(
    network: Network,
    master_xprv: bitcoin::bip32::Xpriv,
) -> OnchainDescriptors {
    let has_single_use = true;
    derive_bip84_descriptors_inner(network, master_xprv, has_single_use)
}

/// [`derive_bip84_descriptors`] for a legacy (<= node-v0.9.1) wallet, which
/// has no single-use keychain.
pub fn derive_legacy_bip84_descriptors(
    network: Network,
    legacy_master_xprv: bitcoin::bip32::Xpriv,
) -> OnchainDescriptors {
    let has_single_use = false;
    derive_bip84_descriptors_inner(network, legacy_master_xprv, has_single_use)
}

fn derive_bip84_descriptors_inner(
    network: Network,
    master_xprv: bitcoin::bip32::Xpriv,
    has_single_use: bool,
) -> OnchainDescriptors {
    let coin_type = match network {
        Network::Mainnet => 0,
        Network::Testnet3
        | Network::Testnet4
        | Network::Regtest
        | Network::Signet => 1,
    };

    // Master fingerprint for descriptor origin info
    let master_xpub = Xpub::from_priv(&SECP256K1, &master_xprv);
    let fingerprint = master_xpub.fingerprint();

    // BIP84 account path: m/84h/{coin}h/0h
    let account_path = [
        ChildNumber::from_hardened_idx(84).expect("Checked in tests"),
        ChildNumber::from_hardened_idx(coin_type).expect("Checked in tests"),
        ChildNumber::from_hardened_idx(0).expect("Checked in tests"),
    ];
    let account_xprv = master_xprv
        .derive_priv(&SECP256K1, &account_path)
        .expect("derivation should succeed");
    let account_xpub = Xpub::from_priv(&SECP256K1, &account_xprv);

    // Build descriptor strings
    let origin = format!("[{fingerprint}/84'/{coin_type}'/0']");
    let with_checksum = |body: String| {
        let checksum = miniscript::descriptor::checksum::desc_checksum(&body)
            .expect("valid descriptor");
        format!("{body}#{checksum}")
    };

    // BIP389 multipath: wpkh([fp/84'/coin'/0']xpub/<0;1;2>/*)
    let paths = if has_single_use { "<0;1;2>" } else { "<0;1>" };
    let multipath_descriptor =
        with_checksum(format!("wpkh({origin}{account_xpub}/{paths}/*)"));
    // External: wpkh([fp/84'/coin'/0']xpub/0/*)
    let external_descriptor =
        with_checksum(format!("wpkh({origin}{account_xpub}/0/*)"));
    // Internal: wpkh([fp/84'/coin'/0']xpub/1/*)
    let internal_descriptor =
        with_checksum(format!("wpkh({origin}{account_xpub}/1/*)"));
    // Single-use: wpkh([fp/84'/coin'/0']xpub/2/*)
    let single_use_descriptor = has_single_use
        .then(|| with_checksum(format!("wpkh({origin}{account_xpub}/2/*)")));

    OnchainDescriptors {
        multipath_descriptor,
        external_descriptor,
        internal_descriptor,
        single_use_descriptor,
        account_xpub,
    }
}

// --- impl SyncStats --- //

impl SyncStats {
    fn with_sync_response(&mut self, resp: &SyncResponse) {
        self.with_tx_update(&resp.tx_update);
    }

    fn with_full_scan_response(
        &mut self,
        resp: &FullScanResponse<KeychainKind>,
    ) {
        self.with_tx_update(&resp.tx_update);
    }

    fn with_tx_update<A>(&mut self, tx: &TxUpdate<A>) {
        self.txs += tx.txs.len() as u32;
        self.txouts += tx.txouts.len() as u32;
        self.anchors += tx.anchors.len() as u32;
        self.seen += tx.seen_ats.len() as u32;
        self.evicted += tx.evicted_ats.len() as u32;
    }

    pub(crate) fn log_sync_complete(&self, is_legacy: bool, elapsed_ms: u128) {
        let kind = if is_legacy { "Legacy " } else { "" };
        let num_ext = self.total_external;
        let sync_ext = self.syncing_external;
        let num_int = self.total_internal;
        let sync_int = self.syncing_internal;
        let num_su = self.total_single_use;
        let sync_su = self.syncing_single_use;
        let txs = self.txs;
        let txouts = self.txouts;
        let anchors = self.anchors;
        let seen = self.seen;
        let evicted = self.evicted;
        if self.is_full_sync {
            info!(
                "{kind}BDK full sync complete <{elapsed_ms}ms> | \
                 {num_int} int, {num_ext} ext, {num_su} su | \
                 resp: txs={txs}, txouts={txouts}, anchors={anchors}, \
                 seen={seen}, evicted={evicted}",
            );
        } else {
            info!(
                "{kind}BDK sync complete <{elapsed_ms}ms> | \
                 req: {sync_int}/{num_int} int, {sync_ext}/{num_ext} ext, \
                 {sync_su}/{num_su} su | \
                 resp: txs={txs}, txouts={txouts}, anchors={anchors}, \
                 seen={seen}, evicted={evicted}",
            );
        }
    }
}

// --- LexeCoinSelector --- //

/// A [`CoinSelectionAlgorithm`] impl which spends the oldest UTXOs first,
/// i.e. it prioritizes confirmed UTXOds over unconfirmed UTXOs.
///
/// Can be configured to log a warning if we select an unconfirmed UTXO.
///
/// Note that `OldestFirstCoinSelection` (FIFO) only has a marginally higher
/// UTXO footprint than the default `BranchAndBoundCoinSelection` provided by
/// BDK (which is itself based on Bitcoin Core's implementation).
/// See section 6.3.2.1 of Murch's paper for details:
/// <https://murch.one/wp-content/uploads/2016/11/erhardt2016coinselection.pdf>
#[derive(Copy, Clone, Debug, Default)]
pub struct LexeCoinSelector {
    /// Whether to log WARNs anytime an unconfirmed UTXO is selected.
    pub log_unconfirmed: bool,
}

impl CoinSelectionAlgorithm for LexeCoinSelector {
    fn coin_select<R: RngCore>(
        &self,
        required_utxos: Vec<WeightedUtxo>,
        optional_utxos: Vec<WeightedUtxo>,
        fee_rate: bitcoin::FeeRate,
        target_amount: bitcoin::Amount,
        drain_script: &bitcoin::Script,
        rand: &mut R,
    ) -> Result<CoinSelectionResult, InsufficientFunds> {
        use bdk_wallet::Utxo;

        /// Whether the given `selection_result` contains any unconfirmed UTXOs.
        fn contains_unconfirmed_utxo(
            selection_result: &CoinSelectionResult,
        ) -> bool {
            selection_result.selected.iter().any(|utxo| match utxo {
                Utxo::Local(local) => !local.chain_position.is_confirmed(),
                Utxo::Foreign { .. } => false,
            })
        }

        // First filter out all foreign UTXOs, as OldestFirstCoinSelection
        // contains a bug which actually selects foreign UTXOs *first*:
        // https://github.com/bitcoindevkit/bdk_wallet/issues/264
        // TODO(max): Remove this filtering once fixed
        let optional_utxos = optional_utxos
            .into_iter()
            .filter(|weighted| match weighted.utxo {
                Utxo::Local(_) => true,
                Utxo::Foreign { .. } => false,
            })
            .collect();

        // This implementation depends on `ChainPosition`'s derived Ord impl;
        // confirmed UTXOs sort "less than" unconfirmed UTXOs, so oldest-first
        // selection spends them first.
        // BDK has a test named `chain_position_ord` which enforces this.
        let selection_result =
            bdk_wallet::coin_selection::OldestFirstCoinSelection.coin_select(
                required_utxos,
                optional_utxos,
                fee_rate,
                target_amount,
                drain_script,
                rand,
            )?;

        if self.log_unconfirmed && contains_unconfirmed_utxo(&selection_result)
        {
            warn!("Selected unconfirmed UTXOs: {selection_result:?}");
        }

        Ok(selection_result)
    }
}

// --- BdkWalletExt --- //

/// An extension trait on the (locked) BDK `Wallet`.
trait BdkWalletExt {
    /// Returns `true` if we have at least one revealed but unused spk in the
    /// given keychain.
    fn have_unused_spk(&self, keychain: KeychainKind) -> bool;

    /// Reveals an spk in the given keychain if there are no unused spks
    /// available.
    fn reveal_spk_if_no_unused(&mut self, keychain: KeychainKind);

    /// Returns the number of revealed spks for this keychain.
    fn num_revealed_spks(&self, keychain: KeychainKind) -> u32;

    /// Returns the first external spk (at index 0). Reveals it if it hasn't
    /// already been revealed.
    fn external_spk_0(&mut self) -> bitcoin::ScriptBuf;
}

impl BdkWalletExt for Wallet {
    fn have_unused_spk(&self, keychain: KeychainKind) -> bool {
        self.spk_index()
            .unused_keychain_spks(keychain)
            .next()
            .is_some()
    }

    fn reveal_spk_if_no_unused(&mut self, keychain: KeychainKind) {
        if !self.have_unused_spk(keychain) {
            let _ = self.reveal_next_address(keychain);
        }
    }

    fn num_revealed_spks(&self, keychain: KeychainKind) -> u32 {
        self.spk_index()
            .last_revealed_index(keychain)
            .map(|index| index + 1)
            .unwrap_or(0)
    }

    fn external_spk_0(&mut self) -> bitcoin::ScriptBuf {
        if self.num_revealed_spks(KeychainKind::External) > 0 {
            // we've already revealed an external spk, just return it
            self.spk_index()
                .spk_at_index(KeychainKind::External, 0)
                .expect("We just checked that there's at least one revealed")
        } else {
            self.reveal_next_address(KeychainKind::External)
                .script_pubkey()
        }
    }
}

mod helpers {
    use super::*;

    /// Convert a BDK UTXO into an LDK UTXO. Returns `None` for non-witness
    /// outputs.
    pub fn bdk_utxo_to_ldk_utxo(
        output: &bdk_wallet::LocalOutput,
    ) -> anyhow::Result<Option<wallet_utils::Utxo>> {
        let spk = &output.txout.script_pubkey;
        match spk.witness_version() {
            Some(version @ bitcoin::WitnessVersion::V0) => {
                // See: bitcoin::Script::p2wpkh_script_code
                let witness_bytes = &spk.as_bytes()[2..];
                let witness_program =
                    bitcoin::WitnessProgram::new(version, witness_bytes)?;
                let wpkh = bitcoin::WPubkeyHash::from_slice(
                    witness_program.program().as_bytes(),
                )?;
                let utxo = wallet_utils::Utxo::new_v0_p2wpkh(
                    output.outpoint,
                    output.txout.value,
                    &wpkh,
                );
                Ok(Some(utxo))
            }
            Some(version @ bitcoin::WitnessVersion::V1) => {
                // See: bitcoin::Script::p2wpkh_script_code
                let witness_bytes = &spk.as_bytes()[2..];
                let witness_program =
                    bitcoin::WitnessProgram::new(version, witness_bytes)?;

                // Should be XOnlyPublicKey
                let _ = bitcoin::XOnlyPublicKey::from_slice(
                    witness_program.program().as_bytes(),
                )?;

                // See: ldk-node/src/wallet/mod.rs
                let utxo = wallet_utils::Utxo {
                    outpoint: output.outpoint,
                    output: bitcoin::TxOut {
                        value: output.txout.value,
                        script_pubkey: bitcoin::ScriptBuf::new_witness_program(
                            &witness_program,
                        ),
                    },
                    #[rustfmt::skip]
                    #[allow(clippy::identity_op)] // Improve readability
                    satisfaction_weight:
                        1 /* empty script_sig */ * bitcoin::blockdata::weight::WITNESS_SCALE_FACTOR as u64
                        + 1 /* witness items */
                        + 1 /* schnorr sig len */ + 64, /* schnorr sig */
                    sequence: bitcoin::Sequence::ENABLE_RBF_NO_LOCKTIME,
                };
                Ok(Some(utxo))
            }
            // Unrecognized witness version
            Some(_version) => Ok(None),
            // Skip non-P2WPKH outputs. Our on-chain wallet doesn't
            // currently have Taproot or legacy keychains.
            None => Ok(None),
        }
    }

    /// Use this fake TXO drain script to prevent the BDK wallet from modifying
    /// its internal state when building fake txs, like the ones used to
    /// estimate fees during preflight.
    pub fn fake_drain_script() -> bitcoin::ScriptBuf {
        let wpkh = bitcoin::WPubkeyHash::from_byte_array([0xaa; 20]);
        bitcoin::ScriptBuf::new_p2wpkh(&wpkh)
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use std::sync::Arc;

    use bdk_chain::{
        ConfirmationBlockTime, DescriptorId, keychain_txout, local_chain,
        tx_graph,
    };
    use bdk_wallet::{ChangeSet, KeychainKind, template::DescriptorTemplate};
    use bitcoin::hashes::Hash as _;
    use lexe_common::{root_seed::RootSeed, test_utils::arbitrary};
    use proptest::{
        arbitrary::any,
        option,
        strategy::{Just, Strategy},
    };

    use super::*;

    type KeychainChangeset = keychain_txout::ChangeSet;
    type TxGraphChangeset = tx_graph::ChangeSet<ConfirmationBlockTime>;

    pub(super) fn any_changeset() -> impl Strategy<Value = ChangeSet> {
        let network = Network::Mainnet;
        let seed = RootSeed::from_u64(20241114);
        let master_xprv = seed.derive_legacy_master_xprv(network);
        let just_descriptor = Just({
            let (descriptor, _, _) = Bip84(master_xprv, KeychainKind::External)
                .build(network.to_bitcoin())
                .unwrap();
            descriptor
        });
        let just_change_descriptor = Just({
            let (descriptor, _, _) = Bip84(master_xprv, KeychainKind::Internal)
                .build(network.to_bitcoin())
                .unwrap();
            descriptor
        });
        let just_single_use_descriptor = Just({
            let (descriptor, _, _) =
                Bip84(master_xprv, KeychainKind::SingleUse)
                    .build(network.to_bitcoin())
                    .unwrap();
            descriptor
        });

        (
            option::of(just_descriptor),
            option::of(just_change_descriptor),
            option::of(just_single_use_descriptor),
            option::of(any::<Network>().prop_map(Into::into)),
            any_localchain_changeset(),
            any_txgraph_changeset(),
            any_keychain_changeset(),
        )
            .prop_map(
                |(
                    descriptor,
                    change_descriptor,
                    single_use_descriptor,
                    network,
                    local_chain,
                    tx_graph,
                    indexer,
                )| {
                    ChangeSet {
                        descriptor,
                        change_descriptor,
                        single_use_descriptor,
                        network,
                        local_chain,
                        tx_graph,
                        indexer,
                    }
                },
            )
    }

    fn any_txgraph_changeset() -> impl Strategy<Value = TxGraphChangeset> {
        let any_arc_tx = arbitrary::any_raw_tx().prop_map(Arc::new);
        let any_txs = proptest::collection::btree_set(any_arc_tx, 0..4);
        let any_txouts = proptest::collection::btree_map(
            arbitrary::any_outpoint(),
            arbitrary::any_txout(),
            0..4,
        );
        let anchors = proptest::collection::btree_set(
            (any_confirmationblocktime(), arbitrary::any_txid()),
            0..4,
        );
        let txid_map = proptest::collection::btree_map(
            arbitrary::any_txid(),
            any::<u64>(),
            0..4,
        );
        let last_seen = txid_map.clone();
        let last_evicted = txid_map.clone();
        let first_seen = txid_map;

        (
            any_txs,
            any_txouts,
            anchors,
            last_seen,
            last_evicted,
            first_seen,
        )
            .prop_map(
                |(
                    txs,
                    txouts,
                    anchors,
                    last_seen,
                    last_evicted,
                    first_seen,
                )| {
                    TxGraphChangeset {
                        txs,
                        txouts,
                        anchors,
                        last_seen,
                        last_evicted,
                        first_seen,
                    }
                },
            )
    }

    fn any_keychain_changeset() -> impl Strategy<Value = KeychainChangeset> {
        let any_descriptor_id = any::<[u8; 32]>()
            .prop_map(bitcoin::hashes::sha256::Hash::from_byte_array)
            .prop_map(DescriptorId);
        let last_revealed = proptest::collection::btree_map(
            any_descriptor_id.clone(),
            any::<u32>(),
            0..4,
        );
        let script_bufs = proptest::collection::btree_map(
            any::<u32>(),
            arbitrary::any_script(),
            0..4,
        );
        let spk_cache = proptest::collection::btree_map(
            any_descriptor_id,
            script_bufs,
            0..4,
        );

        (last_revealed, spk_cache).prop_map(|(last_revealed, spk_cache)| {
            KeychainChangeset {
                last_revealed,
                spk_cache,
            }
        })
    }

    fn any_localchain_changeset()
    -> impl Strategy<Value = local_chain::ChangeSet> {
        proptest::collection::btree_map(
            any::<u32>(),
            option::of(arbitrary::any_blockhash()),
            0..4,
        )
        .prop_map(|blocks| local_chain::ChangeSet { blocks })
    }

    fn any_confirmationblocktime()
    -> impl Strategy<Value = ConfirmationBlockTime> {
        (any_blockid(), any::<u64>()).prop_map(
            |(block_id, confirmation_time)| ConfirmationBlockTime {
                block_id,
                confirmation_time,
            },
        )
    }

    fn any_blockid() -> impl Strategy<Value = bdk_chain::BlockId> {
        (any::<u32>(), arbitrary::any_blockhash())
            .prop_map(|(height, hash)| bdk_chain::BlockId { height, hash })
    }
}

#[cfg(test)]
mod test {
    use std::{
        collections::{BTreeMap, BTreeSet},
        fs, iter,
        path::Path,
        process::{Command, Stdio},
        str::FromStr,
    };

    use bdk_chain::{BlockId, ConfirmationBlockTime, DescriptorExt};
    use bdk_wallet::{
        AddressInfo,
        KeychainKind::{self, External, Internal, SingleUse},
        miniscript::DescriptorPublicKey,
        template::{Bip84, DescriptorTemplate},
    };
    use bitcoin::{TxOut, hashes::Hash as _};
    use lexe_api::types::payments::ClientPaymentId;
    use lexe_common::{
        ln::hashes::Txid,
        root_seed::RootSeed,
        sat,
        test_utils::{arbitrary, roundtrip},
    };
    use lexe_crypto::rng::FastRng;
    use lightning::util::wallet_utils::WalletSource as _;
    use proptest::{
        prop_assert, prop_assert_eq, proptest, test_runner::Config,
    };
    use tracing::trace;

    use super::*;

    struct Harness {
        wallet: OnchainWallet,
        network: Network,
        seed: u64,
        wallet_persister_rx: mpsc::Receiver<WalletPersistRequest>,
    }

    impl Harness {
        fn new(seed: u64) -> Self {
            let h = Self::from_changeset(seed, None);

            // Add some initial confirmed blocks
            {
                let mut w = h.ww();
                w.add_checkpoint(100);
                w.add_checkpoint(900);
            }

            h.persist();
            h
        }

        /// Load the wallet from `changeset`, or create it if `None`.
        fn from_changeset(seed: u64, changeset: Option<ChangeSet>) -> Self {
            let network = Network::Regtest;
            let master_xprv =
                RootSeed::from_u64(seed).derive_bip32_master_xprv(network);
            let (wallet, wallet_persister_rx) =
                OnchainWallet::dummy(master_xprv, changeset);
            Harness {
                wallet,
                network,
                seed,
                wallet_persister_rx,
            }
        }

        /// Reload the wallet from the aggregated changeset alone, dropping
        /// any staged (unpersisted) changes. Emulates a crash + reboot.
        fn reload(&self) -> Self {
            let changeset = self.wallet.changeset.lock().unwrap().clone();
            Self::from_changeset(self.seed, Some(changeset))
        }

        /// Get the wallet write lock.
        fn ww(&self) -> RwLockWriteGuard<'_, Wallet> {
            self.wallet.write()
        }

        /// Get the wallet read lock.
        fn wr(&self) -> RwLockReadGuard<'_, Wallet> {
            self.wallet.read()
        }

        /// Return the fake clock timestamp
        fn now(&self) -> TimestampMs {
            self.wr().now()
        }

        /// "Persist" the current staged wallet changes into the in-memory
        /// changeset. Returns `true` if there were any staged changes.
        fn persist(&self) -> bool {
            let staged = self.ww().take_staged();
            if let Some(update) = staged {
                self.wallet.changeset.lock().unwrap().merge(update);
                true
            } else {
                false
            }
        }

        /// Check that running the given function `f` does not generate any new
        /// persists in the wallet.
        fn assert_no_persists_in<F, R>(&mut self, f: F) -> R
        where
            F: FnOnce(&mut Self) -> R,
        {
            self.persist();
            let ret = f(self);
            assert_eq!(None, self.ww().take_staged());
            ret
        }

        /// Reveal a single-use address, standing in for the wallet persister
        /// task by acking the write-through persist with `persist_result`.
        async fn reveal_single_use_with(
            &mut self,
            persist_result: anyhow::Result<()>,
        ) -> anyhow::Result<bitcoin::Address> {
            let reveal_fut = self.wallet.get_exclusive_address();
            tokio::pin!(reveal_fut);
            let mut persist_result = Some(persist_result);
            loop {
                tokio::select! {
                    result = &mut reveal_fut => return result,
                    // Drain plain triggers too, or a queued one deadlocks the
                    // reveal (select! disables a branch whose pattern fails).
                    Some(maybe_ack_tx) = self.wallet_persister_rx.recv() => {
                        self.persist();
                        if let Some(ack_tx) = maybe_ack_tx {
                            ack_tx.send(persist_result.take().unwrap()).unwrap();
                        }
                    }
                }
            }
        }

        /// Reveal a single-use address with a successful write-through persist.
        async fn reveal_single_use(&mut self) -> bitcoin::Address {
            self.reveal_single_use_with(Ok(())).await.unwrap()
        }

        /// Reveal a single-use address whose write-through persist fails;
        /// asserts that the reveal errors (the address is withheld).
        async fn reveal_single_use_persist_err(&mut self) {
            self.reveal_single_use_with(Err(anyhow!("test: persist failed")))
                .await
                .unwrap_err();
        }

        /// Assert that the BDK wallet believes we can spend a given amount and
        /// return the fee
        #[track_caller]
        fn assert_spend_ok(&self, amount_sats: u32) -> Amount {
            let amount = Amount::from_sats_u32(amount_sats);
            self.wallet.preflight_channel_funding_tx(amount).unwrap()
        }

        /// Assert that the BDK wallet believes we can't spend a given amount
        #[track_caller]
        fn assert_spend_err(&self, amount_sats: u32) {
            let amount = Amount::from_sats_u32(amount_sats);
            self.wallet
                .preflight_channel_funding_tx(amount)
                .unwrap_err();
        }

        /// Make a new onchain send spending `amount` to `address` and register
        /// it with the BDK wallet as broadcasted.
        fn spend_unconfirmed(
            &self,
            address: bitcoin::Address<bitcoin::address::NetworkUnchecked>,
            amount: Amount,
        ) -> PaymentWithMetadata<OnchainSendV2> {
            let send_req = PayOnchainRequest {
                client_payment_id: ClientPaymentId([42; 32]),
                address,
                amount,
                priority: ConfirmationPriority::Normal,
                personal_note: None,
            };
            let client_pk = None;
            let oswm = self
                .wallet
                .create_onchain_send(send_req, self.network, client_pk)
                .expect("Failed to create onchain send");
            self.wallet.transactions_broadcasted_at(
                self.now(),
                vec![oswm.payment.tx.as_ref().clone()],
            );
            oswm
        }

        /// Assert that building an incremental sync request on the current
        /// wallet state returns the expected set of spks and their associated
        /// expected canonical txids.
        #[track_caller]
        fn assert_sync(
            &self,
            mut expected: BTreeMap<bitcoin::ScriptBuf, BTreeSet<bitcoin::Txid>>,
        ) {
            let (mut sync_req, _sync_stats) =
                self.wallet.build_sync_request_at(self.now());

            let actual = sync_req
                .iter_spks_with_expected_txids()
                .map(|spk_txids| {
                    (
                        spk_txids.spk,
                        BTreeSet::from_iter(spk_txids.expected_txids),
                    )
                })
                .collect::<BTreeMap<_, _>>();

            // Expect all unused spks
            expected.extend(
                self.wr()
                    .spk_index()
                    .inner()
                    .unused_spks(..)
                    .map(|(_, spk)| (spk, BTreeSet::new())),
            );

            if actual != expected {
                println!("Actual:");
                for (spk, txids) in &actual {
                    println!("  {spk:?} => {txids:#?}");
                }
                println!("\nExpected:");
                for (spk, txids) in &expected {
                    println!("  {spk:?} => {txids:#?}");
                }
                panic!("SyncRequest did not return expected spks");
            }

            // We can also check this invariant here
            self.assert_have_unused_after_reload();
        }

        /// Assert that we always have at least one unused internal and external
        /// spk after loading the wallet.
        #[track_caller]
        fn assert_have_unused_after_reload(&self) {
            let mut changeset = self.wallet.changeset.lock().unwrap().clone();
            let staged = self.wr().staged().cloned();
            if let Some(update) = staged {
                changeset.merge(update);
            }

            let h = Self::from_changeset(self.seed, Some(changeset));
            assert!(h.wr().have_unused_spk(KeychainKind::External));
            assert!(h.wr().have_unused_spk(KeychainKind::Internal));
        }
    }

    /// An extension trait on the (locked) BDK `Wallet` to make writing wallet
    /// tests more ergonomic.
    trait BdkWalletTestExt {
        fn height(&self) -> u32;

        /// A fake clock timestamp that's just `unix time := height * 100 sec`.
        fn now(&self) -> TimestampMs;

        /// Fund the wallet with the given amount
        fn fund(
            &mut self,
            keychain: KeychainKind,
            amount: Amount,
        ) -> (Transaction, AddressInfo, bitcoin::ScriptBuf);

        /// Fund the wallet with the given amount, but leave the funding tx
        /// unconfirmed.
        fn fund_unconfirmed(
            &mut self,
            keychain: KeychainKind,
            amount: Amount,
        ) -> (Transaction, AddressInfo, bitcoin::ScriptBuf);

        /// Confirm the given txids in the next block, then add enough blocks to
        /// give them X confirmations.
        fn confirm_txids(&mut self, confs: u32, txids: &[bitcoin::Txid]);

        fn add_unconfirmed_tx(&mut self, tx: &Transaction);

        /// Add a new block checkpoint at `height + blocks`.
        fn add_checkpoint(&mut self, blocks: u32) -> ConfirmationBlockTime;
    }

    impl BdkWalletTestExt for Wallet {
        fn height(&self) -> u32 {
            self.local_chain().tip().height()
        }

        fn now(&self) -> TimestampMs {
            TimestampMs::from_secs_u32(self.height() * 100)
        }

        fn fund(
            &mut self,
            keychain: KeychainKind,
            amount: Amount,
        ) -> (Transaction, AddressInfo, bitcoin::ScriptBuf) {
            let (tx, address_info, spks) =
                self.fund_unconfirmed(keychain, amount);
            self.confirm_txids(
                OnchainWallet::CONFS_TO_FINALIZE,
                &[tx.compute_txid()],
            );
            (tx, address_info, spks)
        }

        fn fund_unconfirmed(
            &mut self,
            keychain: KeychainKind,
            amount: Amount,
        ) -> (Transaction, AddressInfo, bitcoin::ScriptBuf) {
            // build tx and register it
            let address_info = self.next_unused_address(keychain);
            let spk = address_info.script_pubkey();
            let tx = Transaction {
                output: vec![TxOut {
                    value: amount.into(),
                    script_pubkey: spk.clone(),
                }],
                ..new_tx()
            };
            self.add_unconfirmed_tx(&tx);
            (tx, address_info, spk)
        }

        fn add_checkpoint(&mut self, blocks: u32) -> ConfirmationBlockTime {
            let new_height = self.height() + blocks;
            let block_id = BlockId::from_u32(new_height);
            let mut cp = self.latest_checkpoint();
            cp = cp.insert(block_id);
            self.apply_update(bdk_wallet::Update {
                chain: Some(cp),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(self.height(), new_height);
            ConfirmationBlockTime {
                block_id,
                confirmation_time: 100 * new_height as u64,
            }
        }

        fn add_unconfirmed_tx(&mut self, tx: &Transaction) {
            let now = self.now().to_secs();
            self.apply_unconfirmed_txs(iter::once((tx.clone(), now)));
        }

        fn confirm_txids(&mut self, confs: u32, txids: &[bitcoin::Txid]) {
            assert!(confs > 0);
            let anchor = self.add_checkpoint(1);
            let anchors = txids.iter().map(|txid| (anchor, *txid)).collect();
            let mut tx_update = bdk_chain::TxUpdate::default();
            tx_update.anchors = anchors;
            let update = bdk_wallet::Update {
                tx_update,
                ..Default::default()
            };
            self.apply_update(update).unwrap();
            self.add_checkpoint(confs - 1);
        }
    }

    fn new_tx() -> Transaction {
        Transaction {
            version: bitcoin::transaction::Version::TWO,
            lock_time: bitcoin::absolute::LockTime::ZERO,
            input: Vec::new(),
            output: Vec::new(),
        }
    }

    /// A third-party regtest address.
    fn address3p() -> bitcoin::Address<bitcoin::address::NetworkUnchecked> {
        bitcoin::Address::from_str(
            "bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk",
        )
        .unwrap()
    }

    trait BlockHashExt {
        fn from_u32(n: u32) -> Self;
    }

    impl BlockHashExt for bitcoin::BlockHash {
        fn from_u32(n: u32) -> Self {
            let mut hash = [0u8; 32];
            hash[0..4].copy_from_slice(&n.to_le_bytes());
            bitcoin::BlockHash::from_byte_array(hash)
        }
    }

    trait BlockIdExt {
        fn from_u32(n: u32) -> Self;
    }

    impl BlockIdExt for BlockId {
        fn from_u32(n: u32) -> Self {
            BlockId {
                height: n,
                hash: bitcoin::BlockHash::from_u32(n),
            }
        }
    }

    macro_rules! map {
        ($($key:expr => $value:expr),* $(,)?) => {
            {
                #[allow(unused_mut)]
                let mut map = std::collections::BTreeMap::new();
                #[allow(unused_mut)]
                let mut all_unique = true;
                $(all_unique &= map.insert($key.clone(), $value.clone()).is_none();)*
                assert!(all_unique, "All map keys must be unique");
                map
            }
        };
    }

    macro_rules! set {
        ($($key:expr),* $(,)?) => {
            {
                #[allow(unused_mut)]
                let mut map = std::collections::BTreeSet::new();
                #[allow(unused_mut)]
                let mut all_unique = true;
                $(all_unique &= map.insert($key.clone());)*
                assert!(all_unique, "All set items must be unique");
                map
            }
        };
    }

    #[tokio::test]
    async fn wallet_source_selects_and_signs_confirmed_utxo() {
        let h = Harness::new(891372190);
        let (confirmed_tx, _, _) = h.ww().fund(External, sat!(10_000));
        h.ww().fund_unconfirmed(Internal, sat!(20_000));

        let mut utxos = h.wallet.list_confirmed_utxos().await.unwrap();
        assert_eq!(utxos.len(), 1);
        let utxo = utxos.pop().unwrap();
        assert_eq!(
            utxo.outpoint,
            bitcoin::OutPoint::new(confirmed_tx.compute_txid(), 0)
        );
        assert_eq!(utxo.output, confirmed_tx.output[0]);
        assert_eq!(
            h.wallet.get_prevtx(utxo.outpoint).await.unwrap(),
            confirmed_tx
        );

        let change_script = h.wallet.get_change_script().await.unwrap();
        assert!(change_script.is_p2wpkh());
        let unsigned_tx = Transaction {
            input: vec![bitcoin::TxIn {
                previous_output: utxo.outpoint,
                ..Default::default()
            }],
            output: vec![TxOut {
                value: sat!(9_000).into(),
                script_pubkey: change_script,
            }],
            ..new_tx()
        };
        let mut psbt = Psbt::from_unsigned_tx(unsigned_tx).unwrap();
        psbt.inputs[0].witness_utxo = Some(utxo.output);

        let signed_tx = h.wallet.sign_psbt(psbt).await.unwrap();
        assert!(!signed_tx.input[0].witness.is_empty());
    }

    /// `WalletSource` serves and signs single-use utxos, including in a
    /// mixed-keychain PSBT with only `witness_utxo` set, as LDK builds them.
    #[tokio::test]
    async fn wallet_source_spans_single_use() {
        let h = Harness::new(505050505);
        let (main_tx, _, _) = h.ww().fund(External, sat!(10_000));
        let (su_tx, _, _) = h.ww().fund(SingleUse, sat!(12_000));
        h.ww().fund_unconfirmed(SingleUse, sat!(5_000));

        let utxos = h.wallet.list_confirmed_utxos().await.unwrap();
        let txids = utxos
            .iter()
            .map(|utxo| utxo.outpoint.txid)
            .collect::<BTreeSet<_>>();
        let expected_txids =
            BTreeSet::from([main_tx.compute_txid(), su_tx.compute_txid()]);
        assert_eq!(txids, expected_txids);

        let su_outpoint = bitcoin::OutPoint::new(su_tx.compute_txid(), 0);
        assert_eq!(h.wallet.get_prevtx(su_outpoint).await.unwrap(), su_tx);

        let change_script = h.wallet.get_change_script().await.unwrap();
        let unsigned_tx = Transaction {
            input: utxos
                .iter()
                .map(|utxo| bitcoin::TxIn {
                    previous_output: utxo.outpoint,
                    ..Default::default()
                })
                .collect(),
            output: vec![TxOut {
                value: sat!(21_000).into(),
                script_pubkey: change_script,
            }],
            ..new_tx()
        };
        let mut psbt = Psbt::from_unsigned_tx(unsigned_tx).unwrap();
        for (psbt_input, utxo) in psbt.inputs.iter_mut().zip(&utxos) {
            psbt_input.witness_utxo = Some(utxo.output.clone());
        }

        let signed_tx = h.wallet.sign_psbt(psbt).await.unwrap();
        assert!(
            signed_tx
                .input
                .iter()
                .all(|input| !input.witness.is_empty())
        );
    }

    #[test]
    fn transactions_broadcasted_applies_package_as_batch() {
        let h = Harness::new(7421031);
        let wallet_spk = h.ww().next_unused_address(External).script_pubkey();
        let parent = Transaction {
            output: vec![TxOut {
                value: sat!(10_000).into(),
                script_pubkey: wallet_spk,
            }],
            ..new_tx()
        };
        let parent_txid = Txid(parent.compute_txid());
        let child = Transaction {
            input: vec![bitcoin::TxIn {
                previous_output: bitcoin::OutPoint::new(parent_txid.0, 0),
                ..Default::default()
            }],
            output: vec![TxOut {
                value: sat!(9_000).into(),
                script_pubkey: bitcoin::ScriptBuf::new(),
            }],
            ..new_tx()
        };
        let child_txid = Txid(child.compute_txid());

        // Child-first order only works if BDK indexes the whole package before
        // deciding which transactions are wallet-relevant.
        h.wallet
            .transactions_broadcasted_at(h.now(), vec![child, parent]);

        assert!(h.wallet.get_tx_details(parent_txid).is_some());
        assert!(h.wallet.get_tx_details(child_txid).is_some());
        assert!(h.wallet.get_utxos().is_empty());
    }

    #[test]
    fn external_spk_0() {
        let mut h = Harness::new(789981416358);

        // we have no reveled external spks initially
        assert_eq!(h.wr().num_revealed_spks(KeychainKind::External), 0);

        // first external_spk_0 call also reveals it
        let spk_a = h.ww().external_spk_0();
        assert!(h.persist());
        assert_eq!(h.wr().num_revealed_spks(KeychainKind::External), 1);

        // external_spk_0 again should not change state
        let spk_b = h.assert_no_persists_in(|h| h.ww().external_spk_0());
        let spk_c = h.assert_no_persists_in(|h| h.ww().external_spk_0());
        assert_eq!(h.wr().num_revealed_spks(KeychainKind::External), 1);
        assert_eq!(spk_a, spk_b);
        assert_eq!(spk_a, spk_c);
    }

    #[test]
    fn drain_script_equiv() {
        let h = Harness::new(3684944666541);
        let address = h.wallet.get_internal_address();

        let spk = address.script_pubkey();
        assert_eq!(
            spk.len(),
            helpers::fake_drain_script().len(),
            "Drain script ({spk:?}) has unexpected length"
        );
        assert_eq!(
            spk.minimal_non_dust(),
            helpers::fake_drain_script().minimal_non_dust(),
            "Drain script ({spk:?}) has unexpected dust threshold"
        );
    }

    // `preflight_{open_channel,pay_onchain}` should not have any side effects
    #[test]
    fn preflight_doesnt_persist() {
        use bitcoin::{Address, Script};

        // with a single large utxo
        let mut h = Harness::new(498484646866);
        h.ww().fund(External, sat!(123_456));
        assert_eq!(h.wallet.get_balance().confirmed.to_sat(), 123_456);

        // open_channel_preflight
        h.assert_no_persists_in(|h| {
            let fee =
                h.wallet.preflight_channel_funding_tx(sat!(12_345)).unwrap();
            assert_eq!(fee.sats_u64(), 305);
        });

        // pay_onchain_preflight
        let address = {
            let network = h.network.to_bitcoin();
            let script = Script::from_bytes(&[0x42; 10]);
            Address::p2wsh(script, network).into_unchecked()
        };
        h.assert_no_persists_in(|h| {
            let req = PayOnchainPreflightRequest {
                client_payment_id: None,
                address: address.clone(),
                amount: sat!(12_345),
            };
            let fee = h.wallet.pay_onchain_preflight(req, h.network).unwrap();
            assert_eq!(fee.high.map(|x| x.amount.sats_u64()), Some(382));
            assert_eq!(fee.normal.amount.sats_u64(), 305);
            assert_eq!(fee.background.amount.sats_u64(), 185);
        });

        // use a fresh wallet, as coin selection is apparently
        // non-deterministic :')
        // with some smaller utxos so we get multiple inputs to the funding tx
        let mut h = Harness::new(15841984431000);
        h.ww().fund(External, sat!(11_500));
        h.ww().fund(External, sat!(11_500));

        // open_channel_preflight
        h.assert_no_persists_in(|h| {
            let amount = sat!(12_345);
            let fee = h.wallet.preflight_channel_funding_tx(amount).unwrap();
            assert_eq!(fee.sats_u64(), 441);
        });

        // pay_onchain_preflight
        h.assert_no_persists_in(|h| {
            let req = PayOnchainPreflightRequest {
                client_payment_id: None,
                address: address.clone(),
                amount: sat!(12_345),
            };
            let fee = h.wallet.pay_onchain_preflight(req, h.network).unwrap();
            assert_eq!(fee.high.map(|x| x.amount.sats_u64()), Some(552));
            assert_eq!(fee.normal.amount.sats_u64(), 441);
            assert_eq!(fee.background.amount.sats_u64(), 267);
        });
    }

    // Test that we prefer confirmed UTXOs over unconfirmed ones as it's
    // somewhat unsafe for us to use unconfirmed UTXOs when opening JIT
    // channels to user nodes.
    //
    // For example, we've experienced an issue where our LSP closed a channel
    // with an external peer, the external peer RBFs, and the LSP mistakenly
    // used the unconfirmed but replaced UTXO to open a new JIT channel to a
    // user node, which resulted in a broken channel.
    //
    // NOTE: manually tested that this fails if `default_tx_builder` uses the
    // BDK `DefaultCoinSelectionAlgorithm`.
    #[test]
    fn test_coinselection_prefers_confirmed() {
        let h = Harness::new(7768794005608);

        // 1. add an unconfirmed tx
        h.ww().fund_unconfirmed(External, sat!(20_000));

        // 2. we should still be able to spend this unconfirmed tx
        h.assert_spend_ok(9_000);

        // 3. add a confirmed tx
        let (tx_c, _, _) = h.ww().fund(External, sat!(10_000));

        // 4. we can still spend from both
        h.assert_spend_ok(18_000);

        // 5. but we should prefer the confirmed tx
        let address = address3p();
        let req = PayOnchainRequest {
            client_payment_id: ClientPaymentId([42; 32]),
            address,
            amount: sat!(9_000),
            priority: ConfirmationPriority::Normal,
            personal_note: None,
        };
        let client_pk = None;
        let oswm = h
            .wallet
            .create_onchain_send(req, h.network, client_pk)
            .unwrap();
        let tx = &oswm.payment.tx;
        assert_eq!(tx.input.len(), 1);
        assert_eq!(tx.input[0].previous_output.txid, tx_c.compute_txid());
    }

    #[test]
    fn test_evict_unconfirmed_utxo_basic() {
        let h = Harness::new(78798781005005050);

        // Fund w/ 5,656 sats (confirmed)
        let (tx_c, _, _) = h.ww().fund(External, sat!(5_656));

        // Fund w/ 12,121 sats (broadcasted + unconfirmed)
        let (tx_u, _, _) = h.ww().fund_unconfirmed(Internal, sat!(12_121));
        assert_eq!(h.wallet.get_balance().trusted_pending.to_sat(), 12_121);

        // We can't spend more than our total balance
        h.assert_spend_err(20_000);

        // We should be able to spend this broadcasted + unconfirmed UTXO
        h.assert_spend_ok(10_000);

        // Manually declare it evicted / replaced / dropped from the mempool
        let now = h.now();
        let outpoint = OutPoint {
            txid: Txid(tx_u.compute_txid()),
            index: 0,
        };
        h.wallet.unconfirmed_utxo_evicted_at(now, outpoint).unwrap();

        // We can no longer spend it
        h.assert_spend_err(10_000);

        // We can still spend the confirmed UTXO
        h.assert_spend_ok(4_000);

        // We can't evict a confirmed UTXO
        let outpoint = OutPoint {
            txid: Txid(tx_c.compute_txid()),
            index: 0,
        };
        h.wallet
            .unconfirmed_utxo_evicted_at(now, outpoint)
            .unwrap_err();
    }

    // Test that our incremental sync request correctly handles unconfirmed txs
    // getting evicted from the mempool.
    #[test]
    fn test_evict_unconfirmed_utxo_sync_request() {
        let h = Harness::new(224357022208);

        // Fund w/ 5,656 sats (confirmed)
        let (txi0_1, _, spki0) = h.ww().fund(Internal, sat!(5_656));
        let txidi0_1 = txi0_1.compute_txid();
        let utxos = h.wallet.get_utxos();
        let utxoi0_1 = &utxos[0];
        h.assert_spend_ok(5_300);

        assert_eq!(utxos.len(), 1);
        assert_eq!(utxoi0_1.outpoint.txid, txidi0_1);
        assert_eq!(utxoi0_1.txout, txi0_1.output[0]);
        assert!(!utxoi0_1.is_spent);

        // check the sync request
        h.assert_sync(map! { spki0 => set! { txidi0_1 } });

        // Spend 1,000 sats to an external address (unconfirmed)
        // -> Balance: ~4,375 sats (w/ unconfirmed)
        let address = address3p();
        let oswm = h.spend_unconfirmed(address, sat!(1_000));
        let txidi1_1 = oswm.payment.txid.0;
        // Change output spk
        let spki1 = h.wr().spk_index().spk_at_index(Internal, 1).unwrap();

        // check the sync request
        h.assert_sync(map! {
            spki0 => set! { txidi0_1, txidi1_1 },
            spki1 => set! { txidi1_1 },
        });

        h.assert_spend_err(5_300);
        h.assert_spend_ok(4_000);

        // Evict the unconfirmed UTXO.
        h.wallet
            .unconfirmed_transaction_evicted_at(h.now(), oswm.payment.txid);
        h.assert_spend_ok(5_300);

        // Our sync should still include the internal spk used by the evicted tx
        // but we don't expect the evicted txid anymore.
        h.assert_sync(map! {
            spki0 => set! { txidi0_1 },
            spki1 => set! { },
        });

        h.wallet.transactions_broadcasted_at(
            h.now(),
            vec![oswm.payment.tx.as_ref().clone()],
        );
    }

    // Test that we build the expected incremental sync request in various
    // scenarios.
    #[test]
    fn test_build_sync_request() {
        let h = Harness::new(2869818889840);

        // Empty wallet should build empty SyncRequest.
        h.assert_sync(map! {});

        trace!("== unconfirmed fund ===");

        // Fund the wallet with 1,000 sats (unconfirmed, internal)
        let (txi0, ai0, spki0) = h.ww().fund_unconfirmed(Internal, sat!(1_000));
        let txidi0_1 = txi0.compute_txid();
        h.assert_sync(map! { spki0 => set! { txidi0_1 } });

        trace!("=== finalize fund ===");

        // Confirm tx. Should still sync spk i0 since it's unspent.
        h.ww().confirm_txids(6, &[txi0.compute_txid()]);
        h.assert_sync(map! { spki0 => set! { txidi0_1 } });

        // Spend the 1,000 sats to some third party address (unconfirmed).
        let address3p = address3p();
        let oswm = h.spend_unconfirmed(address3p.clone(), sat!(775));

        trace!("=== unconfirmed spend ===");

        // Still sync spk i0 since the spend is unconfirmed
        let txidi0_2 = oswm.payment.txid.0;
        h.assert_sync(map! { spki0 => set! { txidi0_1, txidi0_2 } });

        trace!("=== confirm spend ===");

        // Confirm the send tx but not enough to finalize. Should still sync
        // spk i0.
        h.ww().confirm_txids(5, &[oswm.payment.txid.0]);
        h.assert_sync(map! { spki0 => set! { txidi0_1, txidi0_2 } });

        trace!("=== finalize spend ===");

        // Confirm enough to finalize. No longer sync spk i0.
        h.ww().add_checkpoint(1);
        h.assert_sync(map! {});

        trace!("=== somehow fund spki0 again ===");

        // Somehow we fund spk i0 again. Even though this shouldn't happen, we
        // should still support syncing it to completion.
        let tx = Transaction {
            output: vec![TxOut {
                value: sat!(1_234).into(),
                script_pubkey: ai0.script_pubkey(),
            }],
            ..new_tx()
        };
        let txidi0_3 = tx.compute_txid();
        h.wallet.transactions_broadcasted_at(h.now(), vec![tx]);
        h.assert_sync(map! { spki0 => set! { txidi0_1, txidi0_2, txidi0_3 } });

        trace!("=== immediately spend to external ===");

        // Even though in/out are balanced, we should still sync spk i0 since
        // the spend is unconfirmed.
        let ae0 = h.ww().next_unused_address(External);
        let addresse0 = ae0.address.clone().into_unchecked();
        let spke0 = ae0.script_pubkey();
        let oswm = h.spend_unconfirmed(addresse0, sat!(1_000));
        let txide0_1 = oswm.payment.txid.0;
        h.assert_sync(map! {
            spki0 => set! { txidi0_1, txidi0_2, txidi0_3, txide0_1 },
            spke0 => set! { txide0_1 },
        });

        trace!("=== confirm spend ===");

        // Confirm the send tx but not enough to finalize. Should still sync
        // spk i0.
        h.ww().confirm_txids(5, &[oswm.payment.txid.0]);
        h.assert_sync(map! {
            spki0 => set! { txidi0_1, txidi0_2, txidi0_3, txide0_1 },
            spke0 => set! { txide0_1 },
        });

        trace!("=== finalize spend ===");

        // Confirm enough to finalize. No longer sync spk i0. spk e0 will
        // continue to sync forever since it's an external spk.
        h.ww().add_checkpoint(1);
        h.assert_sync(map! { spke0 => set! { txide0_1 } });

        trace!("=== spend external ===");

        // Spend the external address to some third party address. Syncs
        // should always include the external spk.
        let oswm = h.spend_unconfirmed(address3p, sat!(775));
        let txide0_2 = oswm.payment.txid.0;
        h.assert_sync(map! { spke0 => set! { txide0_1, txide0_2 } });

        h.ww().confirm_txids(5, &[oswm.payment.txid.0]);
        h.assert_sync(map! { spke0 => set! { txide0_1, txide0_2 } });

        h.ww().add_checkpoint(5);
        h.assert_sync(map! { spke0 => set! { txide0_1, txide0_2 } });

        trace!("=== spend to self internal ===");

        // Fund spki1
        let (txi1, ai1, spki1) = h.ww().fund(Internal, sat!(1_000));
        let txidi1_1 = txi1.compute_txid();
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1 },
        });

        // Not sure how we would spend back to a used internal spk, but at least
        // it won't break sync.
        let txi1_self = Transaction {
            input: vec![bitcoin::TxIn {
                previous_output: bitcoin::OutPoint {
                    txid: txidi1_1,
                    vout: 0,
                },
                ..Default::default()
            }],
            output: vec![TxOut {
                value: sat!(1_000).into(),
                script_pubkey: ai1.script_pubkey(),
            }],
            ..new_tx()
        };
        let txidi1_2 = txi1_self.compute_txid();
        h.wallet
            .transactions_broadcasted_at(h.now(), vec![txi1_self]);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1, txidi1_2 },
        });

        h.ww().confirm_txids(5, &[txidi1_2]);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1, txidi1_2 },
        });

        h.ww().add_checkpoint(1);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1, txidi1_2 },
        });

        trace!("=== spend to another internal spk ===");

        // Spend to another internal address
        let spki2 = h.ww().next_unused_address(Internal).script_pubkey();
        let tx2 = Transaction {
            input: vec![bitcoin::TxIn {
                previous_output: bitcoin::OutPoint {
                    txid: txidi1_2,
                    vout: 0,
                },
                ..Default::default()
            }],
            output: vec![TxOut {
                value: sat!(1_000).into(),
                script_pubkey: spki2.clone(),
            }],
            ..new_tx()
        };
        let txidi2_1 = tx2.compute_txid();
        h.wallet.transactions_broadcasted_at(h.now(), vec![tx2]);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1, txidi1_2, txidi2_1 },
            spki2 => set! { txidi2_1 },
        });

        // Confirm
        h.ww().confirm_txids(5, &[txidi2_1]);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki1 => set! { txidi1_1, txidi1_2, txidi2_1 },
            spki2 => set! { txidi2_1 },
        });

        // Finalize -- spki1 should no longer need sync
        h.ww().add_checkpoint(1);
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki2 => set! { txidi2_1 },
        });

        // If we generate an internal addresses without using it, it should
        // be synced separately.
        let a3 = h.wallet.get_internal_address();
        let spki3 = a3.script_pubkey();
        h.assert_sync(map! {
            spke0 => set! { txide0_1, txide0_2 },
            spki2 => set! { txidi2_1 },
            spki3 => set! {},
        });
    }

    // --- Single-use keychain --- //

    #[tokio::test]
    async fn single_use_reveal_is_unique_and_write_through() {
        let mut h = Harness::new(89230412);

        let addr0 = h.reveal_single_use().await;
        let addr1 = h.reveal_single_use().await;
        assert_ne!(addr0, addr1);

        // The persister merged the reveals into the aggregated changeset
        // before acking; nothing is left staged.
        assert!(h.wr().staged().is_none());
        let descriptor_id = h.wr().public_descriptor(SingleUse).descriptor_id();
        let last_revealed = h
            .wallet
            .changeset
            .lock()
            .unwrap()
            .indexer
            .last_revealed
            .get(&descriptor_id)
            .copied();
        assert_eq!(last_revealed, Some(1));
    }

    #[tokio::test]
    async fn single_use_reveal_survives_reload() {
        let mut h = Harness::new(777123);
        let addr0 = h.reveal_single_use().await;

        // Reload from the aggregated changeset alone (i.e. without any
        // harness-side persist): write-through must have persisted the
        // reveal already.
        let mut h2 = h.reload();
        let addr1 = h2.reveal_single_use().await;
        assert_ne!(addr0, addr1);
    }

    #[tokio::test]
    async fn single_use_failed_persist_withholds_address() {
        let mut h = Harness::new(20260903001);

        // The state the last successful persist wrote to "disk".
        let disk_changeset = h.wallet.changeset.lock().unwrap().clone();

        // The reveal errors when the persist fails; addr0 is withheld.
        let addr0 = h.wr().peek_address(SingleUse, 0).address;
        h.reveal_single_use_persist_err().await;

        // An in-process retry hands out the next address, not addr0: the
        // failed reveal still advanced the revealed index, and the retry's
        // successful persist covers both reveals.
        let addr1 = h.reveal_single_use().await;
        assert_ne!(addr1, addr0);

        // Crash + reload from the pre-failure disk state re-issues the
        // withheld address.
        let mut h2 = Harness::from_changeset(h.seed, Some(disk_changeset));
        let addr2 = h2.reveal_single_use().await;
        assert_eq!(addr2, addr0);
    }

    /// Without a single-use keychain, the reveal errors instead of falling
    /// back to the External keychain as BDK does.
    #[tokio::test]
    async fn single_use_address_requires_keychain() {
        let network = Network::Regtest;
        let master_xprv =
            RootSeed::from_u64(31337).derive_legacy_master_xprv(network);
        let (wallet_persister_tx, _wallet_persister_rx) =
            mpsc::channel(OnchainWallet::PERSISTER_CHANNEL_SIZE);
        let is_legacy = true;
        let (wallet, _wallet_created) = OnchainWallet::new(
            master_xprv,
            network,
            FeeEstimates::dummy(),
            LexeCoinSelector::default(),
            None,
            AddressScanParams::default(),
            wallet_persister_tx,
            is_legacy,
        )
        .unwrap();

        wallet.get_exclusive_address().await.unwrap_err();
        assert_eq!(wallet.read().num_revealed_spks(External), 0);
    }

    /// Drift in the single-use descriptor derivation would orphan funds on
    /// already issued addresses, so pin it.
    #[test]
    fn single_use_descriptor_snapshot() {
        let root_seed = RootSeed::from_u64(20260903);
        let network = Network::Regtest;
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        let descriptors = derive_bip84_descriptors(network, master_xprv);
        assert_eq!(
            descriptors.single_use_descriptor.as_deref(),
            Some(
                "wpkh([01ee5425/84'/1'/0']tpubDDjY5BgnXWr2kLr2itGRV37RfLnHkXR\
                 LYMGdoXY1s5p7SgGUhFogXVf5NXnaXLYAgLTP5wdhJRNebhvXNPXx9bw59BC\
                 bEw6VgqSDN7xDw2h/2/*)#lxk0rlx2"
            ),
        );

        // The first two single-use addresses.
        let h = Harness::new(20260903);
        let addr0 = h.ww().reveal_next_address(SingleUse).address.to_string();
        let addr1 = h.ww().reveal_next_address(SingleUse).address.to_string();
        assert_eq!(addr0, "bcrt1qj245lyahm824qwlnljcr0w78xdw7kmq4lgyjqd");
        assert_eq!(addr1, "bcrt1q4nlny4vzqsd5cjxsr27sycd9sv9zwghkkqmmus");
    }

    /// A pre-keychain changeset loads, and the aggregated changeset gains the
    /// single-use descriptor.
    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn pre_single_use_changeset_loads() {
        let snapshot =
            fs::read_to_string("data/bip39-onchain-wallet-snapshot.json")
                .unwrap();
        let changeset = serde_json::from_str::<ChangeSet>(&snapshot).unwrap();
        assert_eq!(changeset.single_use_descriptor, None);

        let h = Harness::from_changeset(20260117, Some(changeset));
        let descriptor = h.wr().public_descriptor(SingleUse).clone();
        assert_eq!(h.wr().num_revealed_spks(SingleUse), 0);
        assert_eq!(
            h.wallet.changeset.lock().unwrap().single_use_descriptor,
            Some(descriptor),
        );
    }

    // Test creating full wallet sweep tx
    #[test]
    fn test_sweep_tx() {
        let mut h = Harness::new(2869818889840);

        let address3p = address3p().assume_checked();
        let prio = ConfirmationPriority::Background;

        // sweep should fail with no funds
        h.assert_no_persists_in(|h| {
            h.wallet.create_sweep_tx(&address3p, prio).unwrap_err();
        });

        h.ww().fund(KeychainKind::External, sat!(425));

        // sweep should fail if we will just create dust
        h.assert_no_persists_in(|h| {
            h.wallet.create_sweep_tx(&address3p, prio).unwrap_err();
        });

        h.ww().fund(KeychainKind::External, sat!(5_000));
        h.persist();

        // sweep shouldn't persist any changes, since we don't need to reveal
        // a new change address
        let (tx, fee) = h.assert_no_persists_in(|h| {
            h.wallet
                .create_sweep_tx(&address3p, prio)
                .expect("Sweep should succeed with enough funds")
        });

        let amount_sats: u64 = tx.output.iter().map(|o| o.value.to_sat()).sum();
        assert!(amount_sats > 5_000);
        assert_eq!(amount_sats + fee.sats_u64(), 5_425);

        // registering the tx should trigger a persist
        h.wallet.transactions_broadcasted_at(h.now(), vec![tx]);
        assert!(h.persist());

        // sweeping again should not succeed, since the funds are marked
        // broadcasted
        h.assert_no_persists_in(|h| {
            h.wallet.create_sweep_tx(&address3p, prio).unwrap_err();
        });
    }

    #[test]
    fn default_changeset_is_empty() {
        assert!(ChangeSet::default().is_empty());
    }

    #[test]
    fn changeset_roundtrip_proptest() {
        roundtrip::json_value_custom(
            arbitrary_impl::any_changeset(),
            Config::default(),
        );
    }

    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn test_changeset_snapshots() {
        let snapshot =
            fs::read_to_string("data/changeset-snapshot.v1.0.0-beta.5.json")
                .unwrap();
        serde_json::from_str::<Vec<ChangeSet>>(&snapshot).unwrap();

        let snapshot =
            fs::read_to_string("data/changeset-snapshot.v1.1.0.json").unwrap();
        serde_json::from_str::<Vec<ChangeSet>>(&snapshot).unwrap();
    }

    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored test_changeset_internal_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn test_changeset_internal_snapshot() {
        // bdk_wallet-v1.1.0
        let input_path = Path::new(
            "../../log/lsp.bdk_wallet_changeset.20250611.pretty.json",
        );
        let input = fs::read_to_string(input_path).unwrap();
        let changeset = serde_json::from_str::<ChangeSet>(&input).unwrap();
        let output = serde_json::to_string_pretty(&changeset).unwrap();
        // println!("{output}");
        let output_path = input_path.with_extension("new.json");
        fs::write(&output_path, output).unwrap();

        // git diff --no-index <input_path> <output_path>
        Command::new("git")
            .arg("diff")
            .arg("--no-index")
            .arg(input_path)
            .arg(&output_path)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .unwrap();
    }

    /// Dumps a JSON array of three `ChangeSet`s using the proptest strategy.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored dump_proptest_wallet_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_proptest_wallet_snapshot() {
        let mut rng = FastRng::from_u64(20250316);
        let strategy = arbitrary_impl::any_changeset();
        let changesets = arbitrary::gen_value_iter(&mut rng, strategy)
            .take(3)
            .collect::<Vec<ChangeSet>>();
        println!("---");
        println!("{}", serde_json::to_string_pretty(&changesets).unwrap());
        println!("---");
    }

    /// Tests that we can load a legacy wallet from the saved snapshot.
    ///
    /// These tests ensure we can load wallets created with the legacy
    /// derivation (RootSeed directly into BIP32) even after we change to
    /// BIP39-compatible derivation for new wallets.
    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn test_legacy_wallet_snapshot_loads() {
        let snapshot =
            fs::read_to_string("data/legacy-onchain-wallet-snapshot.json")
                .unwrap();
        let changeset: ChangeSet = serde_json::from_str(&snapshot).unwrap();

        // Verify we can load the wallet from this changeset
        let root_seed = RootSeed::from_u64(20260117);
        let network = Network::Regtest;
        let master_xprv = root_seed.derive_legacy_master_xprv(network);
        let (wallet, _wallet_persister_rx) =
            OnchainWallet::dummy(master_xprv, Some(changeset));

        // Sanity checks: 100k external + 50k internal = 150k total
        let balance = wallet.read().balance();
        assert_eq!(balance.confirmed.to_sat(), 150_000);
    }

    /// Tests that we can load a new BIP39-compatible wallet from snapshot.
    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn test_bip39_wallet_snapshot_loads() {
        let snapshot =
            fs::read_to_string("data/bip39-onchain-wallet-snapshot.json")
                .unwrap();
        let changeset: ChangeSet = serde_json::from_str(&snapshot).unwrap();

        // Verify we can load the wallet from this changeset
        let root_seed = RootSeed::from_u64(20260117);
        let network = Network::Regtest;
        let master_xprv = root_seed.derive_bip32_master_xprv(network);
        let (wallet, _wallet_persister_rx) =
            OnchainWallet::dummy(master_xprv, Some(changeset));

        // Sanity checks: 100k external + 50k internal = 150k total
        let balance = wallet.read().balance();
        assert_eq!(balance.confirmed.to_sat(), 150_000);
    }

    /// Regenerates the wallet snapshot file.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln --lib -- --ignored dump_wallet_snapshot --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_wallet_snapshot() {
        let h = Harness::new(20260117);

        // Fund both keychains with confirmed UTXOs
        {
            let mut w = h.ww();
            w.fund(External, sat!(100_000));
            w.fund(Internal, sat!(50_000));
        }
        h.persist();

        let changeset = h.wallet.changeset.lock().unwrap().clone();

        let json = serde_json::to_string_pretty(&changeset).unwrap();
        println!("---\n{json}\n---");
    }

    /// Test that our `derive_bip84_descriptors` produces descriptors matching
    /// BDK's `Bip84` template, and that our account-level xpub can derive
    /// addresses matching both descriptors.
    #[test]
    fn derive_bip84_descriptors_matches_bdk() {
        let cfg = Config::with_cases(100);

        proptest!(cfg, |(
            seed: RootSeed,
            network: Network,
            idx in 0u32..1000,
        )| {
            let master_xprv = seed.derive_bip32_master_xprv(network);

            // Our implementation
            let descriptors = derive_bip84_descriptors(network, master_xprv);
            let account_zpub = descriptors.account_zpub();
            let OnchainDescriptors {
                multipath_descriptor: our_multi_desc,
                external_descriptor: our_ext_desc,
                internal_descriptor: our_int_desc,
                single_use_descriptor: our_su_desc,
                account_xpub,
            } = descriptors;
            let our_su_desc = our_su_desc.unwrap();

            // BDK's implementation.
            // This is also how we use `Bip84` in `OnchainWallet::init`
            let (bdk_desc_ext, _, _) =
                Bip84(master_xprv, KeychainKind::External)
                    .build(network.to_bitcoin())
                    .unwrap();
            let (bdk_desc_int, _, _) =
                Bip84(master_xprv, KeychainKind::Internal)
                    .build(network.to_bitcoin())
                    .unwrap();
            let (bdk_desc_su, _, _) =
                Bip84(master_xprv, KeychainKind::SingleUse)
                    .build(network.to_bitcoin())
                    .unwrap();

            // Verify external/internal/single-use descriptor strings match BDK
            prop_assert_eq!(&our_ext_desc, &bdk_desc_ext.to_string());
            prop_assert_eq!(&our_int_desc, &bdk_desc_int.to_string());
            prop_assert_eq!(&our_su_desc, &bdk_desc_su.to_string());

            // Verify multipath descriptor expands to external + internal +
            // single-use (differential fuzz against miniscript's
            // into_single_descriptors)
            let parsed =
                miniscript::Descriptor::<DescriptorPublicKey>::from_str(
                    &our_multi_desc
                ).unwrap();
            let expanded = parsed.into_single_descriptors().unwrap();
            prop_assert_eq!(expanded.len(), 3);
            prop_assert_eq!(&expanded[0].to_string(), &our_ext_desc);
            prop_assert_eq!(&expanded[1].to_string(), &our_int_desc);
            prop_assert_eq!(&expanded[2].to_string(), &our_su_desc);

            // Legacy wallets have no single-use keychain
            let legacy = derive_legacy_bip84_descriptors(network, master_xprv);
            prop_assert_eq!(legacy.single_use_descriptor, None);
            prop_assert!(legacy.multipath_descriptor.contains("/<0;1>/"));

            // Verify account_xpub by deriving the address at `idx` on each
            // chain: external (0), internal (1), and single-use (2).
            let our_addr = |chain: u32| {
                let xpub = account_xpub
                    .derive_pub(
                        &SECP256K1,
                        &[
                            ChildNumber::from_normal_idx(chain).unwrap(),
                            ChildNumber::from_normal_idx(idx).unwrap(),
                        ],
                    )
                    .unwrap();
                bitcoin::Address::p2wpkh(
                    &bitcoin::CompressedPublicKey(xpub.public_key),
                    network.to_bitcoin(),
                )
            };
            let bdk_addr =
                |desc: &miniscript::Descriptor<DescriptorPublicKey>| {
                    desc.at_derivation_index(idx)
                        .unwrap()
                        .address(network.to_bitcoin())
                        .unwrap()
                };

            prop_assert_eq!(our_addr(0), bdk_addr(&bdk_desc_ext));
            prop_assert_eq!(our_addr(1), bdk_addr(&bdk_desc_int));
            prop_assert_eq!(our_addr(2), bdk_addr(&bdk_desc_su));

            // Verify account_zpub is account_xpub with a SLIP-132 prefix
            let zpub_prefix = match network {
                Network::Mainnet => "zpub",
                _ => "vpub",
            };
            prop_assert!(account_zpub.starts_with(zpub_prefix));
            let zpub_bytes = bitcoin::base58::decode_check(&account_zpub)
                .unwrap();
            prop_assert_eq!(&zpub_bytes[4..], &account_xpub.encode()[4..]);
        });
    }
}
