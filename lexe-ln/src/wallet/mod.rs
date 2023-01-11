use std::sync::Arc;

use anyhow::Context;
use bdk::blockchain::EsploraBlockchain;
use bdk::template::Bip84;
use bdk::wallet::{AddressIndex, Wallet};
use bdk::{KeychainKind, SyncOptions};
use bitcoin::util::address::Address;
use common::cli::Network;
use common::constants::{
    IMPORTANT_PERSIST_RETRIES, SINGLETON_DIRECTORY, WALLET_DB_FILENAME,
};
use common::root_seed::RootSeed;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::traits::LexePersister;
use crate::wallet::db::WalletDb;

/// Wallet DB.
pub mod db;

/// The 'stop_gap' parameter used by BDK's wallet sync. This seems to configure
/// the threshold number of blocks after which BDK stops looking for scripts
/// belonging to the wallet. BDK's default value for this is 20.
const BDK_WALLET_SYNC_STOP_GAP: usize = 20;

/// The maximum number of concurrent requests that can be made against the
/// Esplora API provider.
const BDK_WALLET_SYNC_CONCURRENCY: u8 = 8;

/// A newtype wrapper around [`bdk::Wallet`]. Can be cloned and used directly.
// The Mutex is needed because bdk::Wallet isn't thread-safe. bdk::Wallet::new
// internally wraps the db we provide with a RefCell, which isn't Send. Thus, to
// convince the compiler that LexeWallet is indeed Send, we wrap the bdk::Wallet
// with a Mutex, despite the fact that we don't technically need the Mutex since
// we don't use any bdk::Wallet methods that require &mut self. Furthermore,
// since a lock to the bdk::Wallet needs to be held while awaiting on BDK wallet
// sync, the Mutex we use must be a Tokio mutex.
#[derive(Clone)]
pub struct LexeWallet(Arc<tokio::sync::Mutex<Wallet<WalletDb>>>);

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
        wallet_db: WalletDb,
    ) -> anyhow::Result<Self> {
        let network = network.into_inner();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        let inner = Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db,
        )
        .context("bdk::Wallet::new failed")?;

        Ok(Self(Arc::new(tokio::sync::Mutex::new(inner))))
    }

    /// Syncs the inner [`bdk::Wallet`] using the given Esplora server.
    ///
    /// NOTE: Beware deadlocks; this function holds a lock to the inner
    /// [`bdk::Wallet`] during wallet sync. It is held across `.await`.
    pub async fn sync(&self, esplora_url: &str) -> anyhow::Result<()> {
        let esplora_client =
            EsploraBlockchain::new(esplora_url, BDK_WALLET_SYNC_STOP_GAP)
                .with_concurrency(BDK_WALLET_SYNC_CONCURRENCY);

        // No need to hear about sync progress for now
        let sync_options = SyncOptions { progress: None };

        self.0
            .lock()
            .await
            .sync(&esplora_client, sync_options)
            .await
            .context("bdk::Wallet::sync failed")
    }

    /// Returns a new address derived using the external descriptor.
    pub fn get_new_address(&self) -> anyhow::Result<Address> {
        self.0
            .try_lock()
            .context("Wallet is busy; perhaps it is still syncing?")?
            .get_address(AddressIndex::New)
            .map(|info| info.address)
            .context("Could not get new address")
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
                            .persist_basic_file(
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
