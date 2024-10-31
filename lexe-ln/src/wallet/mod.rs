use std::{
    ops::DerefMut,
    sync::{Arc, RwLockReadGuard, RwLockWriteGuard},
};

use anyhow::{ensure, Context};
use bdk::{
    template::Bip84,
    wallet::{
        coin_selection::DefaultCoinSelectionAlgorithm, tx_builder::CreateTx,
        AddressIndex, Update,
    },
    KeychainKind, SignOptions, TxBuilder,
};
use bdk_chain::Append;
use bdk_esplora::EsploraAsyncExt;
use bitcoin::{psbt::PartiallySignedTransaction, Transaction};
use common::{
    api::command::{
        FeeEstimate, PayOnchainRequest, PreflightPayOnchainRequest,
        PreflightPayOnchainResponse,
    },
    constants::{
        IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
    },
    ln::{
        amount::Amount, balance::Balance, network::LxNetwork,
        priority::ConfirmationPriority,
    },
    notify,
    root_seed::RootSeed,
    shutdown::ShutdownChannel,
    task::LxTask,
};
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use self::{db::WalletDb, db29::WalletDb29};
use crate::{
    esplora::LexeEsplora,
    payments::onchain::OnchainSend,
    traits::{LexeInnerPersister, LexePersister},
};

/// Wallet DB.
pub mod db;
/// The old wallet DB used in BDK 0.29.
// TODO(max): Remove
pub mod db29;

/// "`stop_gap` is the maximum number of consecutive unused addresses. For
/// example, with a `stop_gap` of  3, `full_scan` will keep scanning until it
/// encounters 3 consecutive script pubkeys with no associated transactions."
///
/// From: [`EsploraAsyncExt::full_scan`]
const BDK_FULL_SCAN_STOP_GAP: usize = 2;
/// Number of parallel requests BDK is permitted to use.
const BDK_CONCURRENCY: usize = 24;

/// The [`ConfirmationPriority`] for new open_channel funding transactions.
///
/// See: [`LexeWallet::create_and_sign_funding_tx`]
///  and [`LexeWallet::preflight_channel_funding_tx`].
const CHANNEL_FUNDING_CONF_PRIO: ConfirmationPriority =
    ConfirmationPriority::Normal;

type TxBuilderType<'wallet, MODE> =
    TxBuilder<'wallet, WalletDb, DefaultCoinSelectionAlgorithm, MODE>;

/// A newtype wrapper around [`bdk::Wallet`]. Can be cloned and used directly.
// TODO(max): All LexeWallet methods currently use `lock().await` so that we can
// avoid `try_lock()` which could cause random failures. What we really want,
// however, is to make all of these methods non-async and switch back to the
// std::sync::Mutex (or no Mutex at all), but BDK needs to become robust to
// concurrent access first.
#[derive(Clone)]
pub struct LexeWallet {
    // TODO(max): Not security critical; should use Lexe's 'internal' Esplora.
    esplora: Arc<LexeEsplora>,
    // The Mutex is needed because bdk29::Wallet (without our patch) is not
    // Send, and therefore does not guarantee that concurrent accesses will
    // not panic on internal locking calls. Furthermore, since a lock to
    // the bdk29::Wallet needs to be held while awaiting on BDK wallet
    // sync, the Mutex we use must be a Tokio mutex. See the patched
    // commits for more details:
    //
    // - https://github.com/lexe-app/bdk/tree/max/thread-safe
    // - https://github.com/bitcoindevkit/bdk/commit/c5b2f5ac9ac152a7e0658ca99ccaf854b9063727
    // - https://github.com/bitcoindevkit/bdk/commit/ddc84ca1916620d021bae8c467c53555b7c62467
    // TODO(max): Switch over everything to new wallet, then remove
    #[allow(dead_code)] // TODO(max): Remove
    bdk29_wallet: Arc<tokio::sync::Mutex<bdk29::Wallet<WalletDb29>>>,
    // TODO(max): Implement wallet persistence
    // TODO(max): A lot of methods can be sync again, since no need tokio mutex
    #[allow(dead_code)] // TODO(max): Remove
    wallet: Arc<std::sync::RwLock<bdk::Wallet<WalletDb>>>,
}

impl LexeWallet {
    /// Constructs a new [`LexeWallet`] from a [`RootSeed`] and [`WalletDb`].
    /// Wallet addresses are generated according to the [BIP 84] standard.
    /// See also [BIP 44].
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    pub async fn init(
        root_seed: &RootSeed,
        network: LxNetwork,
        esplora: Arc<LexeEsplora>,
        wallet_db: WalletDb,
    ) -> anyhow::Result<Self> {
        let network = network.to_bitcoin();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        let bdk29_wallet = {
            use std::time::Duration;

            use bdk29::{template::Bip84, KeychainKind};

            // Descriptor for external (receive) addresses:
            // `m/84h/{0,1}h/0h/0/*`
            let external_descriptor =
                Bip84(master_xprv, KeychainKind::External);
            // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
            let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

            let (wallet_db_persister_tx, wallet_db_persister_rx) =
                mpsc::channel(256);
            let wallet_db29 = WalletDb29::new(wallet_db_persister_tx);

            // Hack to prevent dropping rx while we transition to BDK 1.0
            LxTask::spawn(async move {
                tokio::time::sleep(Duration::from_secs(60 * 60 * 24 * 365))
                    .await;
                std::mem::drop(wallet_db_persister_rx);
            })
            .detach();

            bdk29::Wallet::new(
                external_descriptor,
                Some(change_descriptor),
                network,
                wallet_db29,
            )
            .map(tokio::sync::Mutex::new)
            .map(Arc::new)
            .context("bdk29::Wallet::new failed")?
        };

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        let db_empty = wallet_db.changeset().is_empty();

        let wallet = bdk::Wallet::new_or_load(
            external_descriptor,
            Some(change_descriptor),
            wallet_db,
            network,
        )
        .map(std::sync::RwLock::new)
        .map(Arc::new)
        .context("bdk::Wallet::new failed")?;

        let lexe_wallet = Self {
            esplora,
            bdk29_wallet,
            wallet,
        };

        if db_empty {
            // After the first full sync, the db won't be empty anymore.
            lexe_wallet
                .full_sync()
                .await
                .context("Failed to conduct initial full sync")?;
        }

        Ok(lexe_wallet)
    }

    /// Returns a read lock on the inner [`bdk::Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    pub fn read(&self) -> RwLockReadGuard<'_, bdk::Wallet<WalletDb>> {
        self.wallet.read().unwrap()
    }

    /// Returns a write lock on the inner [`bdk::Wallet`].
    /// The caller is responsible for avoiding deadlocks.
    pub fn write(&self) -> RwLockWriteGuard<'_, bdk::Wallet<WalletDb>> {
        self.wallet.write().unwrap()
    }

    /// Syncs the [`bdk::Wallet`] using a remote Esplora backend.
    #[instrument(skip_all, name = "(bdk-sync)")]
    pub async fn sync(&self) -> anyhow::Result<()> {
        // The full set of script pubkeys we want to check for updates.
        let script_pubkeys;
        // The UTXOs (outpoints) we check to see if they have been spent.
        let utxos;
        // The txids of txns we want to check if they have been spent.
        let unconfirmed_txids;
        let local_chain;
        let prev_tip;
        {
            let locked_wallet = self.wallet.read().unwrap();

            let keychains = locked_wallet.spk_index();
            let tx_graph = locked_wallet.tx_graph();
            local_chain = locked_wallet.local_chain().clone();
            let chain_tip = local_chain.tip();

            // Sync all external script pubkeys we have ever revealed.
            let external_spks = keychains
                .revealed_keychain_spks(&KeychainKind::External)
                .map(|(_idx, script)| script);
            // Sync all internal (change) spks we've revealed but have not used.
            let unused_internal_spks = keychains
                .unused_keychain_spks(&KeychainKind::Internal)
                .map(|(_idx, script)| script);
            // Sync the last used internal (change) spk, in case a crash or race
            // condition causes us to reuse the last-revealed internal spk.
            let last_used_internal_spk =
                keychains.last_used_index(&KeychainKind::Internal).and_then(
                    |idx| keychains.spk_at_index(KeychainKind::Internal, idx),
                );

            script_pubkeys = external_spks
                .chain(unused_internal_spks)
                .chain(last_used_internal_spk.into_iter())
                .map(ToOwned::to_owned)
                .collect::<Vec<bitcoin::ScriptBuf>>();

            utxos = locked_wallet
                .list_unspent()
                .map(|utxo| utxo.outpoint)
                .collect::<Vec<bitcoin::OutPoint>>();

            unconfirmed_txids = tx_graph
                .list_chain_txs(&local_chain, chain_tip.block_id())
                .filter(|canonical_tx| {
                    !canonical_tx.chain_position.is_confirmed()
                })
                .map(|canonical_tx| canonical_tx.tx_node.txid)
                .collect::<Vec<bitcoin::Txid>>();

            prev_tip = chain_tip;
        }

        let esplora_client = self.esplora.client();

        // Check for updates to our our spks, unconfirmed txids, and utxos.
        // We get a `TxGraph` containing updates to be made to our local chain.
        let tx_graph_update = esplora_client
            .sync(script_pubkeys, unconfirmed_txids, utxos, BDK_CONCURRENCY)
            .await
            .context("`EsploraAsyncExt::sync` failed")?;

        // Determine the block heights missing from our local chain based on the
        // info in our `TxGraph` update. Returns an iterator over u32 heights.
        let missing_heights = tx_graph_update.missing_heights(&local_chain);

        // Now, prepare our local chain update based on the missing heights.
        let local_chain_update = esplora_client
            .update_local_chain(prev_tip, missing_heights)
            .await
            .context("Failed to update local chain")?;
        let update = Update {
            graph: tx_graph_update,
            chain: Some(local_chain_update),
            last_active_indices: Default::default(),
        };

        // Finally, apply the combined update to the wallet.
        {
            let mut locked_wallet = self.wallet.write().unwrap();
            locked_wallet
                .apply_update(update)
                .context("Couldn't apply update")?;
            locked_wallet.commit().context("Couldn't commit update")?;
        }

        Ok(())
    }

    /// Conducts a full sync of all script pubkeys derived from all of our
    /// wallet descriptors, until a stop gap is hit on both of our keychains.
    ///
    /// This should be done rarely, i.e. only when creating the wallet or if we
    /// need to restore from a existing seed. See BDK's examples for more info.
    async fn full_sync(&self) -> anyhow::Result<()> {
        let keychains_spks;
        let prev_tip;
        let local_chain;
        {
            let locked_wallet = self.wallet.read().unwrap();
            // Iterators over the script pks of all of our keychain descriptors
            // (i.e. our external and internal/change keychains).
            keychains_spks = locked_wallet.all_unbounded_spk_iters();
            prev_tip = locked_wallet.latest_checkpoint();
            local_chain = locked_wallet.local_chain().clone();
        };

        // Scan the blockchain for our keychain script pubkeys until we hit the
        // `stop_gap`. We get a `TxGraph` update and the last active script
        // pubkey derivation indices for each of our `KeychainKind`s.
        let esplora_client = self.esplora.client();
        let (tx_graph_update, last_active_indices) = esplora_client
            .full_scan::<KeychainKind>(
                keychains_spks,
                BDK_FULL_SCAN_STOP_GAP,
                BDK_CONCURRENCY,
            )
            .await
            .context("EsploraAsyncExt::full_scan failed")?;

        // Determine the block heights missing from our local chain based on the
        // info in our `TxGraph` update. Returns an iterator over u32 heights.
        let missing_heights = tx_graph_update.missing_heights(&local_chain);

        // Now, prepare our local chain update based on the missing heights.
        let local_chain_update = esplora_client
            .update_local_chain(prev_tip, missing_heights)
            .await
            .context("Failed to update local chain")?;
        let update = Update {
            last_active_indices,
            graph: tx_graph_update,
            chain: Some(local_chain_update),
        };

        // Finally, apply the combined update to the wallet.
        {
            let mut locked_wallet = self.wallet.write().unwrap();
            locked_wallet
                .apply_update(update)
                .context("Couldn't apply update")?;
            locked_wallet.commit().context("Couldn't commit update")?;
        }

        Ok(())
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    pub fn get_balance(&self) -> Balance {
        let balance = self.wallet.read().unwrap().get_balance();

        // Convert bdk::Balance to common::ln::balance::Balance.
        // Not using a From impl bc we don't want `common` to depend on BDK.
        let bdk::wallet::Balance {
            immature,
            trusted_pending,
            untrusted_pending,
            confirmed,
        } = balance;

        Balance {
            immature_sat: immature,
            trusted_pending_sat: trusted_pending,
            untrusted_pending_sat: untrusted_pending,
            confirmed_sat: confirmed,
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
    ///
    /// See [`AddressIndex`] for more details.
    pub fn get_address(&self) -> bitcoin::Address {
        self.wallet
            .write()
            .unwrap()
            .get_address(AddressIndex::LastUnused)
            .address
    }

    /// Returns the last unused address from the internal (change) descriptor.
    ///
    /// Our [`sync`] implementation relies on internal addresses never being
    /// exposed to users in any way, as otherwise users might send funds to
    /// these addresses and expect their funds to show up. In our node code and
    /// elsewhere, always receiving to the last unused internal address allows
    /// us to avoid having to check these addresses for updates after they
    /// have been used.
    ///
    /// [`sync`]: Self::sync
    pub fn get_internal_address(&self) -> bitcoin::Address {
        self.wallet
            .write()
            .unwrap()
            .get_internal_address(AddressIndex::LastUnused)
            .address
    }

    /// Determine if we have enough on-chain balance for a potential channel
    /// funding tx of this `channel_value_sats`. If so, return the estimated
    /// on-chain fees.
    pub(crate) fn preflight_channel_funding_tx(
        &self,
        channel_value_sats: u64,
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

        let mut locked_wallet = self.wallet.write().unwrap();

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
        let feerate = self.esplora.conf_prio_to_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&mut locked_wallet, feerate);
        tx_builder.add_recipient(fake_output_script, channel_value_sats);
        let psbt = tx_builder
            .finish()
            .context("Could not build channel funding tx")?;

        let fee = psbt.fee().context("Bad PSBT fee")?;
        let fee_amount = Amount::try_from_sats_u64(fee.to_sat())
            .context("Bad fee amount")?;
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
        channel_value_sats: u64,
    ) -> anyhow::Result<Transaction> {
        let mut locked_wallet = self.wallet.write().unwrap();

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
        let feerate = self.esplora.conf_prio_to_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&mut locked_wallet, feerate);
        tx_builder.add_recipient(output_script, channel_value_sats);
        let mut psbt = tx_builder
            .finish()
            .context("Could not build funding PSBT")?;

        // Sign
        Self::default_sign_psbt(&locked_wallet, &mut psbt)
            .context("Could not sign funding PSBT")?;

        Ok(psbt.extract_tx())
    }

    /// Create and sign a transaction which sends the given amount to the given
    /// address, packaging up all of this info in a new [`OnchainSend`].
    pub(crate) fn create_onchain_send(
        &self,
        req: PayOnchainRequest,
        network: LxNetwork,
    ) -> anyhow::Result<OnchainSend> {
        let (tx, fees) = {
            let mut locked_wallet = self.wallet.write().unwrap();

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
                .add_recipient(address.script_pubkey(), req.amount.sats_u64());
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

            (psbt.extract_tx(), fee_amount)
        };

        let onchain_send = OnchainSend::new(tx, req, fees);
        Ok(onchain_send)
    }

    /// Estimate the network fee for a potential onchain send payment. We return
    /// estimates for each [`ConfirmationPriority`] preset.
    ///
    /// This fn deliberately avoids modifying the [`WalletDb`] state. We don't
    /// want to generate unnecessary addresses that we need to watch and sync.
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

        let mut locked_wallet = self.wallet.write().unwrap();

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

        Ok(PreflightPayOnchainResponse {
            high: high_fee,
            normal: normal_fee,
            background: background_fee,
        })
    }

    fn preflight_pay_onchain_inner(
        wallet: &mut bdk::Wallet<WalletDb>,
        address: &bitcoin::Address,
        amount: Amount,
        feerate: bitcoin::FeeRate,
    ) -> anyhow::Result<FeeEstimate> {
        // We're just estimating the fee for tx; we don't want to create
        // unnecessary change outputs, which will need to be persisted and take
        // up sync time. `AddressIndex::Peek` will just derive the output at the
        // index without persisting anything.
        // NOTE: `get_[internal_]address` is technically fine atm because it
        // uses `AddressIndex::LastUnused`, but that might change in the future.
        let change_address = wallet.get_internal_address(AddressIndex::Peek(0));
        let mut tx_builder = Self::default_tx_builder(wallet, feerate);
        tx_builder.add_recipient(address.script_pubkey(), amount.sats_u64());
        tx_builder.drain_to(change_address.script_pubkey());
        let psbt = tx_builder
            .finish()
            .context("Failed to build onchain send tx")?;
        let fee = psbt.fee().context("Bad PSBT fee")?;
        let amount = Amount::try_from_sats_u64(fee.to_sat())
            .context("Bad fee amount")?;
        Ok(FeeEstimate { amount })
    }

    /// Get a [`TxBuilder`] which has some defaults prepopulated.
    ///
    /// Note that this builder is specifically for *creating* transactions, not
    /// for e.g. bumping the fee of an existing transaction.
    fn default_tx_builder(
        wallet: &mut bdk::Wallet<WalletDb>,
        feerate: bitcoin::FeeRate,
    ) -> TxBuilderType<'_, CreateTx> {
        // Set the feerate and enable RBF by default
        let mut tx_builder = wallet.build_tx();
        tx_builder.enable_rbf();
        tx_builder.fee_rate(feerate);
        tx_builder
    }

    /// Sign a [`PartiallySignedTransaction`] in the default way.
    fn default_sign_psbt(
        wallet: &bdk::Wallet<WalletDb>,
        psbt: &mut PartiallySignedTransaction,
    ) -> anyhow::Result<()> {
        let options = SignOptions::default();
        let finalized = wallet.sign(psbt, options)?;
        ensure!(finalized, "Failed to sign all PSBT inputs");
        Ok(())
    }
}

/// Spawns a task that persists the current [`WalletDb`] state whenever it
/// receives a notification (via the `wallet_db_persister_rx` channel) that the
/// [`WalletDb`] needs to be re-persisted.
pub fn spawn_wallet_db_persister_task<PS: LexePersister>(
    persister: PS,
    wallet_db: WalletDb,
    mut wallet_db_persister_rx: notify::Receiver,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("wallet db persister", async move {
        loop {
            tokio::select! {
                () = wallet_db_persister_rx.recv() => {
                    // Serialize changeset to JSON bytes, encrypt, then persist
                    let persist_fut = async {
                        let basic_file = persister.encrypt_json(
                            SINGLETON_DIRECTORY.to_owned(),
                            WALLET_DB_FILENAME.to_owned(),
                            &wallet_db.changeset(),
                        );
                        let persist_res = persister
                            .persist_file(
                                basic_file, IMPORTANT_PERSIST_RETRIES
                            )
                            .await
                            .context("Could not persist wallet db");
                        match persist_res {
                            Ok(()) => debug!("Success: persisted wallet db"),
                            Err(e) => warn!("Wallet DB persist error: {e:#}"),
                        }
                    };

                    // Give up during the persist if we recv a shutdown signal
                    tokio::select! {
                        () = persist_fut => {}
                        () = shutdown.recv() =>
                            break info!("Giving up on wallet db persist"),
                    }
                }
                () = shutdown.recv() => break,
            }
        }

        info!("wallet db persister task shutting down");
    })
}
