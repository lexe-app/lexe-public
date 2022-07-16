use std::time::SystemTime;

use anyhow::ensure;
use bitcoin::blockdata::script::Script;
use bitcoin::blockdata::transaction::{Transaction, TxOut};
use bitcoin::secp256k1::ecdsa::RecoverableSignature;
use bitcoin::secp256k1::{PublicKey, Secp256k1, SecretKey, Signing};
use bitcoin_bech32::u5;
use lightning::chain::keysinterface::{
    InMemorySigner, KeyMaterial, KeysInterface, KeysManager, Recipient,
    SpendableOutputDescriptor,
};
use lightning::ln::msgs::DecodeError;
use lightning::ln::script::ShutdownScript;

use crate::api::{Enclave, Node};
use crate::convert;
use crate::types::Seed;

/// A thin wrapper around LDK's KeysManager which provides a cleaner init API
/// and some custom functionalities.
pub struct LexeKeysManager {
    inner: KeysManager,
}

impl TryFrom<(Node, Enclave)> for LexeKeysManager {
    type Error = anyhow::Error;

    fn try_from(node_enclave: (Node, Enclave)) -> anyhow::Result<Self> {
        let (node, enclave) = node_enclave;

        // TODO(decrypt): Decrypt under enclave sealing key to get the seed
        let seed = enclave.seed;

        // Validate the seed
        ensure!(seed.len() == 32, "Incorrect seed length");
        let mut seed_buf = [0; 32];
        seed_buf.copy_from_slice(&seed);

        // Build the inner KeysManager from the validated seed
        let inner = helpers::get_inner_from_seed(&seed_buf);

        // Derive a pubkey from the inner KeysManager
        let derived_pubkey = helpers::get_pubkey_from_inner(&inner);

        // Deserialize the pubkey returned from the DB (given pubkey)
        let given_pubkey_hex = node.public_key;
        let given_pubkey = convert::pubkey_from_hex(&given_pubkey_hex)?;

        // Check the given pubkey against the derived one
        ensure!(
            given_pubkey == derived_pubkey,
            "Derived pubkey doesn't match the pubkey returned from the DB"
        );

        // Check the hex encodings as well
        let derived_pubkey_hex = convert::pubkey_to_hex(&derived_pubkey);
        ensure!(
            given_pubkey_hex == derived_pubkey_hex,
            "Derived pubkey string doesn't match given pubkey string"
        );

        // Validation complete, return Self
        Ok(Self { inner })
    }
}

impl LexeKeysManager {
    // TODO Replace this with a From<RootSeed> impl
    /// Builds a new LexeKeysManager from a given seed. Since this does not
    /// validate the seed against data returned from the backend, this function
    /// should only be called from within the provisioning flow.
    pub fn from_seed(seed: &Seed) -> Self {
        let inner = helpers::get_inner_from_seed(seed);
        Self { inner }
    }

    pub fn derive_pubkey(&self) -> PublicKey {
        helpers::get_pubkey_from_inner(&self.inner)
    }

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

/// Helper fns which can be called without requiring that a LexeKeysManager is
/// initialized
mod helpers {
    use super::*;

    pub(super) fn get_inner_from_seed(seed: &Seed) -> KeysManager {
        // FIXME(secure randomness): KeysManager::new() MUST be given a unique
        // `starting_time_secs` and `starting_time_nanos` for security. Since
        // secure timekeeping within an enclave is difficult, we should just
        // take a (securely) random u64, u32 instead. See KeysManager::new().
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time went backwards");

        KeysManager::new(seed, now.as_secs(), now.subsec_nanos())
    }

    pub(super) fn get_pubkey_from_inner(inner: &KeysManager) -> PublicKey {
        let privkey = inner
            .get_node_secret(Recipient::Node)
            .expect("Always succeeds when called with Recipient::Node");
        let mut secp = Secp256k1::new();
        secp.seeded_randomize(&inner.get_secure_random_bytes());
        PublicKey::from_secret_key(&secp, &privkey)
    }
}

impl KeysInterface for LexeKeysManager {
    type Signer = InMemorySigner;

    fn get_node_secret(&self, recipient: Recipient) -> Result<SecretKey, ()> {
        self.inner.get_node_secret(recipient)
    }

    fn get_destination_script(&self) -> Script {
        self.inner.get_destination_script()
    }

    fn get_shutdown_scriptpubkey(&self) -> ShutdownScript {
        self.inner.get_shutdown_scriptpubkey()
    }

    fn get_channel_signer(
        &self,
        inbound: bool,
        channel_value_satoshis: u64,
    ) -> Self::Signer {
        self.inner
            .get_channel_signer(inbound, channel_value_satoshis)
    }

    fn get_secure_random_bytes(&self) -> [u8; 32] {
        self.inner.get_secure_random_bytes()
    }

    fn read_chan_signer(
        &self,
        reader: &[u8],
    ) -> Result<Self::Signer, DecodeError> {
        self.inner.read_chan_signer(reader)
    }

    fn sign_invoice(
        &self,
        hrp_bytes: &[u8],
        invoice_data: &[u5],
        recipient: Recipient,
    ) -> Result<RecoverableSignature, ()> {
        self.inner.sign_invoice(hrp_bytes, invoice_data, recipient)
    }

    fn get_inbound_payment_key_material(&self) -> KeyMaterial {
        self.inner.get_inbound_payment_key_material()
    }
}
