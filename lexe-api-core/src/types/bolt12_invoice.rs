use lexe_serde::base64_or_bytes;
use lexe_std::const_assert_mem_size;
use lightning::{
    offers::invoice::Bolt12Invoice as LdkBolt12Invoice, util::ser::Writeable,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A BOLT12 invoice, sent by the payee in response to an invoice request for
/// an [`Offer`].
///
/// Serialized as base64; LDK exposes only raw [`Writeable`] bytes.
///
/// [`Offer`]: super::offer::Offer
#[derive(Debug, Eq, PartialEq)]
pub struct Bolt12Invoice(pub LdkBolt12Invoice);

const_assert_mem_size!(Bolt12Invoice, 1616);

impl From<LdkBolt12Invoice> for Bolt12Invoice {
    #[inline]
    fn from(value: LdkBolt12Invoice) -> Self {
        Self(value)
    }
}

impl Serialize for Bolt12Invoice {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        base64_or_bytes::serialize(self.0.encode(), serializer)
    }
}

impl<'de> Deserialize<'de> for Bolt12Invoice {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        let bytes = base64_or_bytes::deserialize::<D, Vec<u8>>(deserializer)?;
        LdkBolt12Invoice::try_from(bytes)
            .map(Self)
            // `Bolt12ParseError` doesn't impl `Display`.
            .map_err(|e| de::Error::custom(format!("Invalid BOLT12: {e:?}")))
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub(crate) mod arb {
    use std::{ops::RangeInclusive, time::Duration};

    use bitcoin::secp256k1::{Keypair, PublicKey};
    use lexe_common::{
        ln::amount::Amount, rng::FastRngDerefHack, root_seed::RootSeed,
        secp256k1_ctx::SECP256K1, test_utils::arbitrary,
    };
    use lexe_crypto::rng::{FastRng, RngExt};
    use lightning::{
        blinded_path::{
            BlindedHop,
            payment::{BlindedPayInfo, BlindedPaymentPath},
        },
        ln::{channelmanager::PaymentId, inbound_payment::ExpandedKey},
        offers::{
            invoice::UnsignedBolt12Invoice,
            invoice_request::InvoiceRequestVerifiedFromOffer, nonce::Nonce,
            offer::Offer as LdkOffer,
        },
        onion_message::dns_resolution::HumanReadableName,
        types::{
            features::BlindedHopFeatures,
            payment::{PaymentHash, PaymentPreimage},
        },
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        collection, option,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;
    use crate::types::offer::{Offer, arb::OfferParams};

    /// [`proptest`] parameters for generating a [`Bolt12Invoice`].
    #[derive(Default)]
    pub struct Bolt12InvoiceParams {
        /// The preimage of the invoice's payment hash.
        ///
        /// If not given, an arbitrary preimage is generated.
        pub payment_preimage: Option<PaymentPreimage>,
        /// The payer's key material and payment id, which together re-derive
        /// the payer signing key. Needed to build a `PayerProof` for the
        /// generated invoice.
        ///
        /// If not given, arbitrary key material is generated.
        pub payer_keys: Option<(ExpandedKey, PaymentId)>,
    }

    impl Arbitrary for Bolt12Invoice {
        type Parameters = Bolt12InvoiceParams;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            let Bolt12InvoiceParams {
                payment_preimage,
                payer_keys,
            } = args;
            any::<FastRng>()
                .prop_flat_map(move |mut rng| {
                    // Generate an arbitrary offer using our own root_seed and
                    // key materials
                    let root_seed = RootSeed::from_rng(&mut rng);
                    let payee_node_keypair = root_seed.derive_node_key_pair();
                    let payee_nonce = Nonce::from_entropy_source(
                        FastRngDerefHack::from_rng(&mut rng),
                    );
                    let payee_expanded_key = ExpandedKey::new(rng.gen_bytes());
                    let is_derived = rng.gen_boolean();
                    let derived_keys = Some(
                        is_derived.then_some((payee_nonce, payee_expanded_key)),
                    );
                    let params = OfferParams {
                        root_seed: Some(root_seed),
                        derived_keys,
                    };
                    (
                        // Caller-injected params
                        Just(payment_preimage),
                        Just(payer_keys),
                        // Offer params
                        Just(rng),
                        Just(payee_node_keypair),
                        Just(payee_nonce),
                        Just(payee_expanded_key),
                        any_with::<Offer>(params).prop_map(|offer| offer.0),
                        // Invoice params
                        any::<Amount>(),
                        option::of(any_hrn()),
                        arbitrary::any_option_string(),
                        arbitrary::any_duration_secs(),
                    )
                })
                .prop_map(
                    |(
                        payment_preimage,
                        payer_keys,
                        rng,
                        payee_node_keypair,
                        payee_nonce,
                        payee_expanded_key,
                        offer,
                        amount,
                        hrn,
                        payer_note,
                        created_at,
                    )| {
                        gen_bolt12_invoice(
                            payment_preimage,
                            payer_keys,
                            rng,
                            payee_node_keypair,
                            payee_nonce,
                            payee_expanded_key,
                            offer,
                            amount,
                            hrn,
                            payer_note,
                            created_at,
                        )
                    },
                )
                .boxed()
        }
    }

    /// To create and sign a BOLT12 invoice in LDK:
    /// 1. Create an LDK [`Offer`]
    /// 2. [`Offer::request_invoice`] yields [`InvoiceRequest`]
    /// 3. [`InvoiceRequest::respond_with_no_std`] yields
    ///    [`UnsignedBolt12Invoice`]
    /// 4. [`UnsignedBolt12Invoice::sign`] yields [`Bolt12Invoice`]
    ///
    /// [`Offer`]: lightning::offers::offer::Offer
    /// [`Offer::request_invoice`]: lightning::offers::offer::Offer::request_invoice
    /// [`InvoiceRequest`]: lightning::offers::invoice_request::InvoiceRequest
    /// [`InvoiceRequest::respond_with_no_std`]: lightning::offers::invoice_request::InvoiceRequest::respond_with_no_std
    /// [`Bolt12Invoice`]: lightning::offers::invoice::Bolt12Invoice
    fn gen_bolt12_invoice(
        payment_preimage: Option<PaymentPreimage>,
        payer_keys: Option<(ExpandedKey, PaymentId)>,
        mut rng: FastRng,
        payee_node_keypair: Keypair,
        payee_nonce: Nonce,
        payee_expanded_key: ExpandedKey,
        offer: LdkOffer,
        amount: Amount,
        hrn: Option<HumanReadableName>,
        payer_note: Option<String>,
        created_at: Duration,
    ) -> Bolt12Invoice {
        // --- Build invoice request from offer --- //

        let payer_nonce =
            Nonce::from_entropy_source(FastRngDerefHack::from_rng(&mut rng));
        let (payer_expanded_key, payment_id) =
            payer_keys.unwrap_or_else(|| {
                let expanded_key = ExpandedKey::new(rng.gen_bytes());
                let id = PaymentId(rng.gen_bytes());
                (expanded_key, id)
            });

        let mut request = offer
            .request_invoice(
                &payer_expanded_key,
                payer_nonce,
                &SECP256K1,
                payment_id,
            )
            .expect("Failed to build invoice request");

        let chains = offer.chains();
        let chain = chains[rng.gen_range_usize(0..chains.len())];
        let network = bitcoin::Network::try_from(chain)
            .expect("Offer supports an unknown chain");
        request = request
            .chain(network)
            .expect("Failed to set invoice request chain");

        if offer.amount().is_none() {
            // The payer cannot pay a 0 amount.
            let amount_msats = amount.msat().max(1);
            request = request
                .amount_msats(amount_msats)
                .expect("Failed to set invoice request amount");
        }
        if offer.expects_quantity() {
            request = request
                .quantity(1)
                .expect("Failed to set invoice request quantity");
        }
        if let Some(hrn) = hrn {
            request = request.sourced_from_human_readable_name(hrn);
        }
        if let Some(payer_note) = payer_note {
            request = request.payer_note(payer_note);
        }
        let request = request
            .build_and_sign()
            .expect("Failed to sign invoice request");

        // --- Build invoice from invoice request --- //

        let payment_paths = vec![payment_path(payee_node_keypair.public_key())];
        let payment_preimage = payment_preimage
            .unwrap_or_else(|| PaymentPreimage(rng.gen_bytes()));
        let payment_hash = PaymentHash::from(payment_preimage);
        // The invoice uses a different key based on how the `Offer` was built.
        // See `OfferBuilder::deriving_signing_pubkey` docs for details.
        //
        // In practice, LDK signs with either the payee's node keys or a
        // per-offer derived key, so we match on `verify_using_recipient_data`
        // to find out which one this offer calls for.
        let verified = request.clone().verify_using_recipient_data(
            payee_nonce,
            &payee_expanded_key,
            &SECP256K1,
        );
        let invoice = match verified {
            // Case: The offer's signing pubkey was derived from our key
            //       material, so LDK signs with the keys it re-derived.
            Ok(InvoiceRequestVerifiedFromOffer::DerivedKeys(verified)) =>
                verified
                    .respond_using_derived_keys_no_std(
                        payment_paths,
                        payment_hash,
                        created_at,
                    )
                    .expect("Failed to respond to invoice request")
                    .build_and_sign(&SECP256K1)
                    .expect("Failed to sign invoice"),
            // Case: Should be unreachable from `verify_using_recipient_data`.
            Ok(InvoiceRequestVerifiedFromOffer::ExplicitKeys(_)) =>
                panic!("Expected recipient data to verify to derived keys"),
            // Case: Failed to verify using recipient data, meaning the signing
            //       pubkey was set to the `node_pk`.
            Err(()) => {
                let unsigned_invoice = request
                    .respond_with_no_std(
                        payment_paths,
                        payment_hash,
                        created_at,
                    )
                    .expect("Failed to respond to invoice request")
                    .build()
                    .expect("Failed to build invoice");
                // Verify that we are signing with the issuer signing pubkey.
                assert_eq!(
                    unsigned_invoice.signing_pubkey(),
                    payee_node_keypair.public_key(),
                );
                unsigned_invoice
                    .sign(|invoice: &UnsignedBolt12Invoice| {
                        let digest = invoice.as_ref().as_digest();
                        Ok(SECP256K1.sign_schnorr_no_aux_rand(
                            digest,
                            &payee_node_keypair,
                        ))
                    })
                    .expect("Failed to sign invoice")
            }
        };

        Bolt12Invoice(invoice)
    }

    /// A BIP 353 name, e.g. `satoshi@lexe.app`. Each dot-separated label must
    /// be a non-empty, valid hostname of at most 63 characters.
    fn any_hrn() -> impl Strategy<Value = HumanReadableName> {
        static LABEL_CHARS: &[RangeInclusive<char>] = &['0'..='9', 'a'..='z'];
        let any_label = || {
            collection::vec(proptest::char::ranges(LABEL_CHARS.into()), 1..=16)
                .prop_map(String::from_iter)
        };

        (any_label(), any_label(), any_label()).prop_map(
            |(user, domain, tld)| {
                HumanReadableName::new(&user, &format!("{domain}.{tld}"))
                    .expect("Generated an invalid human readable name")
            },
        )
    }

    /// A dummy blinded path; an invoice requires at least one, but nothing
    /// here is ever routed over.
    // TODO(nicole): make this more comprehensive
    fn payment_path(node_pk: PublicKey) -> BlindedPaymentPath {
        BlindedPaymentPath::from_blinded_path_and_payinfo(
            node_pk,
            node_pk,
            vec![BlindedHop {
                blinded_node_id: node_pk,
                encrypted_payload: Vec::new(),
            }],
            BlindedPayInfo {
                fee_base_msat: 0,
                fee_proportional_millionths: 0,
                cltv_expiry_delta: 0,
                htlc_minimum_msat: 0,
                htlc_maximum_msat: u64::MAX,
                features: BlindedHopFeatures::empty(),
            },
        )
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn invoice_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<Bolt12Invoice>();
    }
}
