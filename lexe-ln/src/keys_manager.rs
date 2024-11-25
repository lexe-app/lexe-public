use anyhow::anyhow;
use bitcoin::{
    absolute,
    blockdata::transaction::{Transaction, TxOut},
    secp256k1::{
        ecdh, ecdsa, scalar::Scalar, schnorr, PublicKey, Secp256k1, Signing,
    },
};
use common::{
    api::user::NodePk,
    rng::{Crng, RngExt},
    root_seed::RootSeed,
};
use lightning::{
    ln::{
        msgs::{DecodeError, UnsignedGossipMessage},
        script::ShutdownScript,
    },
    offers::{
        invoice::UnsignedBolt12Invoice, invoice_request::UnsignedInvoiceRequest,
    },
    sign::{
        EntropySource, InMemorySigner, KeyMaterial, KeysManager, NodeSigner,
        OutputSpender, Recipient, SignerProvider, SpendableOutputDescriptor,
    },
};
use lightning_invoice::RawBolt11Invoice;
use secrecy::ExposeSecret;
use tracing::{debug, error};

use crate::wallet::LexeWallet;

/// Wraps LDK's [`KeysManager`] to provide the following:
///
/// 1) We have a simplified init API and a `get_node_pk` convenience method.
/// 2) Mirroring [ldk-node's implementation], we override
///    [`get_destination_script`] and [`get_shutdown_scriptpubkey`] so that LDK
///    gets addresses managed by BDK whenever it has the opportunity to close a
///    channel to a [`StaticOutput`] (usually (or only?) a cooperative close).
///    This allows us to avoid the on-chain fees incurred by a tx that sweeps
///    the output descriptors given to us in the [`SpendableOutputs`] event.
///
/// [ldk-node's implementation]: https://github.com/lightningdevkit/ldk-node/blob/3c7dac9d01ffdf66705b4a27ac699ab3d83c77f6/src/wallet.rs#L461-L484
/// [`get_destination_script`]: SignerProvider::get_destination_script.
/// [`get_shutdown_scriptpubkey`]: SignerProvider::get_shutdown_scriptpubkey.
/// [`StaticOutput`]: lightning::sign::SpendableOutputDescriptor::StaticOutput
/// [`SpendableOutputs`]: lightning::events::Event::SpendableOutputs
pub struct LexeKeysManager {
    inner: KeysManager,
    wallet: LexeWallet,
}

impl LexeKeysManager {
    /// Initialize a [`LexeKeysManager`] from a [`RootSeed`] and [`LexeWallet`].
    pub fn init(
        rng: &mut impl Crng,
        root_seed: &RootSeed,
        wallet: LexeWallet,
    ) -> anyhow::Result<Self> {
        // Build the KeysManager from the LDK seed derived from the root seed
        let ldk_seed = root_seed.derive_ldk_seed(rng);

        // KeysManager requires a "starting_time_secs" and "starting_time_nanos"
        // to seed an CRNG. We just provide random values from our system CRNG.
        let random_secs = rng.gen_u64();
        let random_nanos = rng.gen_u32();
        let inner = KeysManager::new(
            ldk_seed.expose_secret(),
            random_secs,
            random_nanos,
        );

        Ok(Self { inner, wallet })
    }

    pub fn get_node_pk(&self) -> NodePk {
        self.inner
            .get_node_id(Recipient::Node)
            .map(NodePk)
            .expect("Always succeeds when called with Recipient::Node")
    }

    /// Overrides [`KeysManager::spend_spendable_outputs`] so that we don't try
    /// to spend any [`StaticOutput`]s given to us in the `descriptors`
    /// parameter, since these are already managed by BDK.
    ///
    /// Based off of [ldk-node's implementation].
    ///
    /// [`StaticOutput`]: lightning::sign::SpendableOutputDescriptor::StaticOutput
    /// [ldk-node's implementation]: https://github.com/lightningdevkit/ldk-node/blob/3c7dac9d01ffdf66705b4a27ac699ab3d83c77f6/src/wallet.rs#L361-L378
    pub fn spend_spendable_outputs<C: Signing>(
        &self,
        descriptors: &[&SpendableOutputDescriptor],
        outputs: Vec<TxOut>,
        change_destination_script: bitcoin::ScriptBuf,
        feerate_sat_per_1000_weight: u32,
        maybe_locktime: Option<absolute::LockTime>,
        secp_ctx: &Secp256k1<C>,
    ) -> anyhow::Result<Option<Transaction>> {
        let num_outputs = descriptors.len();
        debug!("spend_spendable_outputs spending {num_outputs} outputs");
        let only_non_static = descriptors
            .iter()
            .filter(|d| {
                if matches!(d, SpendableOutputDescriptor::StaticOutput { .. }) {
                    debug!("Skipping StaticOutput");
                    false
                } else {
                    true
                }
            })
            .copied()
            .collect::<Vec<_>>();

        if only_non_static.is_empty() {
            debug!("spend_spendable_outputs: No non-static outputs to spend");
            return Ok(None);
        }

        self.inner
            .spend_spendable_outputs(
                &only_non_static,
                outputs,
                change_destination_script,
                feerate_sat_per_1000_weight,
                maybe_locktime,
                secp_ctx,
            )
            .map(Some)
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
    ) -> Result<ecdh::SharedSecret, ()> {
        self.inner.ecdh(recipient, other_key, tweak)
    }

    fn sign_invoice(
        &self,
        invoice: &RawBolt11Invoice,
        recipient: Recipient,
    ) -> Result<ecdsa::RecoverableSignature, ()> {
        self.inner.sign_invoice(invoice, recipient)
    }

    fn sign_bolt12_invoice_request(
        &self,
        invoice_request: &UnsignedInvoiceRequest,
    ) -> Result<schnorr::Signature, ()> {
        self.inner.sign_bolt12_invoice_request(invoice_request)
    }

    fn sign_bolt12_invoice(
        &self,
        invoice: &UnsignedBolt12Invoice,
    ) -> Result<schnorr::Signature, ()> {
        self.inner.sign_bolt12_invoice(invoice)
    }

    fn sign_gossip_message(
        &self,
        msg: UnsignedGossipMessage<'_>,
    ) -> Result<ecdsa::Signature, ()> {
        self.inner.sign_gossip_message(msg)
    }
}

impl SignerProvider for LexeKeysManager {
    type EcdsaSigner = InMemorySigner;

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
    ) -> Self::EcdsaSigner {
        self.inner
            .derive_channel_signer(channel_value_satoshis, channel_keys_id)
    }

    fn read_chan_signer(
        &self,
        reader: &[u8],
    ) -> Result<Self::EcdsaSigner, DecodeError> {
        self.inner.read_chan_signer(reader)
    }

    // TODO(max): LDK says we should return a different script each time
    // this is called, but address reuse here should be fairly rare.
    fn get_destination_script(
        &self,
        _channel_keys_id: [u8; 32],
    ) -> Result<bitcoin::ScriptBuf, ()> {
        // Use an internal address so we only have to watch it once.
        let sweep_address = self.wallet.get_internal_address();
        let destination_script = sweep_address.script_pubkey();
        Ok(destination_script)
    }

    // TODO(max): LDK says we should return a different script each time
    // this is called, but address reuse here should be fairly rare.
    fn get_shutdown_scriptpubkey(&self) -> Result<ShutdownScript, ()> {
        // Use an internal address so we only have to watch it once.
        let sweep_address = self.wallet.get_internal_address();
        let witness_program = sweep_address
            .witness_program()
            .ok_or_else(|| error!("Sweep address wasn't segwit address!"))?;
        let shutdown_scriptpubkey =
            ShutdownScript::new_witness_program(&witness_program)
                .inspect_err(|e| error!("Invalid shutdown script: {e:?}"))
                .map_err(|_| ())?;
        Ok(shutdown_scriptpubkey)
    }
}

#[cfg(test)]
mod test {
    use common::rng::WeakRng;
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    /// Tests that [`RootSeed::derive_node_pk`] generates the same [`NodePk`]
    /// that [`KeysManager::get_node_id`] does.
    #[test]
    fn test_rootseed_keysmanager_derivation_equivalence() {
        proptest!(|(
            root_seed in any::<RootSeed>(),
            mut rng in any::<WeakRng>()
        )| {
            let root_seed_node_pk = root_seed.derive_node_pk(&mut rng);

            let keys_manager = KeysManager::new(
                root_seed.derive_ldk_seed(&mut rng).expose_secret(),
                rng.gen_u64(),
                rng.gen_u32(),
            );
            let keys_manager_node_pk = keys_manager
                .get_node_id(Recipient::Node)
                .map(NodePk)
                .expect("Always succeeds when called with Recipient::Node");
            prop_assert_eq!(root_seed_node_pk, keys_manager_node_pk);
        });
    }
}
