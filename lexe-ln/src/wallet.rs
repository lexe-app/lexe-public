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
    coin_selection::{
        CoinSelectionAlgorithm, CoinSelectionResult, InsufficientFunds,
    },
    template::Bip84,
    CreateParams, KeychainKind, LoadParams, SignOptions, TxBuilder, Wallet,
    WeightedUtxo,
};
use bitcoin::{Psbt, Transaction};
use common::{
    constants::IMPORTANT_PERSIST_RETRIES,
    ln::{
        amount::Amount, balance::OnchainBalance, network::LxNetwork,
        priority::ConfirmationPriority,
    },
    root_seed::RootSeed,
    time::TimestampMs,
};
use lexe_api::{
    models::command::{
        FeeEstimate, PayOnchainRequest, PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
    },
    vfs::{Vfs, VfsFileId, SINGLETON_DIRECTORY, WALLET_CHANGESET_FILENAME},
};
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
use rand::RngCore;
use tracing::{debug, info, instrument, warn};

use crate::{
    esplora::{FeeEstimates, LexeEsplora},
    payments::onchain::OnchainSend,
    traits::LexePersister,
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
    pub(crate) fn dummy(root_seed: &RootSeed) -> Self {
        let fee_estimates = FeeEstimates::dummy();
        let coin_selector = LexeCoinSelector::default();
        let network = LxNetwork::Regtest;
        let maybe_changeset = None;
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
    pub async fn sync(&self, esplora: &LexeEsplora) -> anyhow::Result<()> {
        // Build a SyncRequest with everything we're interested in syncing.
        let sync_request = self.build_sync_request();

        // Check for updates on everything we specified in the SyncRequest.
        let sync_result = esplora
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

    /// Collect all the script pubkeys, UTXOs, and unconfirmed txids that we
    /// want to sync from the esplora backend.
    fn build_sync_request(&self) -> SyncRequest<u32> {
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
        let utxos = locked_wallet.list_unspent().map(|utxo| utxo.outpoint);

        // The txids of txns we want to check if they have been spent.
        let unconfirmed_txids = tx_graph
            .list_canonical_txs(local_chain, chain_tip.block_id())
            .filter(|canonical_tx| !canonical_tx.chain_position.is_confirmed())
            .map(|canonical_tx| canonical_tx.tx_node.txid);

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
    }

    /// Conducts a full sync of all script pubkeys derived from all of our
    /// wallet descriptors, until a stop gap is hit on both of our keychains.
    ///
    /// This should be done rarely, i.e. only when creating the wallet or if we
    /// need to restore from a existing seed. See BDK's examples for more info.
    async fn full_sync(&self, esplora: &LexeEsplora) -> anyhow::Result<()> {
        let full_scan_request = {
            let locked_wallet = self.inner.read().unwrap();
            locked_wallet.start_full_scan()
        };

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
        let timestamp_secs = now.to_duration().as_secs();
        self.inner
            .write()
            .unwrap()
            .apply_unconfirmed_txs([(tx, timestamp_secs)]);
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
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
    LxTask::spawn("wallet persister", async move {
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
    use std::{
        fs,
        path::Path,
        process::{Command, Stdio},
    };

    use bdk_chain::{BlockId, ConfirmationBlockTime};
    use bitcoin::{TxOut, Txid};
    use bitcoin_hashes::Hash;
    use common::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::test_runner::Config;

    use super::*;

    struct Harness {
        wallet: LexeWallet,
        network: LxNetwork,
    }

    impl Harness {
        fn new() -> Self {
            let root_seed = RootSeed::from_u64(923409802);
            let wallet = LexeWallet::dummy(&root_seed);
            let network = LxNetwork::Regtest;

            Harness { wallet, network }
        }

        fn fund(&mut self, amount: Amount) {
            let address = self.wallet.get_address();
            let mut wallet = self.wallet.write();

            // "confirm" some random blocks
            wallet.add_checkpoint(100);
            wallet.add_checkpoint(900);

            // build tx and fake anchor to confirm tx
            let tx = Transaction {
                output: vec![TxOut {
                    value: amount.into(),
                    script_pubkey: address.script_pubkey(),
                }],
                ..new_tx()
            };

            // add and confirm the tx
            wallet.add_confirmed_tx(&tx);

            // "persist"
            let _ = wallet.take_staged();
        }

        fn check_no_persists_in<F, R>(&mut self, f: F) -> R
        where
            F: FnOnce(&mut Self) -> R,
        {
            let _ = self.wallet.write().take_staged();
            let ret = f(self);
            assert_eq!(None, self.wallet.write().take_staged());
            ret
        }
    }

    trait WalletExt {
        fn height(&self) -> u32;
        fn add_checkpoint(&mut self, blocks: u32) -> ConfirmationBlockTime;
        fn add_unconfirmed_tx(&mut self, tx: &Transaction);
        fn add_confirmed_tx(&mut self, tx: &Transaction);
        fn confirm_txids(&mut self, txids: &[Txid]);
    }

    impl WalletExt for Wallet {
        fn height(&self) -> u32 {
            self.local_chain().tip().height()
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
            let tx = Arc::new(tx.clone());
            self.apply_update(bdk_wallet::Update {
                tx_update: bdk_chain::TxUpdate {
                    txs: vec![tx.clone()],
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        }

        fn add_confirmed_tx(&mut self, tx: &Transaction) {
            self.add_unconfirmed_tx(tx);
            self.confirm_txids(&[tx.compute_txid()]);
        }

        fn confirm_txids(&mut self, txids: &[Txid]) {
            let anchor = self.add_checkpoint(6);
            let anchors = txids.iter().map(|txid| (anchor, *txid)).collect();
            self.apply_update(bdk_wallet::Update {
                tx_update: bdk_chain::tx_graph::TxUpdate {
                    anchors,
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
        }
    }

    fn new_tx() -> Transaction {
        Transaction {
            version: bitcoin::transaction::Version::ONE,
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

    #[test]
    fn drain_script_len_equiv() {
        let h = Harness::new();
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
        let mut h = Harness::new();
        h.fund(Amount::from_sats_u32(123_456));

        // preflight_open_channel
        h.check_no_persists_in(|h| {
            let fee = h
                .wallet
                .preflight_channel_funding_tx(Amount::from_sats_u32(12_345))
                .unwrap();
            assert_eq!(fee.sats_u64(), 305);
        });

        // preflight_pay_onchain
        let address = {
            let network = h.network.to_bitcoin();
            let script = Script::from_bytes(&[0x42; 10]);
            Address::p2wsh(script, network).as_unchecked().clone()
        };
        h.check_no_persists_in(|h| {
            let req = PreflightPayOnchainRequest {
                address: address.clone(),
                amount: Amount::from_sats_u32(12_345),
            };
            let fee = h.wallet.preflight_pay_onchain(req, h.network).unwrap();
            assert_eq!(fee.high.map(|x| x.amount.sats_u64()), Some(382));
            assert_eq!(fee.normal.amount.sats_u64(), 305);
            assert_eq!(fee.background.amount.sats_u64(), 185);
        });

        // use a fresh wallet, as coin selection is apparently
        // non-deterministic :')
        // with some smaller utxos so we get multiple inputs to the funding tx
        let mut h = Harness::new();
        h.fund(Amount::from_sats_u32(11_500));
        h.fund(Amount::from_sats_u32(11_500));

        // preflight_open_channel
        h.check_no_persists_in(|h| {
            let amount = Amount::from_sats_u32(12_345);
            let fee = h.wallet.preflight_channel_funding_tx(amount).unwrap();
            assert_eq!(fee.sats_u64(), 441);
        });

        // preflight_pay_onchain
        h.check_no_persists_in(|h| {
            let req = PreflightPayOnchainRequest {
                address: address.clone(),
                amount: Amount::from_sats_u32(12_345),
            };
            let fee = h.wallet.preflight_pay_onchain(req, h.network).unwrap();
            assert_eq!(fee.high.map(|x| x.amount.sats_u64()), Some(552));
            assert_eq!(fee.normal.amount.sats_u64(), 441);
            assert_eq!(fee.background.amount.sats_u64(), 267);
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
