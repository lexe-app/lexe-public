use std::time::SystemTime;

use anyhow::anyhow;
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

use crate::types::Seed;

/// A thin wrapper around LDK's KeysManager which provides a cleaner init API
/// and some custom functionalities.
pub struct LexeKeysManager {
    inner: KeysManager,
}

impl LexeKeysManager {
    // TODO Replace this with a From<RootSeed> impl
    pub fn from_seed(seed: &Seed) -> Self {
        // FIXME(secure randomness): KeysManager::new() MUST be given a unique
        // `starting_time_secs` and `starting_time_nanos` for security. Since
        // secure timekeeping within an enclave is difficult, we should just
        // take a (securely) random u64, u32 instead. See KeysManager::new().
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Literally 1984");

        let inner = KeysManager::new(seed, now.as_secs(), now.subsec_nanos());

        Self { inner }
    }

    pub fn derive_pubkey(&self) -> anyhow::Result<PublicKey> {
        let privkey = self
            .inner
            .get_node_secret(Recipient::Node)
            .map_err(|()| anyhow!("Decode error: invalid value"))?;
        let mut secp = Secp256k1::new();
        secp.seeded_randomize(&self.inner.get_secure_random_bytes());
        let derived_pubkey = PublicKey::from_secret_key(&secp, &privkey);
        Ok(derived_pubkey)
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
