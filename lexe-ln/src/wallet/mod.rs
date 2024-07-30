use std::sync::Arc;

use anyhow::{ensure, Context};
use bdk29::{
    blockchain::{EsploraBlockchain, Progress},
    template::Bip84,
    wallet::{
        coin_selection::DefaultCoinSelectionAlgorithm, signer::SignOptions,
        tx_builder::CreateTx, AddressIndex,
    },
    FeeRate, KeychainKind, SyncOptions, TransactionDetails, TxBuilder,
};
use bitcoin::{psbt::PartiallySignedTransaction, Transaction, Txid};
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
    root_seed::RootSeed,
    shutdown::ShutdownChannel,
    task::LxTask,
};
use tokio::sync::mpsc;
use tracing::{debug, info, instrument, warn};

use crate::{
    esplora::LexeEsplora,
    payments::onchain::OnchainSend,
    traits::{LexeInnerPersister, LexePersister},
    wallet::db::WalletDb,
};

/// Wallet DB.
pub mod db;

/// The 'stop_gap' parameter used by BDK's wallet sync. This seems to configure
/// the threshold number of blocks after which BDK stops looking for scripts
/// belonging to the wallet. BDK's default value for this is 20.
const BDK_WALLET_SYNC_STOP_GAP: usize = 20;

/// The [`ConfirmationPriority`] for new open_channel funding transactions.
///
/// See: [`LexeWallet::create_and_sign_funding_tx`]
///  and [`LexeWallet::preflight_channel_funding_tx`].
const CHANNEL_FUNDING_CONF_PRIO: ConfirmationPriority =
    ConfirmationPriority::Normal;

type TxBuilderType<'wallet, MODE> =
    TxBuilder<'wallet, WalletDb, DefaultCoinSelectionAlgorithm, MODE>;

/// A newtype wrapper around [`bdk29::Wallet`]. Can be cloned and used directly.
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
    bdk29_wallet: Arc<tokio::sync::Mutex<bdk29::Wallet<WalletDb>>>,
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
        network: LxNetwork,
        esplora: Arc<LexeEsplora>,
        wallet_db: WalletDb,
    ) -> anyhow::Result<Self> {
        let network = network.to_bitcoin();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        let wallet = bdk29::Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db,
        )
        .map(tokio::sync::Mutex::new)
        .map(Arc::new)
        .context("bdk29::Wallet::new failed")?;

        Ok(Self {
            esplora,
            bdk29_wallet: wallet,
        })
    }

    /// Syncs the inner [`bdk29::Wallet`] using the given Esplora server.
    ///
    /// NOTE: Beware deadlocks; this function holds a lock to the inner
    /// [`bdk29::Wallet`] during wallet sync. It is held across `.await`.
    #[instrument(skip_all, name = "(bdk-sync)")]
    pub async fn sync(&self) -> anyhow::Result<()> {
        let esplora_blockchain = EsploraBlockchain::from_client(
            self.esplora.client().clone(),
            BDK_WALLET_SYNC_STOP_GAP,
        );

        let progress =
            Some(Box::new(ProgressLogger) as Box<(dyn Progress + 'static)>);
        let sync_options = SyncOptions { progress };

        self.bdk29_wallet
            .lock()
            .await
            .sync(&esplora_blockchain, sync_options)
            .await
            .context("bdk29::Wallet::sync failed")
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    pub async fn get_balance(&self) -> anyhow::Result<Balance> {
        self.bdk29_wallet
            .lock()
            .await
            .get_balance()
            // Convert bdk29::Balance to common::ln::balance::Balance.
            // Not using a From impl bc we don't want `common` to depend on BDK.
            .map(
                |bdk29::Balance {
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
    pub async fn get_address(&self) -> anyhow::Result<bitcoin::Address> {
        self.bdk29_wallet
            .lock()
            .await
            .get_address(AddressIndex::LastUnused)
            .map(|info| info.address)
            .context("Could not get new address")
    }

    /// Calls [`bdk29::Wallet::list_transactions`].
    pub async fn list_transactions(
        &self,
        include_raw: bool,
    ) -> anyhow::Result<Vec<TransactionDetails>> {
        self.bdk29_wallet
            .lock()
            .await
            .list_transactions(include_raw)
            .context("Could not list transactions")
    }

    /// Calls [`bdk29::Wallet::get_tx`].
    pub async fn get_tx(
        &self,
        txid: &Txid,
        include_raw: bool,
    ) -> anyhow::Result<Option<TransactionDetails>> {
        self.bdk29_wallet
            .lock()
            .await
            .get_tx(txid, include_raw)
            .context("Could not get tx")
    }

    /// Determine if we have enough on-chain balance for a potential channel
    /// funding tx of this `channel_value_sats`. If so, return the estimated
    /// on-chain fees.
    pub(crate) async fn preflight_channel_funding_tx(
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

        let locked_wallet = self.bdk29_wallet.lock().await;

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
        let bdk_feerate = self.esplora.conf_prio_to_bdk_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&locked_wallet, bdk_feerate);
        tx_builder.add_recipient(fake_output_script, channel_value_sats);
        let (_psbt, tx_details) = tx_builder
            .finish()
            .context("Could not build channel funding tx")?;

        // Extract fees
        let fees = tx_details
            .fee
            .expect("When creating a new tx, bdk always sets the fee value");

        Amount::try_from_sats_u64(fees).context("Bad fee amount")
    }

    /// Create and sign a funding tx given an output script, channel value, and
    /// confirmation target. Intended to be called downstream of an
    /// [`FundingGenerationReady`] event
    ///
    /// [`FundingGenerationReady`]: lightning::events::Event::FundingGenerationReady
    pub(crate) async fn create_and_sign_funding_tx(
        &self,
        output_script: bitcoin::ScriptBuf,
        channel_value_sats: u64,
    ) -> anyhow::Result<Transaction> {
        let locked_wallet = self.bdk29_wallet.lock().await;

        // Build
        let conf_prio = CHANNEL_FUNDING_CONF_PRIO;
        let bdk_feerate = self.esplora.conf_prio_to_bdk_feerate(conf_prio);
        let mut tx_builder =
            Self::default_tx_builder(&locked_wallet, bdk_feerate);
        tx_builder.add_recipient(output_script, channel_value_sats);
        let (mut psbt, _tx_details) = tx_builder
            .finish()
            .context("Could not build funding PSBT")?;

        // Sign
        Self::default_sign_psbt(&locked_wallet, &mut psbt)
            .context("Could not sign funding PSBT")?;

        Ok(psbt.extract_tx())
    }

    /// Create and sign a transaction which sends the given amount to the given
    /// address, packaging up all of this info in a new [`OnchainSend`].
    pub(crate) async fn create_onchain_send(
        &self,
        req: PayOnchainRequest,
        network: LxNetwork,
    ) -> anyhow::Result<OnchainSend> {
        let (tx, fees) = {
            let locked_wallet = self.bdk29_wallet.lock().await;

            let address = req
                .address
                .clone()
                .require_network(network.into())
                .context("Invalid network")?;

            // Build unsigned tx
            let bdk_feerate =
                self.esplora.conf_prio_to_bdk_feerate(req.priority);
            let mut tx_builder =
                Self::default_tx_builder(&locked_wallet, bdk_feerate);
            tx_builder
                .add_recipient(address.script_pubkey(), req.amount.sats_u64());
            let (mut psbt, tx_details) = tx_builder
                .finish()
                .context("Failed to build onchain send tx")?;

            let fees = tx_details.fee.expect(
                "When creating a new tx, bdk always sets the fee value",
            );
            let fees =
                Amount::try_from_sats_u64(fees).context("Bad fee amount")?;

            // Sign tx
            Self::default_sign_psbt(&locked_wallet, &mut psbt)
                .context("Could not sign outbound tx")?;

            (psbt.extract_tx(), fees)
        };

        let onchain_send = OnchainSend::new(tx, req, fees);

        Ok(onchain_send)
    }

    /// Estimate the network fee for a potential onchain send payment. We return
    /// estimates for each [`ConfirmationPriority`] preset.
    ///
    /// This fn deliberately avoids modifying the [`WalletDb`] state. We don't
    /// want to generate unnecessary addresses that we need to watch and sync.
    pub(crate) async fn preflight_pay_onchain(
        &self,
        req: PreflightPayOnchainRequest,
        network: LxNetwork,
    ) -> anyhow::Result<PreflightPayOnchainResponse> {
        let high_prio = ConfirmationPriority::High;
        let normal_prio = ConfirmationPriority::Normal;
        let background_prio = ConfirmationPriority::Background;

        let high_feerate = self.esplora.conf_prio_to_bdk_feerate(high_prio);
        let normal_feerate = self.esplora.conf_prio_to_bdk_feerate(normal_prio);
        let background_feerate =
            self.esplora.conf_prio_to_bdk_feerate(background_prio);

        let locked_wallet = self.bdk29_wallet.lock().await;

        // We _require_ a tx to at least be able to use normal fee rate.
        let address = req.address.require_network(network.into())?;
        let normal_fee = Self::preflight_pay_onchain_inner(
            &locked_wallet,
            &address,
            req.amount,
            normal_feerate,
        )?;
        let background_fee = Self::preflight_pay_onchain_inner(
            &locked_wallet,
            &address,
            req.amount,
            background_feerate,
        )?;

        // The high fee rate tx is allowed to fail with insufficient balance.
        let high_fee = Self::preflight_pay_onchain_inner(
            &locked_wallet,
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
        wallet: &bdk29::Wallet<WalletDb>,
        address: &bitcoin::Address,
        amount: Amount,
        bdk_feerate: FeeRate,
    ) -> anyhow::Result<FeeEstimate> {
        // We're just estimating the fee for tx; we don't want to create
        // unnecessary change outputs, which will need to be persisted and take
        // up sync time. `AddressIndex::Peek` will just derive the output at the
        // index without persisting anything. It should always succeed.
        let change_address = wallet
            .get_internal_address(AddressIndex::Peek(0))
            .context("Failed to derive change address")?;

        let mut tx_builder = Self::default_tx_builder(wallet, bdk_feerate);
        tx_builder.add_recipient(address.script_pubkey(), amount.sats_u64());
        tx_builder.drain_to(change_address.script_pubkey());
        let (_, tx_details) = tx_builder
            .finish()
            .context("Failed to build onchain send tx")?;

        let fees = tx_details
            .fee
            .expect("When creating a new tx, bdk always sets the fee value");
        Ok(FeeEstimate {
            amount: Amount::try_from_sats_u64(fees)
                .context("Bad fee amount")?,
        })
    }

    /// Get a [`TxBuilder`] which has some defaults prepopulated.
    ///
    /// Note that this builder is specifically for *creating* transactions, not
    /// for e.g. bumping the fee of an existing transaction.
    fn default_tx_builder(
        wallet: &bdk29::Wallet<WalletDb>,
        bdk_feerate: FeeRate,
    ) -> TxBuilderType<'_, CreateTx> {
        // Set the feerate and enable RBF by default
        let mut tx_builder = wallet.build_tx();
        tx_builder.enable_rbf();
        tx_builder.fee_rate(bdk_feerate);
        tx_builder
    }

    /// Sign a [`PartiallySignedTransaction`] in the default way.
    fn default_sign_psbt(
        wallet: &bdk29::Wallet<WalletDb>,
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
    ) -> Result<(), bdk29::Error> {
        match message {
            Some(msg) => info!("BDK sync progress: {progress}%, msg: {msg}"),
            None => info!("BDK sync progress: {progress}%"),
        }
        Ok(())
    }
}
