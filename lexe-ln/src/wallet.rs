use anyhow::Context;
use bdk::database::memory::MemoryDatabase;
use bdk::template::Bip84;
use bdk::wallet::Wallet;
use bdk::KeychainKind;
use bitcoin::Network;
use common::root_seed::RootSeed;

/// A newtype wrapper around [`bdk::Wallet`].
pub struct LexeWallet(Wallet<MemoryDatabase>);

impl LexeWallet {
    /// Constructs a new [`LexeWallet`] from a [`RootSeed`]. Wallet addresses
    /// are generated according to the [BIP 84] standard. See also [BIP 44].
    ///
    /// [BIP 84]: https://github.com/bitcoin/bips/blob/master/bip-0084.mediawiki
    /// [BIP 44]: https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki
    pub fn new(root_seed: &RootSeed, network: Network) -> anyhow::Result<Self> {
        let master_xprv = root_seed.derive_master_xprv(network);

        // Descriptor for external (receive) addresses: `m/84h/{0,1}h/0h/0/*`
        let external_descriptor = Bip84(master_xprv, KeychainKind::External);
        // Descriptor for internal (change) addresses: `m/84h/{0,1}h/0h/1/*`
        let change_descriptor = Bip84(master_xprv, KeychainKind::Internal);

        // In-memory wallet database
        let wallet_db = MemoryDatabase::new();

        let inner = Wallet::new(
            external_descriptor,
            Some(change_descriptor),
            network,
            wallet_db,
        )
        .context("bdk::Wallet::new failed")?;

        Ok(Self(inner))
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;

    use super::*;

    #[test]
    fn all_root_seeds_form_valid_wallet() {
        let any_root_seed = any::<RootSeed>();
        let any_network = any::<common::cli::Network>();
        proptest!(|(root_seed in any_root_seed, network in any_network)| {
            LexeWallet::new(&root_seed, network.into_inner()).unwrap();
        })
    }
}
