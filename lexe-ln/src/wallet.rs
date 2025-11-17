//! # A note on [`ChangeSet`]s and wallet persistence
//!
//! [`bdk_wallet::ChangeSet`] is the top-level data struct given to us by BDK,
//! and is the main thing that need to be persisted. It implements [`Serialize`]
//! / [`Deserialize`], and [`bdk_chain::Merge`], which allows changesets to be
//! merged together. The [`ChangeSet`]s may be persisted in aggregated form, or
//! they can be persisted separately and reaggregated when (re-)initializing our
//! [`Wallet`].
//!
//! Our [`LexeWallet`] uses a write-back model. Changes are:
//!
//! 1) Staged inside [`bdk_wallet::Wallet`],
//! 2) Merged into a total [`ChangeSet`] cached in the [`LexeWallet`], then
//! 3) The total [`ChangeSet`] is (re-)persisted to the DB by the wallet
//!    persister task whenever it receives a notification.
//!
//! NOTE: This persistence model means we need to manually call
//! [`LexeWallet::trigger_persist`] anytime we mutate the BDK [`Wallet`].
//!
//! NOTE: It is possible that we'll lose some data if the node crashes before
//! any staged changes are persisted, but this should be OK because all data in
//! the [`ChangeSet`] can be re-derived with a full sync
//!
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize
//! [`ChangeSet`]: bdk_wallet::ChangeSet
//! [`Wallet`]: bdk_wallet::Wallet
//! [`LexeWallet`]: crate::wallet::LexeWallet
//! [`LexeWallet::trigger_persist`]: crate::wallet::LexeWallet::trigger_persist

use std::{
    collections::{HashMap, HashSet},
    ops::DerefMut,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{Context, ensure};
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
    template::Bip84,
};
use bitcoin::{Psbt, Transaction};
#[cfg(test)]
use common::ln::channel::LxOutPoint;
use common::{
    constants::IMPORTANT_PERSIST_RETRIES,
    ln::{
        amount::Amount, balance::OnchainBalance, hashes::LxTxid,
        network::LxNetwork, priority::ConfirmationPriority,
    },
    root_seed::RootSeed,
    time::TimestampMs,
};
use lexe_api::{
    models::command::{
        FeeEstimate, PayOnchainRequest, PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
    },
    vfs::{SINGLETON_DIRECTORY, Vfs, VfsFileId, WALLET_CHANGESET_FILENAME},
};
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
use rand::RngCore;
use tracing::{debug, info, instrument, warn};

use crate::{
    esplora::{FeeEstimates, LexeEsplora},
    payments::{PaymentWithMetadata, onchain::OnchainSendV2},
    traits::LexePersister,
};

/// The number of confirmations required to consider a transaction finalized.
/// This determines when we'll stop syncing an internal spk (script pubkey)
/// because it has no more pending spends.
const CONFS_TO_FINALIZE: u32 = 6;

/// "`stop_gap` is the maximum number of consecutive unused addresses. For
/// example, with a `stop_gap` of  3, `full_scan` will keep scanning until it
/// encounters 3 consecutive script pubkeys with no associated transactions."
///
/// From: [`EsploraAsyncExt::full_scan`]
const BDK_FULL_SCAN_STOP_GAP: usize = 2;

/// Number of parallel requests BDK is permitted to use.
const BDK_CONCURRENCY: usize = 24;

/// "The lookahead defines a number of script pubkeys to derive over and above
/// the last revealed index."
// We only reveal unused addresses; however, our write back persistence scheme
// means we may sometimes forget that we revealed an index, due to a crash prior
// to persisting the update. Thus, a lookahead of 1 should be sufficient for us.
const BDK_LOOKAHEAD: u32 = 1;

/// The [`ConfirmationPriority`] for new open_channel funding transactions.
///
/// See: [`LexeWallet::create_and_sign_funding_tx`]
///  and [`LexeWallet::preflight_channel_funding_tx`].
const CHANNEL_FUNDING_CONF_PRIO: ConfirmationPriority =
    ConfirmationPriority::Normal;

/// A newtype wrapper around [`Wallet`]. Can be cloned and used directly.
#[derive(Clone)]
pub struct LexeWallet {
    inner: Arc<std::sync::RwLock<Wallet>>,
    fee_estimates: Arc<FeeEstimates>,
    coin_selector: LexeCoinSelector,
    /// NOTE: This is the full, *aggregated* changeset, not an intermediate
    /// state diff, contrary to what the name of "[`ChangeSet`]" might suggest.
    changeset: Arc<std::sync::Mutex<ChangeSet>>,
    wallet_persister_tx: notify::Sender,
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

impl LexeWallet {
    /// Init a [`LexeWallet`] from a [`RootSeed`] and [`ChangeSet`].
    /// Wallet addresses are generated according to the [BIP 84] standard.
    /// See also [BIP 44].
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    #[instrument(skip_all, name = "(wallet-init)")]
    pub async fn init(
        root_seed: &RootSeed,
        network: LxNetwork,
        esplora: &LexeEsplora,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        maybe_changeset: Option<ChangeSet>,
        wallet_persister_tx: notify::Sender,
    ) -> anyhow::Result<Self> {
        let (lexe_wallet, wallet_created) = Self::new(
            root_seed,
            network,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            wallet_persister_tx,
        )?;

        if wallet_created {
            lexe_wallet
                .full_sync(esplora)
                .await
                .context("Failed to conduct initial full sync")?;
        } else {
            lexe_wallet.trigger_persist();
        }

        Ok(lexe_wallet)
    }

    fn new(
        root_seed: &RootSeed,
        network: LxNetwork,
        fee_estimates: Arc<FeeEstimates>,
        coin_selector: LexeCoinSelector,
        maybe_changeset: Option<ChangeSet>,
        wallet_persister_tx: notify::Sender,
    ) -> anyhow::Result<(Self, bool)> {
        let network = network.to_bitcoin();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let internal = Bip84(master_xprv, KeychainKind::Internal);

        // Creates a new wallet.
        let mut wallet_created = false;
        let mut create_wallet = || {
            let create_params =
                CreateParams::new(external.clone(), internal.clone())
                    .network(network)
                    // TODO(max): Wire through specific testnet3/testnet4 hash
                    // .genesis_hash(genesis_hash)
                    .lookahead(BDK_LOOKAHEAD);
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
                // Extract private keys from these descriptors so we can
                // actually sign txs.
                .extract_keys()
                // TODO(max): Might want to check testnet3/testnet4 hash
                // .check_genesis_hash(genesis_hash)
                .check_network(network)
                .lookahead(BDK_LOOKAHEAD);
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

        // Sanity check: BDK wallet should pick up our change/external signers.
        let has_internal_signers =
            !wallet.get_signers(KeychainKind::Internal).ids().is_empty();
        let has_external_signers =
            !wallet.get_signers(KeychainKind::External).ids().is_empty();
        assert!(
            has_internal_signers && has_external_signers,
            "BDK wallet must have at least one External and one Internal signer"
        );

        Ok((
            Self {
                inner: Arc::new(std::sync::RwLock::new(wallet)),
                fee_estimates,
                coin_selector,
                changeset: Arc::new(std::sync::Mutex::new(initial_changeset)),
                wallet_persister_tx,
            },
            wallet_created,
        ))
    }

    /// Constructs a dummy [`LexeWallet`] useful for tests.
    #[cfg(test)]
    pub(crate) fn dummy(
        root_seed: &RootSeed,
        maybe_changeset: Option<ChangeSet>,
    ) -> Self {
        let fee_estimates = FeeEstimates::dummy();
        let coin_selector = LexeCoinSelector::default();
        let network = LxNetwork::Regtest;
        let (persist_tx, _persist_rx) = notify::channel();
        let (wallet, _wallet_created) = LexeWallet::new(
            root_seed,
            network,
            fee_estimates,
            coin_selector,
            maybe_changeset,
            persist_tx,
        )
        .unwrap();

        wallet
    }

    /// Returns a read lock on the inner [`Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    pub fn read(&self) -> RwLockReadGuard<'_, Wallet> {
        self.inner.read().unwrap()
    }

    /// Returns a write lock on the inner [`Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    /// NOTE: You should call [`LexeWallet::trigger_persist`] after you are done
    /// writing to ensure that any changes you make are persisted.
    pub fn write(&self) -> RwLockWriteGuard<'_, Wallet> {
        self.inner.write().unwrap()
    }

    /// Notifies the wallet persister task to persist any changes to the wallet.
    pub fn trigger_persist(&self) {
        self.wallet_persister_tx.send()
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
            .sync(sync_request, BDK_CONCURRENCY)
            .await
            .context("`EsploraAsyncExt::sync` failed")?;
        sync_stats.with_sync_response(&sync_result);

        // Apply the update to the wallet.
        {
            let mut locked_wallet = self.inner.write().unwrap();
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
        let locked_wallet = self.inner.read().unwrap();

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
        }

        impl SpkInfo {
            fn keychain_index(&self) -> (KeychainKind, u32) {
                match self {
                    SpkInfo::External { keychain_index } => *keychain_index,
                    SpkInfo::Internal { keychain_index, .. } => *keychain_index,
                }
            }

            fn needs_sync(&self) -> bool {
                match self {
                    // Sync all external spks
                    SpkInfo::External { .. } => true,
                    // Sync all (1) unused and (2) used internal spks that have
                    // pending spends or have no finalized spends at all.
                    SpkInfo::Internal {
                        num_canonical_spends_to_spk,
                        num_finalized_spends_from_spk,
                        ..
                    } =>
                        num_canonical_spends_to_spk
                            > num_finalized_spends_from_spk
                            || *num_finalized_spends_from_spk == 0,
                }
            }

            fn is_internal(&self) -> bool {
                matches!(self, SpkInfo::Internal { .. })
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
                            confs >= CONFS_TO_FINALIZE
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

        // Collect info about every revealed internal and external spk.
        let mut spk_infos: HashMap<&bitcoin::ScriptBuf, SpkInfo> = keychains
            .inner()
            .all_spks() // all internal and external spks we've ever revealed
            .iter()
            .map(|(&keychain_index, spk)| {
                // Number of canonical outputs that spend to this spk (only used
                // for internal spks)
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
                let spk_info = if keychain_kind == KeychainKind::External {
                    SpkInfo::External { keychain_index }
                } else {
                    SpkInfo::Internal {
                        keychain_index,
                        num_canonical_spends_to_spk,
                        // We need to compute this in the next pass
                        num_finalized_spends_from_spk: 0,
                    }
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
                    // Record another canonical tx that spends from this
                    // internal or external spk.
                    let spk = &txout.script_pubkey;
                    expected_spk_txids.insert((spk, txid));

                    // Only care about finalized txs now
                    if !is_finalized {
                        continue;
                    }

                    // If this finalized tx input spends from an internal spk,
                    // record that
                    if let Some(SpkInfo::Internal {
                        keychain_index: _,
                        num_canonical_spends_to_spk: _,
                        num_finalized_spends_from_spk,
                    }) = spk_infos.get_mut(spk)
                    {
                        *num_finalized_spends_from_spk += 1;
                    }
                }
            }
        }

        // Collect some basic stats on how many spks for each keychain we're
        // going to sync.
        let (syncing_external, syncing_internal) = spk_infos.values().fold(
            (0_u32, 0_u32),
            |(num_ext, num_int), spk_info| {
                if !spk_info.needs_sync() {
                    (num_ext, num_int)
                } else if spk_info.is_internal() {
                    (num_ext, num_int + 1)
                } else {
                    (num_ext + 1, num_int)
                }
            },
        );
        let total_external = keychains
            .last_revealed_index(KeychainKind::External)
            .map(|index| index + 1)
            .unwrap_or(0);
        let total_internal = keychains
            .last_revealed_index(KeychainKind::Internal)
            .map(|index| index + 1)
            .unwrap_or(0);
        let sync_stats = SyncStats {
            is_full_sync: false,
            total_external,
            syncing_external,
            total_internal,
            syncing_internal,
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
    /// wallet descriptors, until a stop gap is hit on both of our keychains.
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
                BDK_FULL_SCAN_STOP_GAP,
                BDK_CONCURRENCY,
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
            let mut locked_wallet = self.inner.write().unwrap();
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
        let locked_wallet = self.inner.read().unwrap();
        locked_wallet
            .start_full_scan_at(synced_at.to_secs())
            .build()
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    pub fn get_balance(&self) -> OnchainBalance {
        let balance = self.inner.read().unwrap().balance();

        // Convert bdk_wallet::Balance to common::ln::balance::Balance.
        // Not using a From impl bc we don't want `common` to depend on BDK.
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
        self.inner.read().unwrap().list_unspent().fold(
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
        let locked_wallet = self.inner.read().unwrap();
        locked_wallet.list_unspent().collect()
    }

    /// Get a [`TxDetails`] for given [`LxTxid`] inside of the wallet.
    /// If not found, returns `None`.
    pub fn get_tx_details(&self, txid: LxTxid) -> Option<TxDetails> {
        let locked_wallet = self.inner.read().unwrap();
        locked_wallet.tx_details(txid.0)
    }

    /// Return the next unused address from the external descriptor. If there
    /// are no unused addresses, this will reveal and return the next one.
    ///
    /// We do this because it prevents a DoS attack where a `get_address` that
    /// returns `reveal_next_address` is called repeatedly. This would reveal
    /// an unbounded number of external spks and make our transaction sync
    /// extremely expensive, as sync requires ~one API call per external spk
    /// ever revealed.
    ///
    /// NOTE: an address is "used" from BDK's perspective if we've broadcasted a
    /// relevant transaction or we've seen a relevant transaction on-chain or
    /// in the mempool.
    ///
    /// NOTE: If a user tries to send two on-chain txs to their wallet in quick
    /// succession, the second call to `get_address` will return the same
    /// address as the first if the wallet has not yet detected the first
    /// transaction. If the user wishes to avoid address reuse, they should wait
    /// for their wallet to sync before sending the second transaction (or
    /// simply avoid this scenario in the first place).
    pub fn get_address(&self) -> bitcoin::Address {
        let address = self
            .write()
            .next_unused_address(KeychainKind::External)
            .address;
        self.trigger_persist();
        address
    }

    /// Return the next unused address from the internal (change) descriptor.
    /// If there are no unused addresses, this will reveal and return the next
    /// one.
    ///
    /// This method should be preferred over `get_address` when the address will
    /// never be exposed to the user in any way, e.g. protocol transactions,
    /// change outputs, etc.... It allows our [`sync`] implementation to avoid
    /// checking the address for updates after it has been finalized, i.e.,
    /// outputs in and inputs out are both balanced and all input spends have at
    /// least `CONFS_TO_FINALIZE` confs.
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

    /// Notifies the BDK wallet that a transaction created by us was
    /// successfully broadcasted and exists in the mempool. This avoids the need
    /// to resync the wallet post-broadcast just to observe the same transaction
    /// that we already know is in the mempool in the mempool.
    ///
    /// NOTE: This function should be called after every successful broadcast,
    /// otherwise BDK may double-spend the outputs spent by this tx, which
    /// usually results in the second tx failing to be broadcasted due to not
    /// meeting RBF requirements.
    ///
    /// TODO(max): If the transaction never gets confirmed, the outputs 'spent'
    /// by this transaction might be locked forever. BDK is working on a fix.
    ///
    /// - Main issue: <https://github.com/bitcoindevkit/bdk/issues/1748>
    /// - Explanation of 'inserted' vs 'unbroadcasted' vs other states: <https://github.com/bitcoindevkit/bdk/issues/1642#issuecomment-2399575535>
    /// - tnull opened an issue which applies to our current approach: <https://github.com/bitcoindevkit/bdk/issues/1666#issue-2621291151>
    pub(crate) fn transaction_broadcasted(&self, tx: Transaction) {
        let now = TimestampMs::now();
        self.transaction_broadcasted_at(now, tx);
    }

    fn transaction_broadcasted_at(
        &self,
        broadcasted_at: TimestampMs,
        tx: Transaction,
    ) {
        let broadcasted_at_secs = broadcasted_at.to_duration().as_secs();
        self.inner
            .write()
            .unwrap()
            .apply_unconfirmed_txs([(tx, broadcasted_at_secs)]);
        self.trigger_persist();
    }

    /// Try to evict an _unconfirmed_ UTXO from BDK's UTXO index. This
    /// effectively tells BDK that an unconfirmed UTXO was evicted from the
    /// mempool.
    #[cfg(test)]
    fn unconfirmed_utxo_evicted_at(
        &self,
        evicted_at: TimestampMs,
        outpoint: LxOutPoint,
    ) -> anyhow::Result<()> {
        let mut locked_wallet = self.inner.write().unwrap();
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
        txid: common::ln::hashes::LxTxid,
    ) {
        let evicted_at_secs = evicted_at.to_duration().as_secs();
        self.inner
            .write()
            .unwrap()
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

        let mut locked_wallet = self.inner.write().unwrap();

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
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
            .drain_to(fake_drain_script());
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
        let mut locked_wallet = self.inner.write().unwrap();

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
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
        network: LxNetwork,
    ) -> anyhow::Result<PaymentWithMetadata<OnchainSendV2>> {
        let (tx, fees) = {
            let mut locked_wallet = self.inner.write().unwrap();

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
            let fee = psbt.fee().context("Bad PSBT fee")?;
            let fee_amount = Amount::try_from_sats_u64(fee.to_sat())
                .context("Bad fee amount")?;

            // Sign tx
            Self::default_sign_psbt(&locked_wallet, &mut psbt)
                .context("Could not sign outbound tx")?;
            let tx = psbt.extract_tx().context("Could not extract tx")?;

            (tx, fee_amount)
        };
        self.trigger_persist();

        Ok(OnchainSendV2::new(tx, req, fees))
    }

    /// Estimate the network fee for a potential onchain send payment. We return
    /// estimates for each [`ConfirmationPriority`] preset.
    ///
    /// This fn deliberately avoids modifying the wallet state. We don't want to
    /// generate unnecessary addresses that we need to watch and sync.
    pub(crate) fn preflight_pay_onchain(
        &self,
        req: PreflightPayOnchainRequest,
        network: LxNetwork,
    ) -> anyhow::Result<PreflightPayOnchainResponse> {
        let high_prio = ConfirmationPriority::High;
        let normal_prio = ConfirmationPriority::Normal;
        let background_prio = ConfirmationPriority::Background;

        let high_feerate = self.fee_estimates.conf_prio_to_feerate(high_prio);
        let normal_feerate =
            self.fee_estimates.conf_prio_to_feerate(normal_prio);
        let background_feerate =
            self.fee_estimates.conf_prio_to_feerate(background_prio);

        let mut locked_wallet = self.inner.write().unwrap();

        // We _require_ a tx to at least be able to use normal fee rate.
        let address = req.address.require_network(network.into())?;
        let normal_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            normal_feerate,
        )?;
        let background_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            background_feerate,
        )?;

        // The high fee rate tx is allowed to fail with insufficient balance.
        let high_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            self.coin_selector,
            &address,
            req.amount,
            high_feerate,
        )
        .ok();

        Ok(PreflightPayOnchainResponse {
            high: high_fee,
            normal: normal_fee,
            background: background_fee,
        })
    }

    fn preflight_pay_onchain_inner(
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
            .drain_to(fake_drain_script());
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
}

/// Spawns a task that (re-)persists the total wallet [`ChangeSet`] whenever
/// it receives a notification (via the `wallet_persister_rx` channel).
pub fn spawn_wallet_persister_task<PS: LexePersister>(
    persister: PS,
    wallet: LexeWallet,
    mut wallet_persister_rx: notify::Receiver,
    // TODO(phlip9): only shutdown persisters after "activity" generating tasks
    // shutdown.
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
    LxTask::spawn("wallet persister", async move {
        loop {
            tokio::select! {
                biased;
                () = wallet_persister_rx.recv() => {
                    do_wallet_persist(&persister, &wallet).await;
                }
                () = shutdown.recv() => break,
            }
        }
        info!("wallet db persister task shutdown");
    })
}

/// Persist the current BDK wallet state if there are any outstanding changes.
async fn do_wallet_persist<PS: LexePersister>(
    persister: &PS,
    wallet: &LexeWallet,
) {
    // Take any staged changes from the wallet and merge them
    // into the combined changeset (i.e. our write-back cache),
    // then serialize + encrypt these to a VFS file.
    let file = {
        let new_changes = match wallet.write().take_staged() {
            Some(c) => c,
            None => {
                debug!("Skipping persist: no new changes");
                return;
            }
        };

        let mut locked_changeset = wallet.changeset.lock().unwrap();
        locked_changeset.merge(new_changes);

        let file_id =
            VfsFileId::new(SINGLETON_DIRECTORY, WALLET_CHANGESET_FILENAME);
        persister.encrypt_json(file_id, &*locked_changeset)
    };

    // Finish the current persist attempt before responding to
    // any shutdown signal received in the meantime.
    let persist_result = persister
        .persist_file(file, IMPORTANT_PERSIST_RETRIES)
        .await;

    match persist_result {
        Ok(()) => debug!("Success: persisted wallet db"),
        Err(e) => warn!("Wallet DB persist error: {e:#}"),
    }
}

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
        // unconfirmed UTXOs should be "less than" confirmed UTXOs.
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

    pub(crate) fn log_sync_complete(&self, elapsed_ms: u128) {
        let num_ext = self.total_external;
        let sync_ext = self.syncing_external;
        let num_int = self.total_internal;
        let sync_int = self.syncing_internal;
        let txs = self.txs;
        let txouts = self.txouts;
        let anchors = self.anchors;
        let seen = self.seen;
        let evicted = self.evicted;
        if self.is_full_sync {
            info!(
                "BDK full sync complete <{elapsed_ms}ms> | \
                 {num_int} int, {num_ext} ext | \
                 resp: txs={txs}, txouts={txouts}, anchors={anchors}, \
                 seen={seen}, evicted={evicted}",
            );
        } else {
            info!(
                "BDK sync complete <{elapsed_ms}ms> | \
                 req: {sync_int}/{num_int} int, {sync_ext}/{num_ext} ext | \
                 resp: txs={txs}, txouts={txouts}, anchors={anchors}, \
                 seen={seen}, evicted={evicted}",
            );
        }
    }
}

/// Use this fake TXO drain script to prevent the BDK wallet from modifying its
/// internal state when building fake txs, like the ones used to estimate fees
/// during preflight.
///
/// Returns a 22-byte script: [
///     OP_0
///     OP_PUSHBYTES_20
///     aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
/// ]
fn fake_drain_script() -> bitcoin::ScriptBuf {
    bitcoin::ScriptBuf::from_bytes(vec![0xaa; 22])
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
    use common::{root_seed::RootSeed, test_utils::arbitrary};
    use proptest::{
        arbitrary::any,
        option,
        strategy::{Just, Strategy},
    };

    use super::*;

    type KeychainChangeset = keychain_txout::ChangeSet;
    type TxGraphChangeset = tx_graph::ChangeSet<ConfirmationBlockTime>;

    pub(super) fn any_changeset() -> impl Strategy<Value = ChangeSet> {
        let network = bitcoin::Network::Bitcoin;
        let seed = RootSeed::from_u64(20241114);
        let master_xprv = seed.derive_bip32_master_xprv(network);
        let just_descriptor = Just({
            let (descriptor, _, _) = Bip84(master_xprv, KeychainKind::External)
                .build(network)
                .unwrap();
            descriptor
        });
        let just_change_descriptor = Just({
            let (descriptor, _, _) = Bip84(master_xprv, KeychainKind::Internal)
                .build(network)
                .unwrap();
            descriptor
        });

        (
            option::of(just_descriptor),
            option::of(just_change_descriptor),
            option::of(any::<LxNetwork>().prop_map(Into::into)),
            any_localchain_changeset(),
            any_txgraph_changeset(),
            any_keychain_changeset(),
        )
            .prop_map(
                |(
                    descriptor,
                    change_descriptor,
                    network,
                    local_chain,
                    tx_graph,
                    indexer,
                )| {
                    ChangeSet {
                        descriptor,
                        change_descriptor,
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

    use bdk_chain::{BlockId, ConfirmationBlockTime};
    use bdk_wallet::{
        AddressInfo,
        KeychainKind::{External, Internal},
    };
    use bitcoin::{TxOut, Txid, hashes::Hash as _};
    use common::{
        ln::hashes::LxTxid,
        rng::FastRng,
        sat,
        test_utils::{arbitrary, roundtrip},
    };
    use lexe_api::types::payments::ClientPaymentId;
    use proptest::test_runner::Config;
    use tracing::trace;

    use super::*;

    struct Harness {
        wallet: LexeWallet,
        network: LxNetwork,
        root_seed: RootSeed,
    }

    impl Harness {
        fn new(seed: u64) -> Self {
            let root_seed = RootSeed::from_u64(seed);
            let maybe_changeset = None;
            let wallet = LexeWallet::dummy(&root_seed, maybe_changeset);
            let network = LxNetwork::Regtest;

            // Add some initial confirmed blocks
            {
                let mut w = wallet.write();
                w.add_checkpoint(100);
                w.add_checkpoint(900);
            }

            let h = Harness {
                wallet,
                network,
                root_seed,
            };
            h.persist();
            h
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
                cid: ClientPaymentId([42; 32]),
                address,
                amount,
                priority: ConfirmationPriority::Normal,
                note: None,
            };
            let oswm = self
                .wallet
                .create_onchain_send(send_req, self.network)
                .expect("Failed to create onchain send");
            self.wallet.transaction_broadcasted_at(
                self.now(),
                oswm.payment.tx.as_ref().clone(),
            );
            oswm
        }

        /// Assert that building an incremental sync request on the current
        /// wallet state returns the expected set of spks and their associated
        /// expected canonical txids.
        #[track_caller]
        fn assert_sync(
            &self,
            mut expected: BTreeMap<bitcoin::ScriptBuf, BTreeSet<Txid>>,
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

            let wallet = LexeWallet::dummy(&self.root_seed, Some(changeset));
            assert!(wallet.read().have_unused_spk(KeychainKind::External));
            assert!(wallet.read().have_unused_spk(KeychainKind::Internal));
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
        fn confirm_txids(&mut self, confs: u32, txids: &[Txid]);

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
            self.confirm_txids(CONFS_TO_FINALIZE, &[tx.compute_txid()]);
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

        fn confirm_txids(&mut self, confs: u32, txids: &[Txid]) {
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
    fn drain_script_len_equiv() {
        let h = Harness::new(3684944666541);
        let address = h.wallet.get_internal_address();

        let spk = address.script_pubkey();
        assert_eq!(
            spk.len(),
            fake_drain_script().len(),
            "Drain script ({spk:?}) has unexpected length"
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

        // preflight_open_channel
        h.assert_no_persists_in(|h| {
            let fee =
                h.wallet.preflight_channel_funding_tx(sat!(12_345)).unwrap();
            assert_eq!(fee.sats_u64(), 305);
        });

        // preflight_pay_onchain
        let address = {
            let network = h.network.to_bitcoin();
            let script = Script::from_bytes(&[0x42; 10]);
            Address::p2wsh(script, network).into_unchecked()
        };
        h.assert_no_persists_in(|h| {
            let req = PreflightPayOnchainRequest {
                address: address.clone(),
                amount: sat!(12_345),
            };
            let fee = h.wallet.preflight_pay_onchain(req, h.network).unwrap();
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

        // preflight_open_channel
        h.assert_no_persists_in(|h| {
            let amount = sat!(12_345);
            let fee = h.wallet.preflight_channel_funding_tx(amount).unwrap();
            assert_eq!(fee.sats_u64(), 441);
        });

        // preflight_pay_onchain
        h.assert_no_persists_in(|h| {
            let req = PreflightPayOnchainRequest {
                address: address.clone(),
                amount: sat!(12_345),
            };
            let fee = h.wallet.preflight_pay_onchain(req, h.network).unwrap();
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
        let address = bitcoin::Address::from_str(
            "bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk",
        )
        .unwrap();
        let req = PayOnchainRequest {
            cid: ClientPaymentId([42; 32]),
            address,
            amount: sat!(9_000),
            priority: ConfirmationPriority::Normal,
            note: None,
        };
        let oswm = h.wallet.create_onchain_send(req, h.network).unwrap();
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
        let outpoint = LxOutPoint {
            txid: LxTxid(tx_u.compute_txid()),
            index: 0,
        };
        h.wallet.unconfirmed_utxo_evicted_at(now, outpoint).unwrap();

        // We can no longer spend it
        h.assert_spend_err(10_000);

        // We can still spend the confirmed UTXO
        h.assert_spend_ok(4_000);

        // We can't evict a confirmed UTXO
        let outpoint = LxOutPoint {
            txid: LxTxid(tx_c.compute_txid()),
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
        let address = bitcoin::Address::from_str(
            "bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk",
        )
        .unwrap();
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

        h.wallet.transaction_broadcasted_at(
            h.now(),
            oswm.payment.tx.as_ref().clone(),
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
        let address3p = bitcoin::Address::from_str(
            "bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk",
        )
        .unwrap();
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
        h.wallet.transaction_broadcasted_at(h.now(), tx);
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
        h.wallet.transaction_broadcasted_at(h.now(), txi1_self);
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
        h.wallet.transaction_broadcasted_at(h.now(), tx2);
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

    // Snapshot taken 2024-11-14 @ bdk-v1.0.0-beta.5
    const CHANGESET_SNAPSHOT_1_0_0_BETA_5: &str =
        include_str!("../data/changeset-snapshot.v1.0.0-beta.5.json");

    // Snapshot taken 2025-03-16 @ bdk-v1.1.0
    const CHANGESET_SNAPSHOT_1_1_0: &str =
        include_str!("../data/changeset-snapshot.v1.1.0.json");

    #[test]
    fn test_changeset_snapshots() {
        serde_json::from_str::<Vec<ChangeSet>>(CHANGESET_SNAPSHOT_1_0_0_BETA_5)
            .unwrap();
        serde_json::from_str::<Vec<ChangeSet>>(CHANGESET_SNAPSHOT_1_1_0)
            .unwrap();
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
    /// $ cargo test -p lexe-ln --lib -- --ignored dump_changesets --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_changesets() {
        let mut rng = FastRng::from_u64(20250316);
        let strategy = arbitrary_impl::any_changeset();
        let changesets = arbitrary::gen_value_iter(&mut rng, strategy)
            .take(3)
            .collect::<Vec<ChangeSet>>();
        println!("---");
        println!("{}", serde_json::to_string_pretty(&changesets).unwrap());
        println!("---");
    }
}
