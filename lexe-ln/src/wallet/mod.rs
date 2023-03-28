use std::sync::Arc;

use anyhow::{ensure, Context};
use bdk::blockchain::EsploraBlockchain;
use bdk::template::Bip84;
use bdk::wallet::signer::SignOptions;
use bdk::wallet::{AddressIndex, Wallet};
use bdk::{Balance, KeychainKind, SyncOptions};
use bitcoin::util::address::Address;
use bitcoin::{Script, Transaction};
use common::cli::Network;
use common::constants::{
    IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
};
use common::root_seed::RootSeed;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::chain::chaininterface::ConfirmationTarget;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::esplora::LexeEsplora;
use crate::traits::LexePersister;
use crate::wallet::db::WalletDb;

/// Wallet DB.
pub mod db;

/// The 'stop_gap' parameter used by BDK's wallet sync. This seems to configure
/// the threshold number of blocks after which BDK stops looking for scripts
/// belonging to the wallet. BDK's default value for this is 20.
const BDK_WALLET_SYNC_STOP_GAP: usize = 20;

/// A newtype wrapper around [`bdk::Wallet`]. Can be cloned and used directly.
#[derive(Clone)]
pub struct LexeWallet {
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
    pub async fn sync(&self) -> anyhow::Result<()> {
        let esplora_blockchain = EsploraBlockchain::from_client(
            self.esplora.client().clone(),
            BDK_WALLET_SYNC_STOP_GAP,
        );

        // No need to hear about sync progress for now
        let sync_options = SyncOptions { progress: None };

        self.wallet
            .lock()
            .await
            .sync(&esplora_blockchain, sync_options)
            .await
            .context("bdk::Wallet::sync failed")
    }

    /// Returns the current wallet balance. Note that newly received funds will
    /// not be detected unless the wallet has been `sync()`ed first.
    // NOTE: We use lock().await as a hack to avoid the problematic try_lock().
    // TODO(max): Change back to sync once BDK is robust to concurrent access.
    pub async fn get_balance(&self) -> anyhow::Result<Balance> {
        self.wallet
            .lock()
            .await
            .get_balance()
            .context("Could not get balance")
    }

    /// Returns a new address derived using the external descriptor.
    // NOTE: We use lock().await as a hack to avoid the problematic try_lock().
    // TODO(max): Change back to sync once BDK is robust to concurrent access.
    pub async fn get_new_address(&self) -> anyhow::Result<Address> {
        self.wallet
            .lock()
            .await
            .get_address(AddressIndex::New)
            .map(|info| info.address)
            .context("Could not get new address")
    }

    /// Create and sign a funding tx given an output script, channel value, and
    /// confirmation target. Intended to be called downstream of an
    /// [`FundingGenerationReady`] event
    ///
    /// [`FundingGenerationReady`]: lightning::util::events::Event::FundingGenerationReady
    // NOTE: We use lock().await as a hack to avoid the problematic try_lock().
    // TODO(max): Change back to sync once BDK is robust to concurrent access.
    pub(crate) async fn create_and_sign_funding_tx(
        &self,
        output_script: Script,
        channel_value_satoshis: u64,
        conf_target: ConfirmationTarget,
    ) -> anyhow::Result<Transaction> {
        let locked_wallet = self.wallet.lock().await;
        let bdk_feerate = self.esplora.get_bdk_feerate(conf_target);

        let mut tx_builder = locked_wallet.build_tx();
        tx_builder
            .add_recipient(output_script, channel_value_satoshis)
            .fee_rate(bdk_feerate)
            .enable_rbf();
        let (mut psbt, _tx_deets) = tx_builder
            .finish()
            .context("Could not build funding PSBT")?;

        // Sign and extract the raw tx.
        let sign_options = SignOptions::default();
        let finalized = locked_wallet
            .sign(&mut psbt, sign_options)
            .context("Could not sign funding PSBT")?;
        ensure!(finalized, "Failed to sign all PSBT inputs");
        let raw_tx = psbt.extract_tx();

        Ok(raw_tx)
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
