use std::sync::Arc;

use anyhow::{ensure, Context};
use bdk::{
    blockchain::{EsploraBlockchain, Progress},
    template::Bip84,
    wallet::{
        coin_selection::DefaultCoinSelectionAlgorithm, signer::SignOptions,
        tx_builder::CreateTx, AddressIndex, Wallet,
    },
    FeeRate, KeychainKind, SyncOptions, TransactionDetails, TxBuilder,
};
use bitcoin::{
    util::{address::Address, psbt::PartiallySignedTransaction},
    Script, Transaction, Txid,
};
use common::{
    api::command::SendOnchainRequest,
    cli::Network,
    constants::{
        IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
    },
    ln::{amount::Amount, balance::Balance},
    root_seed::RootSeed,
    shutdown::ShutdownChannel,
    task::LxTask,
};
use lightning::chain::chaininterface::ConfirmationTarget;
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use crate::{
    esplora::LexeEsplora, payments::onchain::OnchainSend,
    traits::LexePersister, wallet::db::WalletDb,
};

/// Wallet DB.
pub mod db;

/// The 'stop_gap' parameter used by BDK's wallet sync. This seems to configure
/// the threshold number of blocks after which BDK stops looking for scripts
/// belonging to the wallet. BDK's default value for this is 20.
const BDK_WALLET_SYNC_STOP_GAP: usize = 20;

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
    // The Mutex is needed because bdk::Wallet (without our patch) is not Send,
    // and therefore does not guarantee that concurrent accesses will not panic
    // on internal locking calls. Furthermore, since a lock to the bdk::Wallet
    // needs to be held while awaiting on BDK wallet sync, the Mutex we use
    // must be a Tokio mutex. See the patched commits for more details:
    //
    // - https://github.com/lexe-tech/bdk/tree/max/thread-safe
    // - https://github.com/bitcoindevkit/bdk/commit/c5b2f5ac9ac152a7e0658ca99ccaf854b9063727
    // - https://github.com/bitcoindevkit/bdk/commit/ddc84ca1916620d021bae8c467c53555b7c62467
    wallet: Arc<tokio::sync::Mutex<Wallet<WalletDb>>>,
}

impl LexeWallet {
    /// Constructs a new [`LexeWallet`] from a [`RootSeed`] and [`WalletDb`].
    /// Wallet addresses are generated according to the [BIP 84] standard. See
    /// also [BIP 44].
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    pub fn new(
        root_seed: &RootSeed,
        network: Network,
        esplora: Arc<LexeEsplora>,
        wallet_db: WalletDb,
    ) -> anyhow::Result<Self> {
        let network = network.to_inner();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        let wallet = Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db,
        )
        .map(tokio::sync::Mutex::new)
        .map(Arc::new)
        .context("bdk::Wallet::new failed")?;

        Ok(Self { esplora, wallet })
    }

    /// Syncs the inner [`bdk::Wallet`] using the given Esplora server.
    ///
    /// NOTE: Beware deadlocks; this function holds a lock to the inner
    /// [`bdk::Wallet`] during wallet sync. It is held across `.await`.
    #[instrument(skip_all, name = "(bdk-sync)")]
    pub async fn sync(&self) -> anyhow::Result<()> {
        let esplora_blockchain = EsploraBlockchain::from_client(
            self.esplora.client().clone(),
            BDK_WALLET_SYNC_STOP_GAP,
        );

        let progress =
            Some(Box::new(ProgressLogger) as Box<(dyn Progress + 'static)>);
        let sync_options = SyncOptions { progress };

        self.wallet
            .lock()
            .await
            .sync(&esplora_blockchain, sync_options)
            .await
            .context("bdk::Wallet::sync failed")
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    pub async fn get_balance(&self) -> anyhow::Result<Balance> {
        self.wallet
            .lock()
            .await
            .get_balance()
            // Convert bdk::Balance to common::ln::balance::Balance.
            // Not using a From impl bc we don't want `common` to depend on BDK.
            .map(
                |bdk::Balance {
                     immature,
                     trusted_pending,
                     untrusted_pending,
                     confirmed,
                 }| Balance {
                    immature_sat: immature,
                    trusted_pending_sat: trusted_pending,
                    untrusted_pending_sat: untrusted_pending,
                    confirmed_sat: confirmed,
                },
            )
            .context("Could not get balance")
    }

    /// Returns the last unused address derived using the external descriptor.
    ///
    /// We employ this address index selection strategy because it prevents a
    /// DoS attack where `get_new_address` is called repeatedly, making
    /// transaction sync (which generally requires one API call per watched
    /// address) extremely expensive.
    ///
    /// NOTE: If a user tries to send two on-chain txs to their wallet in quick
    /// succession, the second call to `get_new_address` will return the same
    /// address as the first if the wallet has not yet detected the first
    /// transaction. If the user wishes to avoid address reuse, they should wait
    /// for their wallet to sync before sending the second transaction (or
    /// simply avoid this scenario in the first place).
    ///
    /// See [`AddressIndex`] for more details.
    pub async fn get_new_address(&self) -> anyhow::Result<Address> {
        self.wallet
            .lock()
            .await
            .get_address(AddressIndex::LastUnused)
            .map(|info| info.address)
            .context("Could not get new address")
    }

    /// Calls [`bdk::Wallet::list_transactions`].
    pub async fn list_transactions(
        &self,
        include_raw: bool,
    ) -> anyhow::Result<Vec<TransactionDetails>> {
        self.wallet
            .lock()
            .await
            .list_transactions(include_raw)
            .context("Could not list transactions")
    }

    /// Calls [`bdk::Wallet::get_tx`].
    pub async fn get_tx(
        &self,
        txid: &Txid,
        include_raw: bool,
    ) -> anyhow::Result<Option<TransactionDetails>> {
        self.wallet
            .lock()
            .await
            .get_tx(txid, include_raw)
            .context("Could not get tx")
    }

    /// Create and sign a funding tx given an output script, channel value, and
    /// confirmation target. Intended to be called downstream of an
    /// [`FundingGenerationReady`] event
    ///
    /// [`FundingGenerationReady`]: lightning::util::events::Event::FundingGenerationReady
    pub(crate) async fn create_and_sign_funding_tx(
        &self,
        output_script: Script,
        channel_value_satoshis: u64,
        conf_target: ConfirmationTarget,
    ) -> anyhow::Result<Transaction> {
        let locked_wallet = self.wallet.lock().await;

        // Build
        let bdk_feerate = self.esplora.get_bdk_feerate(conf_target);
        let mut tx_builder =
            Self::default_tx_builder(&locked_wallet, bdk_feerate);
        tx_builder.add_recipient(output_script, channel_value_satoshis);
        let (mut psbt, _tx_details) = tx_builder
            .finish()
            .context("Could not build funding PSBT")?;

        // Sign
        Self::default_sign_psbt(&locked_wallet, &mut psbt)
            .context("Could not sign funding PSBT")?;

        Ok(psbt.extract_tx())
    }

    /// Create and sign a transaction which sends an [`Amount`] to the given
    /// [`Address`], packaging up all of this info in a new [`OnchainSend`].
    pub(crate) async fn create_onchain_send(
        &self,
        req: SendOnchainRequest,
    ) -> anyhow::Result<OnchainSend> {
        let locked_wallet = self.wallet.lock().await;
        let script_pubkey = req.address.script_pubkey();

        // Build
        let conf_target = ConfirmationTarget::from(req.priority);
        let bdk_feerate = self.esplora.get_bdk_feerate(conf_target);
        let mut tx_builder =
            Self::default_tx_builder(&locked_wallet, bdk_feerate);
        tx_builder.add_recipient(script_pubkey, req.amount.sats_u64());
        let (mut psbt, _tx_details) =
            tx_builder.finish().context("Could not build outbound tx")?;

        // Sign
        Self::default_sign_psbt(&locked_wallet, &mut psbt)
            .context("Could not sign outbound tx")?;
        let tx = psbt.extract_tx();

        // Fees = (sum of inputs) - (sum of outputs)
        let sum_of_inputs_sat = tx
            .input
            .iter()
            // Inputs don't contain amounts, only references to the outputs that
            // they spend, so we have to fetch these outputs from our wallet.
            .map(|input| {
                let txo = locked_wallet
                    .get_utxo(input.previous_output)
                    .context("Error while fetching utxo for input")?
                    .context("Missing utxo for input")?;
                Ok(txo.txout.value)
            })
            // Convert `Iter<anyhow::Result<u64>>` to `anyhow::Result<u64>` by
            // summing the contained u64s, returning Ok iff all inner were Ok
            .try_fold(0, |acc, res: anyhow::Result<u64>| {
                res.map(|v| acc + v)
            })?;
        let sum_of_outputs_sat =
            tx.output.iter().map(|output| output.value).sum::<u64>();
        let fees = sum_of_inputs_sat
            .checked_sub(sum_of_outputs_sat)
            .map(Decimal::from)
            .map(Amount::from_satoshis)
            .context("Sum of outputs exceeds sum of inputs")?
            .context("Fee amount overflowed")?;

        let onchain_send = OnchainSend::new(tx, req, fees);

        Ok(onchain_send)
    }

    /// Get a [`TxBuilder`] which has some defaults prepopulated.
    ///
    /// Note that this builder is specifically for *creating* transactions, not
    /// for e.g. bumping the fee of an existing transaction.
    fn default_tx_builder(
        wallet: &Wallet<WalletDb>,
        bdk_feerate: FeeRate,
    ) -> TxBuilderType<'_, CreateTx> {
        // Set the feerate and enable RBF by default
        let mut tx_builder = wallet.build_tx();
        tx_builder.fee_rate(bdk_feerate).enable_rbf();
        tx_builder
    }

    /// Sign a [`PartiallySignedTransaction`] in the default way.
    fn default_sign_psbt(
        wallet: &Wallet<WalletDb>,
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
    mut wallet_db_persister_rx: mpsc::Receiver<()>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("wallet db persister", async move {
        loop {
            tokio::select! {
                Some(()) = wallet_db_persister_rx.recv() => {
                    // Clear out all (possibly) remaining notifications on the
                    // channel; they'll all be handled in the following persist.
                    while let Ok(()) = wallet_db_persister_rx.try_recv() {}

                    // Serialize to JSON bytes, encrypt, then persist
                    let persist_fut = async {
                        let basic_file = persister.encrypt_json(
                            SINGLETON_DIRECTORY.to_owned(),
                            WALLET_DB_FILENAME.to_owned(),
                            &wallet_db,
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

/// A struct that logs every [`Progress`] update at info.
#[derive(Debug)]
struct ProgressLogger;

impl Progress for ProgressLogger {
    fn update(
        &self,
        progress: f32,
        message: Option<String>,
    ) -> Result<(), bdk::Error> {
        match message {
            Some(msg) => info!("BDK sync progress: {progress}%, msg: {msg}"),
            None => info!("BDK sync progress: {progress}%"),
        }
        Ok(())
    }
}
