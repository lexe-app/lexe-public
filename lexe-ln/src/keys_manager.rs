use std::ops::Deref;
use std::sync::Arc;

use anyhow::ensure;
use bitcoin::blockdata::script::Script;
use bitcoin::blockdata::transaction::{Transaction, TxOut};
use bitcoin::secp256k1::{self, Secp256k1, Signing};
use common::api::NodePk;
use common::rng::{self, Crng};
use common::root_seed::RootSeed;
use lightning::chain::keysinterface::{
    KeysInterface, KeysManager, Recipient, SpendableOutputDescriptor,
};
use secrecy::ExposeSecret;

/// A thin wrapper around LDK's KeysManager which provides a cleaner init API
/// and some custom functionalities.
///
/// An Arc is held internally, so it is fine to clone and use directly.
#[derive(Clone)]
pub struct LexeKeysManager {
    inner: Arc<KeysManager>,
}

impl Deref for LexeKeysManager {
    type Target = KeysManager;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref()
    }
}

impl LexeKeysManager {
    /// Initialize a [`LexeKeysManager`] from a [`RootSeed`] without supplying a
    /// pubkey to check the derived pubkey against.
    pub fn unchecked_init<R: Crng>(rng: &mut R, root_seed: &RootSeed) -> Self {
        let ldk_seed = root_seed.derive_ldk_seed(rng);
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = Arc::new(KeysManager::new(
            ldk_seed.expose_secret(),
            random_secs,
            random_nanos,
        ));
        Self { inner }
    }

    /// Initialize a `LexeKeysManager` from a given [`RootSeed`]. Verifies that
    /// the derived node public key matches `given_pk`.
    pub fn init<R: Crng>(
        rng: &mut R,
        given_pk: &NodePk,
        root_seed: &RootSeed,
    ) -> anyhow::Result<Self> {
        // Build the KeysManager from the LDK seed derived from the root seed
        let ldk_seed = root_seed.derive_ldk_seed(rng);

        // NOTE: KeysManager::new() MUST be given a unique `starting_time_secs`
        // and `starting_time_nanos` for security. Since secure timekeeping
        // within an enclave is difficult, we just take a (secure) random u64,
        // u32 instead. See KeysManager::new() for more info.
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = Arc::new(KeysManager::new(
            ldk_seed.expose_secret(),
            random_secs,
            random_nanos,
        ));

        // Construct the LexeKeysManager, but validation isn't done yet
        let keys_manager = Self { inner };

        // Derive the node_pk from the inner KeysManager
        let derived_pk = keys_manager.derive_node_pk(rng);

        // Check the given pk against the derived one
        ensure!(
            given_pk == &derived_pk,
            "Derived pk doesn't match the given pk"
        );

        // Validation complete, finally return the LexeKeysManager
        Ok(keys_manager)
    }

    pub fn derive_node_pk<R: Crng>(&self, rng: &mut R) -> NodePk {
        let secp_ctx = rng::get_randomized_secp256k1_ctx(rng);

        // Derive the public key from the private key.
        let privkey = self
            .inner
            .get_node_secret(Recipient::Node)
            .expect("Always succeeds when called with Recipient::Node");

        NodePk(secp256k1::PublicKey::from_secret_key(&secp_ctx, &privkey))
    }

    // Bad fn signature is inherited from LDK
    #[allow(clippy::result_unit_err)]
    pub fn spend_spendable_outputs<C: Signing>(
        &self,
        descriptors: &[&SpendableOutputDescriptor],
        outputs: Vec<TxOut>,
        change_destination_script: Script,
        feerate_sat_per_1000_weight: u32,
        secp_ctx: &Secp256k1<C>,
    ) -> Result<Transaction, ()> {
        self.inner.spend_spendable_outputs(
            descriptors,
            outputs,
            change_destination_script,
            feerate_sat_per_1000_weight,
            secp_ctx,
        )
    }
}

#[cfg(test)]
mod test {
    use common::rng::SmallRng;
    use proptest::arbitrary::any;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    /// Tests that [`RootSeed::derive_node_pk`] generates the same [`NodePk`]
    /// that [`LexeKeysManager::derive_node_pk`] does.
    #[test]
    fn test_rootseed_keysmanager_derivation_equivalence() {
        let any_root_seed = any::<RootSeed>();
        let any_rng = any::<SmallRng>();

        proptest!(|(root_seed in any_root_seed, mut rng in any_rng)| {
            let root_seed_node_pk = root_seed.derive_node_pk(&mut rng);

            let keys_manager =
                LexeKeysManager::unchecked_init(&mut rng, &root_seed);
            let keys_manager_node_pk = keys_manager.derive_node_pk(&mut rng);
            prop_assert_eq!(root_seed_node_pk, keys_manager_node_pk);
        });
    }
}
