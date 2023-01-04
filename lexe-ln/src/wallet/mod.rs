use std::sync::{Arc, Mutex};

use anyhow::Context;
use bdk::template::Bip84;
use bdk::wallet::Wallet;
use bdk::KeychainKind;
use common::api::vfs::BasicFile;
use common::cli::Network;
use common::constants::IMPORTANT_PERSIST_RETRIES;
use common::root_seed::RootSeed;
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use tokio::sync::mpsc;
use tracing::{info, warn};

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
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    pub fn new(
        root_seed: &RootSeed,
        network: Network,
        wallet_db_persister_tx: mpsc::Sender<BasicFile>,
    ) -> anyhow::Result<Self> {
        let network = network.into_inner();
        let master_xprv = root_seed.derive_bip32_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        let wallet_db = WalletDb::new(wallet_db_persister_tx);

        let inner = Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db,
        )
        .context("bdk::Wallet::new failed")?;

        Ok(Self(Arc::new(Mutex::new(inner))))
    }
}

#[allow(unused)] // TODO(max): Remove
pub fn spawn_wallet_db_persister_task<PS: LexePersister>(
    persister: PS,
    mut wallet_db_persister_rx: mpsc::Receiver<BasicFile>,
    mut shutdown: ShutdownChannel,
) -> LxTask<()> {
    LxTask::spawn_named("wallet db persister", async move {
        loop {
            tokio::select! {
                Some(basic_file) = wallet_db_persister_rx.recv() => {
                    // TODO(max): Optimize; only persist the last one
                    let res = persister
                        .persist_basic_file(basic_file, IMPORTANT_PERSIST_RETRIES)
                        .await
                        .context("Could not persist wallet db");
                    if let Err(e) = res {
                        warn!("Wallet DB persist error: {e:#}");
                    }
                }
                () = shutdown.recv() =>
                    break info!("wallet db persister task shutting down"),
            }
        }
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
            LexeWallet::new(&root_seed, network, tx).unwrap();
        })
    }
}
