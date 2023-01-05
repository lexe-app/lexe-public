use std::sync::{Arc, Mutex};

use anyhow::Context;
use bdk::template::Bip84;
use bdk::wallet::Wallet;
use bdk::KeychainKind;
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
mod db;

/// A newtype wrapper around [`bdk::Wallet`]. Can be cloned and used directly.
// The Mutex is needed because bdk::Wallet isn't thread-safe. bdk::Wallet::new
// internally wraps the db we provide with a RefCell, which isn't Send. Thus, to
// convince the compiler that LexeWallet is indeed Send, we wrap the bdk::Wallet
// with a Mutex, despite the fact that we don't technically need the Mutex since
// we don't use any bdk::Wallet methods that require &mut self.
#[derive(Clone)]
pub struct LexeWallet(Arc<Mutex<Wallet<WalletDb>>>);

impl LexeWallet {
    /// Constructs a new [`LexeWallet`] from a [`RootSeed`]. Wallet addresses
    /// are generated according to the [BIP 84] standard. See also [BIP 44].
    /// Additionally returns a handle to the underlying `WalletDb`.
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    pub fn init(
        root_seed: &RootSeed,
        network: Network,
        wallet_db_persister_tx: mpsc::Sender<()>,
    ) -> anyhow::Result<(Self, WalletDb)> {
        let network = network.into_inner();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        // TODO(max): Deserialize from persisted wallet db
        let wallet_db = WalletDb::new(wallet_db_persister_tx);

        let inner = Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db.clone(),
        )
        .context("bdk::Wallet::new failed")?;

        let wallet = Self(Arc::new(Mutex::new(inner)));

        Ok((wallet, wallet_db))
    }
}

/// Spawns a task that persists the current `WalletDb` state whenever it
/// receives a notification (via the `wallet_db_persister_rx` channel) that the
/// `WalletDb` needs to be re-persisted.
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

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;

    use super::*;

    #[test]
    fn all_root_seeds_form_valid_wallet() {
        let any_root_seed = any::<RootSeed>();
        let any_network = any::<Network>();
        proptest!(|(root_seed in any_root_seed, network in any_network)| {
            let (tx, _rx) = mpsc::channel(1);
            LexeWallet::init(&root_seed, network, tx).unwrap();
        })
    }
}
