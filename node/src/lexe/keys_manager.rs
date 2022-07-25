use std::ops::Deref;
use std::sync::Arc;

use anyhow::ensure;
use bitcoin::blockdata::script::Script;
use bitcoin::blockdata::transaction::{Transaction, TxOut};
use bitcoin::secp256k1::{PublicKey, Secp256k1, Signing};
use common::rng::Crng;
use common::root_seed::RootSeed;
use lightning::chain::keysinterface::{
    KeysInterface, KeysManager, Recipient, SpendableOutputDescriptor,
};
use secrecy::ExposeSecret;

/// A thin wrapper around LDK's KeysManager which provides a cleaner init API
/// and some custom functionalities.
///
/// An Arc is held internally, so it is fine to clone directly.
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
    /// A helper used to (insecurely) initialize a LexeKeysManager in the
    /// temporary provision flow. Once provisioning works, this fn should be
    /// removed entirely. TODO: Remove
    pub fn unchecked_init<R: Crng>(rng: &mut R, root_seed: &RootSeed) -> Self {
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = Arc::new(KeysManager::new(
            root_seed.expose_secret(),
            random_secs,
            random_nanos,
        ));
        Self { inner }
    }

    /// Initialize a `LexeKeysManager` from a given [`RootSeed`]. Verifies that
    /// the derived node public matches `given_pubkey`.
    pub fn init<R: Crng>(
        rng: &mut R,
        given_pubkey: &PublicKey,
        root_seed: &RootSeed,
    ) -> anyhow::Result<Self> {
        // Build the inner KeysManager from the RootSeed.
        // NOTE: KeysManager::new() MUST be given a unique `starting_time_secs`
        // and `starting_time_nanos` for security. Since secure timekeeping
        // within an enclave is difficult, we just take a (secure) random u64,
        // u32 instead. See KeysManager::new() for more info.
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = Arc::new(KeysManager::new(
            root_seed.expose_secret(),
            random_secs,
            random_nanos,
        ));

        // Construct the LexeKeysManager, but validation isn't done yet
        let keys_manager = Self { inner };

        // Derive the pubkey from the inner KeysManager
        let derived_pubkey = keys_manager.derive_pubkey(rng);

        // Check the given pubkey against the derived one
        ensure!(
            given_pubkey == &derived_pubkey,
            "Derived pubkey doesn't match the given pubkey"
        );

        // Validation complete, finally return the LexeKeysManager
        Ok(keys_manager)
    }

    pub fn derive_pubkey<R: Crng>(&self, rng: &mut R) -> PublicKey {
        // Initialize and seed the Secp256k1 context with some random bytes for
        // some extra side-channel resistance.
        let mut secp_random_bytes = [0; 32];
        rng.fill_bytes(&mut secp_random_bytes);
        let mut secp = Secp256k1::new();
        secp.seeded_randomize(&secp_random_bytes);

        // Derive the public key from the private key.
        let privkey = self
            .inner
            .get_node_secret(Recipient::Node)
            .expect("Always succeeds when called with Recipient::Node");
        PublicKey::from_secret_key(&secp, &privkey)
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
