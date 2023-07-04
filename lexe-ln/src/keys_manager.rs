use anyhow::{anyhow, ensure};
use bitcoin::{
    bech32::u5,
    blockdata::{
        script::Script,
        transaction::{Transaction, TxOut},
    },
    secp256k1::{
        ecdh::SharedSecret,
        ecdsa::{RecoverableSignature, Signature},
        scalar::Scalar,
        PublicKey, Secp256k1, Signing,
    },
};
use common::{api::NodePk, rng::Crng, root_seed::RootSeed};
use lightning::{
    chain::keysinterface::{
        EntropySource, InMemorySigner, KeyMaterial, KeysManager, NodeSigner,
        Recipient, SignerProvider, SpendableOutputDescriptor,
    },
    ln::{
        msgs::{DecodeError, UnsignedGossipMessage},
        script::ShutdownScript,
    },
};
use secrecy::ExposeSecret;

/// A thin wrapper around LDK's KeysManager which provides a cleaner init API
/// and some custom functionalities.
pub struct LexeKeysManager {
    inner: KeysManager,
}

impl LexeKeysManager {
    /// Initialize a [`LexeKeysManager`] from a [`RootSeed`] without supplying a
    /// pubkey to check the derived pubkey against.
    pub fn unchecked_init<R: Crng>(rng: &mut R, root_seed: &RootSeed) -> Self {
        let ldk_seed = root_seed.derive_ldk_seed(rng);
        // KeysManager requires a "starting_time_secs" and "starting_time_nanos"
        // to seed an CRNG. We just provide random values from our system CRNG.
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = KeysManager::new(
            ldk_seed.expose_secret(),
            random_secs,
            random_nanos,
        );
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

        // KeysManager requires a "starting_time_secs" and "starting_time_nanos"
        // to seed an CRNG. We just provide random values from our system CRNG.
        let random_secs = rng.next_u64();
        let random_nanos = rng.next_u32();
        let inner = KeysManager::new(
            ldk_seed.expose_secret(),
            random_secs,
            random_nanos,
        );

        // Construct the LexeKeysManager, but validation isn't done yet
        let keys_manager = Self { inner };

        // Derive the node_pk from the inner KeysManager
        let derived_pk = keys_manager.get_node_pk();

        // Check the given pk against the derived one
        ensure!(
            given_pk == &derived_pk,
            "Derived pk doesn't match the given pk"
        );

        // Validation complete, finally return the LexeKeysManager
        Ok(keys_manager)
    }

    pub fn get_node_pk(&self) -> NodePk {
        self.inner
            .get_node_id(Recipient::Node)
            .map(NodePk)
            .expect("Always succeeds when called with Recipient::Node")
    }

    pub fn spend_spendable_outputs<C: Signing>(
        &self,
        descriptors: &[&SpendableOutputDescriptor],
        outputs: Vec<TxOut>,
        change_destination_script: Script,
        feerate_sat_per_1000_weight: u32,
        secp_ctx: &Secp256k1<C>,
    ) -> anyhow::Result<Transaction> {
        self.inner
            .spend_spendable_outputs(
                descriptors,
                outputs,
                change_destination_script,
                feerate_sat_per_1000_weight,
                secp_ctx,
            )
            .map_err(|()| anyhow!("spend_spendable_outputs failed"))
    }
}

// --- LDK impls --- //

impl EntropySource for LexeKeysManager {
    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.inner.get_secure_random_bytes()
    }
}

impl NodeSigner for LexeKeysManager {
    fn get_inbound_payment_key_material(&self) -> KeyMaterial {
        self.inner.get_inbound_payment_key_material()
    }

    fn get_node_id(&self, recipient: Recipient) -> Result<PublicKey, ()> {
        self.inner.get_node_id(recipient)
    }

    fn ecdh(
        &self,
        recipient: Recipient,
        other_key: &PublicKey,
        tweak: Option<&Scalar>,
    ) -> Result<SharedSecret, ()> {
        self.inner.ecdh(recipient, other_key, tweak)
    }

    fn sign_invoice(
        &self,
        hrp_bytes: &[u8],
        invoice_data: &[u5],
        recipient: Recipient,
    ) -> Result<RecoverableSignature, ()> {
        self.inner.sign_invoice(hrp_bytes, invoice_data, recipient)
    }

    fn sign_gossip_message(
        &self,
        msg: UnsignedGossipMessage<'_>,
    ) -> Result<Signature, ()> {
        self.inner.sign_gossip_message(msg)
    }
}

impl SignerProvider for LexeKeysManager {
    type Signer = InMemorySigner;

    // Required methods
    fn generate_channel_keys_id(
        &self,
        inbound: bool,
        channel_value_satoshis: u64,
        user_channel_id: u128,
    ) -> [u8; 32] {
        self.inner.generate_channel_keys_id(
            inbound,
            channel_value_satoshis,
            user_channel_id,
        )
    }

    fn derive_channel_signer(
        &self,
        channel_value_satoshis: u64,
        channel_keys_id: [u8; 32],
    ) -> Self::Signer {
        self.inner
            .derive_channel_signer(channel_value_satoshis, channel_keys_id)
    }

    fn read_chan_signer(
        &self,
        reader: &[u8],
    ) -> Result<Self::Signer, DecodeError> {
        self.inner.read_chan_signer(reader)
    }

    fn get_destination_script(&self) -> Script {
        self.inner.get_destination_script()
    }

    fn get_shutdown_scriptpubkey(&self) -> ShutdownScript {
        self.inner.get_shutdown_scriptpubkey()
    }
}

#[cfg(test)]
mod test {
    use common::rng::WeakRng;
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    /// Tests that [`RootSeed::derive_node_pk`] generates the same [`NodePk`]
    /// that [`LexeKeysManager::get_node_pk`] does.
    #[test]
    fn test_rootseed_keysmanager_derivation_equivalence() {
        let any_root_seed = any::<RootSeed>();
        let any_rng = any::<WeakRng>();

        proptest!(|(root_seed in any_root_seed, mut rng in any_rng)| {
            let root_seed_node_pk = root_seed.derive_node_pk(&mut rng);

            let keys_manager =
                LexeKeysManager::unchecked_init(&mut rng, &root_seed);
            let keys_manager_node_pk = keys_manager.get_node_pk();
            prop_assert_eq!(root_seed_node_pk, keys_manager_node_pk);
        });
    }
}
