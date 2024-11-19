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
    ops::DerefMut,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{ensure, Context};
use bdk_chain::{spk_client::SyncRequest, Merge};
use bdk_esplora::EsploraAsyncExt;
pub use bdk_wallet::ChangeSet;
use bdk_wallet::{
    coin_selection::DefaultCoinSelectionAlgorithm, template::Bip84,
    CreateParams, KeychainKind, LoadParams, SignOptions, TxBuilder, Wallet,
};
use bitcoin::{Psbt, Transaction};
use common::{
    api::{
        command::{
            FeeEstimate, PayOnchainRequest, PreflightPayOnchainRequest,
            PreflightPayOnchainResponse,
        },
        vfs::{Vfs, VfsFileId},
    },
    constants::{
        IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY,
        WALLET_CHANGESET_FILENAME,
    },
    ln::{
        amount::Amount, balance::OnchainBalance, network::LxNetwork,
        priority::ConfirmationPriority,
    },
    notify,
    root_seed::RootSeed,
    shutdown::ShutdownChannel,
    task::LxTask,
};
use tracing::{debug, info, instrument, warn};

use crate::{
    esplora::LexeEsplora, payments::onchain::OnchainSend, traits::LexePersister,
};

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

/// The length of our drain script outputs, in bytes. Used for estimating fees.
///
/// Example: [
///     OP_0
///     OP_PUSHBYTES_20
///     9b77ada43d0f43f14ee4d15980511bb3777607e8
/// ]
#[allow(dead_code)] // TODO(phlip9): remove
const DRAIN_SCRIPT_LEN: usize = 22;

/// A newtype wrapper around [`Wallet`]. Can be cloned and used directly.
#[derive(Clone)]
pub struct LexeWallet {
    esplora: Arc<LexeEsplora>,
    inner: Arc<std::sync::RwLock<Wallet>>,
    /// NOTE: This is the full, *aggregated* changeset, not an intermediate
    /// state diff, contrary to what the name of "[`ChangeSet`]" might suggest.
    changeset: Arc<std::sync::Mutex<ChangeSet>>,
    wallet_persister_tx: notify::Sender,
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
        esplora: Arc<LexeEsplora>,
        maybe_changeset: Option<ChangeSet>,
        wallet_persister_tx: notify::Sender,
    ) -> anyhow::Result<Self> {
        let (lexe_wallet, wallet_created) = Self::new(
            root_seed,
            network,
            esplora,
            maybe_changeset,
            wallet_persister_tx,
        )?;

        if wallet_created {
            lexe_wallet
                .full_sync()
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
        esplora: Arc<LexeEsplora>,
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
                Some(w) => w,
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

        Ok((
            Self {
                esplora,
                inner: Arc::new(std::sync::RwLock::new(wallet)),
                changeset: Arc::new(std::sync::Mutex::new(initial_changeset)),
                wallet_persister_tx,
            },
            wallet_created,
        ))
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
    pub async fn sync(&self) -> anyhow::Result<()> {
        // Build a SyncRequest with everything we're interested in syncing.
        let sync_request = {
            let locked_wallet = self.inner.read().unwrap();

            let keychains = locked_wallet.spk_index();
            let tx_graph = locked_wallet.tx_graph();
            let local_chain = locked_wallet.local_chain();
            let chain_tip = local_chain.tip();

            // Sync all external script pubkeys we have ever revealed.
            let revealed_external_spks =
                keychains.revealed_keychain_spks(KeychainKind::External);

            // Sync all internal (change) spks we've revealed but have not used.
            // We save some calls here by skipping all spks we've already used.
            let unused_internal_spks =
                keychains.unused_keychain_spks(KeychainKind::Internal);

            // Sync the last used internal (change) spk, in case two txs in
            // quick succession caused us to reuse the previous internal spk.
            let last_used_internal_spk = keychains
                .last_used_index(KeychainKind::Internal)
                .and_then(|idx| {
                    let spk =
                        keychains.spk_at_index(KeychainKind::Internal, idx)?;
                    Some((idx, spk))
                });

            // Sync the next (unrevealed) spk for both keychains, in case we
            // revealed an index, used it, then crashed before it was persisted.
            let next_external_spk = keychains
                .next_index(KeychainKind::External)
                .and_then(|(idx, _is_new)| {
                    let spk =
                        keychains.spk_at_index(KeychainKind::External, idx)?;
                    Some((idx, spk))
                });
            let next_internal_spk = keychains
                .next_index(KeychainKind::Internal)
                .and_then(|(idx, _is_new)| {
                    let spk =
                        keychains.spk_at_index(KeychainKind::Internal, idx)?;
                    Some((idx, spk))
                });

            // The UTXOs (outpoints) we check to see if they have been spent.
            let utxos = locked_wallet
                .list_unspent()
                .map(|utxo| utxo.outpoint)
                .collect::<Vec<bitcoin::OutPoint>>();

            // The txids of txns we want to check if they have been spent.
            let unconfirmed_txids = tx_graph
                .list_canonical_txs(local_chain, chain_tip.block_id())
                .filter(|canonical_tx| {
                    !canonical_tx.chain_position.is_confirmed()
                })
                .map(|canonical_tx| canonical_tx.tx_node.txid)
                .collect::<Vec<bitcoin::Txid>>();

            // Specify all of the above in our SyncRequest.
            SyncRequest::builder()
                .chain_tip(chain_tip)
                .spks_with_indexes(revealed_external_spks)
                .spks_with_indexes(unused_internal_spks)
                .spks_with_indexes(last_used_internal_spk)
                .spks_with_indexes(next_external_spk)
                .spks_with_indexes(next_internal_spk)
                .outpoints(utxos)
                .txids(unconfirmed_txids)
                .build()
        };

        // Check for updates on everything we specified in the SyncRequest.
        let sync_result = self
            .esplora
            .client()
            .sync(sync_request, BDK_CONCURRENCY)
            .await
            .context("`EsploraAsyncExt::sync` failed")?;

        // Apply the update to the wallet.
        {
            let mut locked_wallet = self.inner.write().unwrap();
            locked_wallet
                .apply_update(sync_result)
                .context("Could not apply sync update to wallet")?;
        }
        self.trigger_persist();

        Ok(())
    }

    /// Conducts a full sync of all script pubkeys derived from all of our
    /// wallet descriptors, until a stop gap is hit on both of our keychains.
    ///
    /// This should be done rarely, i.e. only when creating the wallet or if we
    /// need to restore from a existing seed. See BDK's examples for more info.
    async fn full_sync(&self) -> anyhow::Result<()> {
        let full_scan_request = {
            let locked_wallet = self.inner.read().unwrap();
            locked_wallet.start_full_scan()
        };

        // Scan the blockchain for our keychain script pubkeys until we hit the
        // `stop_gap`.
        let full_scan_result = self
            .esplora
            .client()
            .full_scan::<KeychainKind, _>(
                full_scan_request,
                BDK_FULL_SCAN_STOP_GAP,
                BDK_CONCURRENCY,
            )
            .await
            .context("EsploraAsyncExt::full_scan failed")?;

        // Apply the combined update to the wallet.
        {
            let mut locked_wallet = self.inner.write().unwrap();
            locked_wallet
                .apply_update(full_scan_result)
                .context("Could not apply full scan result to wallet")?;
        }

        self.trigger_persist();

        Ok(())
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

    /// Returns the last unused address derived using the external descriptor.
    ///
    /// We employ this address index selection strategy because it prevents a
    /// DoS attack where `get_address` is called repeatedly, making transaction
    /// sync (which generally requires one API call per watched address)
    /// extremely expensive.
    ///
    /// NOTE: If a user tries to send two on-chain txs to their wallet in quick
    /// succession, the second call to `get_address` will return the same
    /// address as the first if the wallet has not yet detected the first
    /// transaction. If the user wishes to avoid address reuse, they should wait
    /// for their wallet to sync before sending the second transaction (or
    /// simply avoid this scenario in the first place).
    pub fn get_address(&self) -> bitcoin::Address {
        let address = self
            .inner
            .write()
            .unwrap()
            .next_unused_address(KeychainKind::External)
            .address;
        self.trigger_persist();
        address
    }

    /// Returns the last unused address from the internal (change) descriptor.
    ///
    /// This method should be preferred over `get_address` when the address will
    /// never be exposed to the user in any way, e.g. internal transactions,
    /// as it allows our [`sync`] implementation to avoid checking the address
    /// for updates after it has been used.
    ///
    /// NOTE: If a user somehow sees this address and sends funds to it, their
    /// funds will not show up in the wallet, because it won't be synced!
    ///
    /// [`sync`]: Self::sync
    pub fn get_internal_address(&self) -> bitcoin::Address {
        let address = self
            .inner
            .write()
            .unwrap()
            .next_unused_address(KeychainKind::Internal)
            .address;
        self.trigger_persist();
        address
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
        //   1f81a37547d600618b57ffd57d36144158060961a4b22076f365fd3fb1b4c1f0
        // ]
        // => len == 34 bytes
        let fake_output_script = bitcoin::ScriptBuf::from_bytes(vec![0x69; 34]);

        let mut locked_wallet = self.inner.write().unwrap();

        // Get status of wallet prior to preflight
        #[cfg(debug_assertions)]
        let (next_unused_idx1, had_staged_changes) = {
            let idx = locked_wallet
                .next_unused_address(KeychainKind::Internal)
                .index;
            let staged = locked_wallet.staged().is_some();
            (idx, staged)
        };

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
        let feerate = self.esplora.conf_prio_to_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&mut locked_wallet, feerate);
        tx_builder.add_recipient(fake_output_script, channel_value.into());
        let psbt = tx_builder
            // This possibly 'uses' a change address.
            .finish()
            .context("Could not build channel funding tx")?;
        // This unmarks the change address that was just 'used'.
        locked_wallet.cancel_tx(&psbt.unsigned_tx);

        // Check that we didn't increment the last used change index,
        // and didn't stage any changes if nothing was staged before.
        #[cfg(debug_assertions)]
        {
            let next_unused_idx2 = locked_wallet
                .next_unused_address(KeychainKind::Internal)
                .index;
            assert_eq!(
                next_unused_idx1, next_unused_idx2,
                "Preflight funding tx incremented the next unused change index"
            );
            if !had_staged_changes {
                assert!(
                    locked_wallet.staged().is_none(),
                    "Preflight funding tx created staged changes"
                );
            }
        }

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
        let feerate = self.esplora.conf_prio_to_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&mut locked_wallet, feerate);
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
    /// address, packaging up all of this info in a new [`OnchainSend`].
    pub(crate) fn create_onchain_send(
        &self,
        req: PayOnchainRequest,
        network: LxNetwork,
    ) -> anyhow::Result<OnchainSend> {
        let (tx, fees) = {
            let mut locked_wallet = self.inner.write().unwrap();

            let address = req
                .address
                .clone()
                .require_network(network.into())
                .context("Invalid network")?;

            // Build unsigned tx
            let feerate = self.esplora.conf_prio_to_feerate(req.priority);
            let mut tx_builder =
                Self::default_tx_builder(&mut locked_wallet, feerate);
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

        Ok(OnchainSend::new(tx, req, fees))
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

        let high_feerate = self.esplora.conf_prio_to_feerate(high_prio);
        let normal_feerate = self.esplora.conf_prio_to_feerate(normal_prio);
        let background_feerate =
            self.esplora.conf_prio_to_feerate(background_prio);

        let mut locked_wallet = self.inner.write().unwrap();

        // Get status of wallet prior to preflight
        #[cfg(debug_assertions)]
        let (next_unused_idx1, had_staged_changes) = {
            let idx = locked_wallet
                .next_unused_address(KeychainKind::Internal)
                .index;
            let staged = locked_wallet.staged().is_some();
            (idx, staged)
        };

        // We _require_ a tx to at least be able to use normal fee rate.
        let address = req.address.require_network(network.into())?;
        let normal_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            &address,
            req.amount,
            normal_feerate,
        )?;
        let background_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            &address,
            req.amount,
            background_feerate,
        )?;

        // The high fee rate tx is allowed to fail with insufficient balance.
        let high_fee = Self::preflight_pay_onchain_inner(
            locked_wallet.deref_mut(),
            &address,
            req.amount,
            high_feerate,
        )
        .ok();

        // Check that we didn't increment the last used change index,
        // and didn't stage any changes if nothing was staged before.
        #[cfg(debug_assertions)]
        {
            let next_unused_idx2 = locked_wallet
                .next_unused_address(KeychainKind::Internal)
                .index;
            assert_eq!(
                next_unused_idx1, next_unused_idx2,
                "Preflight funding tx incremented the next unused change index"
            );
            if !had_staged_changes {
                assert!(
                    locked_wallet.staged().is_none(),
                    "Preflight funding tx created staged changes"
                );
            }
        }

        Ok(PreflightPayOnchainResponse {
            high: high_fee,
            normal: normal_fee,
            background: background_fee,
        })
    }

    fn preflight_pay_onchain_inner(
        wallet: &mut Wallet,
        address: &bitcoin::Address,
        amount: Amount,
        feerate: bitcoin::FeeRate,
    ) -> anyhow::Result<FeeEstimate> {
        // We're just estimating the fee for tx; we don't want to unnecessarily
        // reveal any addresses, which will take up sync time. `peek_address`
        // will just derive the output at the index without persisting anything.
        let change_address = wallet.peek_address(KeychainKind::Internal, 0);
        let mut tx_builder = Self::default_tx_builder(wallet, feerate);
        tx_builder.add_recipient(address.script_pubkey(), amount.into());
        tx_builder.drain_to(change_address.script_pubkey());
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
        feerate: bitcoin::FeeRate,
    ) -> TxBuilder<'_, DefaultCoinSelectionAlgorithm> {
        // Set the feerate. RBF is already enabled by default.
        let mut tx_builder = wallet.build_tx();
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
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("wallet persister", async move {
        loop {
            tokio::select! {
                () = wallet_persister_rx.recv() => {
                    // Take any staged changes from the wallet and merge them
                    // into the combined changeset (i.e. our write-back cache),
                    // then serialize + encrypt these to a VFS file.
                    let file = {
                        let mut locked_wallet = wallet.inner.write().unwrap();
                        let new_changes = match locked_wallet.take_staged() {
                            Some(c) => c,
                            None => {
                                debug!("Skipping persist: no new changes");
                                continue;
                            }
                        };
                        let mut locked_changeset =
                            wallet.changeset.lock().unwrap();
                        locked_changeset.merge(new_changes);

                        let file_id = VfsFileId::new(
                            SINGLETON_DIRECTORY, WALLET_CHANGESET_FILENAME
                        );
                        persister.encrypt_json(file_id, &*locked_changeset)
                    };

                    // Finish the current persist attempt before responding to
                    // any shutdown signal received in the meantime.
                    let persist_result = persister
                        .persist_file(&file, IMPORTANT_PERSIST_RETRIES)
                        .await;

                    match persist_result {
                        Ok(()) => debug!("Success: persisted wallet db"),
                        Err(e) => warn!("Wallet DB persist error: {e:#}"),
                    }
                }
                () = shutdown.recv() => break,
            }
        }

        info!("wallet db persister task shutting down");
    })
}

#[cfg(test)]
mod arbitrary_impl {
    use std::sync::Arc;

    use bdk_chain::{
        keychain_txout, local_chain, tx_graph, ConfirmationBlockTime,
        DescriptorId,
    };
    use bdk_wallet::{template::DescriptorTemplate, ChangeSet, KeychainKind};
    use bitcoin_hashes::Hash;
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
        let last_seen = proptest::collection::btree_map(
            arbitrary::any_txid(),
            any::<u64>(),
            0..4,
        );

        (any_txs, any_txouts, anchors, last_seen).prop_map(
            |(txs, txouts, anchors, last_seen)| TxGraphChangeset {
                txs,
                txouts,
                anchors,
                last_seen,
            },
        )
    }

    fn any_keychain_changeset() -> impl Strategy<Value = KeychainChangeset> {
        let any_descriptor_id = any::<[u8; 32]>()
            .prop_map(bitcoin::hashes::sha256::Hash::from_byte_array)
            .prop_map(DescriptorId);

        proptest::collection::btree_map(any_descriptor_id, any::<u32>(), 0..4)
            .prop_map(|last_revealed| KeychainChangeset { last_revealed })
    }

    fn any_localchain_changeset(
    ) -> impl Strategy<Value = local_chain::ChangeSet> {
        proptest::collection::btree_map(
            any::<u32>(),
            option::of(arbitrary::any_blockhash()),
            0..4,
        )
        .prop_map(|blocks| local_chain::ChangeSet { blocks })
    }

    fn any_confirmationblocktime(
    ) -> impl Strategy<Value = ConfirmationBlockTime> {
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
    use std::collections::BTreeMap;

    use arc_swap::ArcSwap;
    use common::{
        rng::WeakRng,
        test_utils::{arbitrary, roundtrip},
    };
    use esplora_client::AsyncClient;
    use proptest::test_runner::Config;

    use super::*;

    // Snapshot taken 2024-11-14
    const CHANGESET_SNAPSHOT: &str =
        include_str!("../data/changeset-snapshot.json");

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
    fn test_changeset_snapshot() {
        serde_json::from_str::<Vec<ChangeSet>>(CHANGESET_SNAPSHOT).unwrap();
    }

    /// Dumps a JSON array of three `ChangeSet`s using the proptest strategy.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln -- --ignored dump_changesets --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_changesets() {
        let mut rng = WeakRng::from_u64(20241030);
        let strategy = arbitrary_impl::any_changeset();
        let changesets = arbitrary::gen_value_iter(&mut rng, strategy)
            .take(3)
            .collect::<Vec<ChangeSet>>();
        println!("---");
        println!("{}", serde_json::to_string_pretty(&changesets).unwrap());
        println!("---");
    }

    fn make_wallet() -> LexeWallet {
        let root_seed = RootSeed::from_u64(923409802);

        let client = reqwest11::ClientBuilder::new().build().unwrap();
        let client = AsyncClient::from_client("dummy".to_owned(), client);
        let fee_estimates = ArcSwap::from_pointee(BTreeMap::new());
        let (test_tx, _test_rx) = crate::test_event::channel("test");
        let esplora =
            Arc::new(LexeEsplora::new(client, fee_estimates, test_tx));

        let maybe_changeset = None;
        let (persist_tx, _persist_rx) = notify::channel();
        let (lexe_wallet, _wallet_created) = LexeWallet::new(
            &root_seed,
            LxNetwork::Regtest,
            esplora,
            maybe_changeset,
            persist_tx,
        )
        .unwrap();

        lexe_wallet
    }

    #[test]
    fn test_drain_script_len_equiv() {
        let wallet = make_wallet();
        let address = wallet.get_internal_address();

        let spk = address.script_pubkey();
        assert_eq!(
            spk.len(),
            DRAIN_SCRIPT_LEN,
            "Drain script ({spk:?}) has unexpected length"
        );
    }
}
