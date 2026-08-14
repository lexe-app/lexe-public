//! # BOLT12 payer proofs
//!
//! BOLT12 payer proofs give payers a way to prove that an offer was paid
//! while exposing a minimal amount of information about the BOLT12 invoice.
//!
//! A [`PayerProof`] is already validated, but verifiers of a `PayerProof`
//! should check the [`invoice_node_id`] (the key used to sign the paid BOLT12
//! invoice) to ensure that the payee is who they expect. For an offer created
//! by a Lexe node, this should match the offer's [`issuer_signing_pubkey`].
//!
//! Note that `invoice_node_id` may not match a node's consistent ID if a
//! blinded path was used.
//!
//! ## Payer proof fields
//!
//! ### Deterministic fields
//!
//! Fields which are deterministically populated for minimal proof validation.
//! - `invreq_payer_id`
//! - `invoice_payment_hash`
//! - `invoice_node_id`
//! - `invoice_features`
//! - `signature`
//! - `proof_preimage`
//! - `proof_signature`
//!
//! ### Configurable proof fields
//!
//! Fields configured when the proof was created.
//! - `proof_note`
//!
//! ### Disclosable invoice fields
//!
//! Optional fields revealing information about the paid BOLT12 invoice.
//! These fields were selected to be disclosed or omitted when the proof was
//! created.
//!
//! `None` means the field was withheld **or** the invoice never carried it.
//! A getter fails rather than clamping if the disclosed value isn't
//! representable in our types.
//!
//! - `offer_description`
//! - `offer_issuer`
//! - `invreq_payer_note`
//! - `invoice_amount`
//! - `invoice_created_at`
//! - ... and more
//!
//! [`PayerProof`]: crate::types::payer_proof::PayerProof
//! [`invoice_node_id`]: crate::types::payer_proof::PayerProof::invoice_node_id
//! [`issuer_signing_pubkey`]: crate::types::offer::Offer::issuer_signing_pubkey

use std::{fmt, str::FromStr};

use anyhow::Context;
use bitcoin::secp256k1::{self, schnorr};
use lexe_common::{ln::amount::Amount, time::TimestampMs};
use lightning::{
    offers::{
        parse::Bolt12ParseError, payer_proof::PayerProof as LdkPayerProof,
    },
    util::ser::{BigSize, Readable},
};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::types::payments::{PaymentHash, PaymentPreimage};

/// The BOLT12 TLV types a [`PayerProof`] carries.
pub mod tlv_type {
    // --- Offer --- //
    pub const OFFER_CHAINS: u64 = 2;
    pub const OFFER_METADATA: u64 = 4;
    pub const OFFER_CURRENCY: u64 = 6;
    pub const OFFER_AMOUNT: u64 = 8;
    pub const OFFER_DESCRIPTION: u64 = 10;
    pub const OFFER_FEATURES: u64 = 12;
    pub const OFFER_ABSOLUTE_EXPIRY: u64 = 14;
    pub const OFFER_PATHS: u64 = 16;
    pub const OFFER_ISSUER: u64 = 18;
    pub const OFFER_QUANTITY_MAX: u64 = 20;
    pub const OFFER_ISSUER_ID: u64 = 22;

    // --- Invoice request --- //
    pub const INVREQ_CHAIN: u64 = 80;
    pub const INVREQ_AMOUNT: u64 = 82;
    pub const INVREQ_FEATURES: u64 = 84;
    pub const INVREQ_QUANTITY: u64 = 86;
    pub const INVREQ_PAYER_ID: u64 = 88;
    pub const INVREQ_PAYER_NOTE: u64 = 89;
    pub const INVREQ_PATHS: u64 = 90;
    pub const INVREQ_BIP_353_NAME: u64 = 91;

    // --- Invoice --- //
    pub const INVOICE_PATHS: u64 = 160;
    pub const INVOICE_BLINDEDPAY: u64 = 162;
    pub const INVOICE_CREATED_AT: u64 = 164;
    pub const INVOICE_RELATIVE_EXPIRY: u64 = 166;
    pub const INVOICE_PAYMENT_HASH: u64 = 168;
    pub const INVOICE_AMOUNT: u64 = 170;
    pub const INVOICE_FALLBACKS: u64 = 172;
    pub const INVOICE_FEATURES: u64 = 174;
    pub const INVOICE_NODE_ID: u64 = 176;

    // --- Proof --- //
    pub const SIGNATURE: u64 = 240;
    pub const PROOF_SIGNATURE: u64 = 241;
    pub const PROOF_PREIMAGE: u64 = 1001;
    pub const PROOF_OMITTED_TLVS: u64 = 1002;
    pub const PROOF_MISSING_HASHES: u64 = 1003;
    pub const PROOF_LEAF_HASHES: u64 = 1004;
    pub const PROOF_NOTE: u64 = 1005;
}

/// A cryptographic proof that a [`Bolt12Invoice`] was paid, disclosing a
/// payer-chosen subset of the invoice's fields to a third-party verifier.
///
/// A `PayerProof` is already cryptographically verified; verification occurs
/// during parsing.
///
/// Serialized as bech32, e.g. `lnp1...`.
///
/// Check [`Self::invoice_node_id`] to confirm the *receiver* of the payment.
/// Note that `invoice_node_id` may not match a node's consistent ID if a
/// blinded path was used.
///
/// See the [module docs](self) to learn more about the fields a proof can
/// carry.
///
/// [`Bolt12Invoice`]: super::bolt12_invoice::Bolt12Invoice
//
// NOTE: This is exposed in the Rust SDK.
#[derive(Debug, SerializeDisplay, DeserializeFromStr)]
pub struct PayerProof(pub LdkPayerProof);

impl PayerProof {
    // --- Deterministic fields --- //

    /// The key that signed the invoice request. The [`Self::proof_signature`]
    /// is verified against it.
    ///
    /// TLV type: 88 (`invreq_payer_id`)
    pub fn invreq_payer_id(&self) -> secp256k1::PublicKey {
        self.0.payer_signing_pubkey()
    }

    /// The hash of the invoice preimage ([`Self::proof_preimage`]).
    ///
    /// TLV type: 168 (`invoice_payment_hash`)
    pub fn invoice_payment_hash(&self) -> PaymentHash {
        PaymentHash::from(self.0.payment_hash())
    }

    /// The key that signed the invoice to produce [`Self::signature`]. Despite
    /// the name, this is not necessarily a consistent node ID if the offer
    /// used a blinded path.
    ///
    /// This should match the paid offer's [`issuer_signing_pubkey`], if set.
    ///
    /// TLV type: 176 (`invoice_node_id`)
    ///
    /// [`issuer_signing_pubkey`]: super::offer::Offer::issuer_signing_pubkey
    pub fn invoice_node_id(&self) -> secp256k1::PublicKey {
        self.0.issuer_signing_pubkey()
    }

    /// The payee's signature over the invoice. See also:
    /// [`Self::invoice_node_id`].
    ///
    /// TLV type: 240 (`signature`)
    pub fn signature(&self) -> schnorr::Signature {
        self.0.invoice_signature()
    }

    /// The invoice preimage, which proves the payment actually settled.
    ///
    /// TLV type: 1001 (`proof_preimage`)
    pub fn proof_preimage(&self) -> PaymentPreimage {
        PaymentPreimage::from(self.0.payment_preimage())
    }

    /// The payer's signature over the proof. See also:
    /// [`Self::invreq_payer_id`].
    ///
    /// TLV type: 241 (`proof_signature`)
    pub fn proof_signature(&self) -> schnorr::Signature {
        self.0.proof_signature()
    }

    // TODO(nicole): expose `invoice_features`

    // --- Configurable proof fields --- //

    /// A note bound by signature to this proof, chosen when the proof was
    /// built.
    ///
    /// TLV type: 1005 (`proof_note`)
    pub fn proof_note(&self) -> Option<&str> {
        self.0.proof_note().map(|s| s.0)
    }

    // --- Disclosable invoice fields --- //
    // Ordering: offer->invreq->invoice, A->Z.

    /// The offer's advertised description.
    ///
    /// TLV type: 10 (`offer_description`)
    pub fn offer_description(&self) -> Option<&str> {
        self.0.offer_description().map(|s| s.0)
    }

    /// The payee's self-reported human-readable name.
    ///
    /// TLV type: 18 (`offer_issuer`)
    pub fn offer_issuer(&self) -> Option<&str> {
        self.0.offer_issuer().map(|s| s.0)
    }

    /// The message the payer sent to the payee when paying the offer.
    ///
    /// TLV type: 89 (`invreq_payer_note`)
    pub fn invreq_payer_note(&self) -> anyhow::Result<Option<&str>> {
        // Types are unique, so the first match is the only match.
        self.tlv_records()
            .find(|(record_type, _)| {
                *record_type == tlv_type::INVREQ_PAYER_NOTE
            })
            .map(|(_, value)| std::str::from_utf8(value))
            .transpose()
            .context("Payer note was not valid UTF-8")
    }

    /// The amount the payee's invoice asked for.
    ///
    /// TLV type: 170 (`invoice_amount`)
    pub fn invoice_amount(&self) -> Option<Amount> {
        self.0.invoice_amount_msats().map(Amount::from_msat)
    }

    /// The timestamp when the payee created the invoice. This is not the same
    /// as a Lexe payment's `created_at`.
    ///
    /// TLV type: 164 (`invoice_created_at`)
    pub fn invoice_created_at(&self) -> anyhow::Result<Option<TimestampMs>> {
        self.0
            .invoice_created_at()
            .map(TimestampMs::try_from)
            .transpose()
            .context("Invoice creation time out of range")
    }

    // --- Escape hatch --- //

    /// The proof's TLV records as `(type, value)`, type-ascending.
    //
    // Needed b/c LDK doesn't yet provide a generic TLV accessor for proofs.
    //
    // - if decoded `type`s are not strictly-increasing (including situations
    //   when two or more occurrences of the same `type` are met):
    //   - MUST fail to parse the `tlv_stream`.
    pub fn tlv_records(&self) -> impl Iterator<Item = (u64, &[u8])> {
        const MALFORMED: &str = "PayerProof bytes were already checked by LDK";

        let mut buf = self.0.bytes();
        std::iter::from_fn(move || {
            // A `tlv_record` represents a single field, encoded in the form:
            // * [`bigsize`: `type`]
            // * [`bigsize`: `length`]
            // * [`length`: `value`]
            if buf.is_empty() {
                return None;
            }
            let tlv_type = BigSize::read(&mut buf).expect(MALFORMED).0;
            let value_len = BigSize::read(&mut buf).expect(MALFORMED).0;
            let value_len = usize::try_from(value_len).expect(MALFORMED);
            let (value, rest) =
                buf.split_at_checked(value_len).expect(MALFORMED);
            buf = rest;
            Some((tlv_type, value))
        })
    }
}

impl From<LdkPayerProof> for PayerProof {
    #[inline]
    fn from(value: LdkPayerProof) -> Self {
        Self(value)
    }
}

// Not derived by LDK. A proof is fully determined by its TLV bytes.
impl PartialEq for PayerProof {
    fn eq(&self, other: &Self) -> bool {
        self.0.bytes() == other.0.bytes()
    }
}
impl Eq for PayerProof {}

impl fmt::Display for PayerProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for PayerProof {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        LdkPayerProof::from_str(s).map(Self).map_err(ParseError)
    }
}

/// Exists because [`Bolt12ParseError`] doesn't impl [`fmt::Display`].
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError(pub Bolt12ParseError);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let err = &self.0;
        write!(f, "Failed to parse payer proof: {err:?}")
    }
}

impl std::error::Error for ParseError {}

#[cfg(any(test, feature = "test-utils"))]
mod arb {
    use lexe_common::{secp256k1_ctx::SECP256K1, test_utils::arbitrary};
    use lexe_crypto::rng::{FastRng, RngExt};
    use lightning::{
        ln::{channelmanager::PaymentId, inbound_payment::ExpandedKey},
        offers::payer_proof::PaidBolt12Invoice,
        types::payment::PaymentPreimage,
    };
    use proptest::{
        arbitrary::{Arbitrary, any, any_with},
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;
    use crate::{
        models::command::PayerProofDisclosures,
        types::bolt12_invoice::{Bolt12Invoice, arb::Bolt12InvoiceParams},
    };

    impl Arbitrary for PayerProof {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<FastRng>()
                .prop_flat_map(|mut rng| {
                    // Generate the invoice with our preimage and keys.
                    let preimage = PaymentPreimage(rng.gen_bytes());
                    let expanded_key = ExpandedKey::new(rng.gen_bytes());
                    let payment_id = PaymentId(rng.gen_bytes());
                    let params = Bolt12InvoiceParams {
                        payment_preimage: Some(preimage),
                        payer_keys: Some((expanded_key, payment_id)),
                    };
                    (
                        Just(preimage),
                        Just(expanded_key),
                        Just(payment_id),
                        any_with::<Bolt12Invoice>(params),
                        any::<PayerProofDisclosures>(),
                        arbitrary::any_option_string(),
                    )
                })
                .prop_map(
                    |(
                        preimage,
                        expanded_key,
                        payment_id,
                        invoice,
                        disclosures,
                        proof_note,
                    )| {
                        gen_payer_proof(
                            preimage,
                            expanded_key,
                            payment_id,
                            invoice,
                            disclosures,
                            proof_note,
                        )
                    },
                )
                .boxed()
        }
    }

    fn gen_payer_proof(
        preimage: PaymentPreimage,
        expanded_key: ExpandedKey,
        payment_id: PaymentId,
        invoice: Bolt12Invoice,
        disclosures: PayerProofDisclosures,
        proof_note: Option<String>,
    ) -> PayerProof {
        let paid_invoice = PaidBolt12Invoice::Bolt12Invoice(invoice.0);
        let mut builder = paid_invoice
            .prove_payer_derived(
                preimage,
                &expanded_key,
                payment_id,
                &SECP256K1,
            )
            .expect("Failed to build payer proof");

        let PayerProofDisclosures {
            offer_description,
            offer_issuer,
            invreq_payer_note,
            invoice_amount,
            invoice_created_at,
            additional_disclosures,
        } = disclosures;
        if offer_description {
            builder = builder.include_offer_description();
        }
        if offer_issuer {
            builder = builder.include_offer_issuer();
        }
        if invreq_payer_note {
            builder = builder
                .include_type(tlv_type::INVREQ_PAYER_NOTE)
                .expect("Payer note is disclosable");
        }
        if invoice_amount {
            builder = builder.include_invoice_amount();
        }
        if invoice_created_at {
            builder = builder.include_invoice_created_at();
        }
        for tlv_type in additional_disclosures {
            builder = builder
                .include_type(tlv_type)
                .expect("Strategy only yields disclosable TLV types");
        }
        if let Some(proof_note) = proof_note {
            builder = builder.with_proof_note(proof_note);
        }

        let proof = builder.build_and_sign().expect("Failed to sign proof");
        PayerProof::from(proof)
    }
}

#[cfg(test)]
mod test {
    use lightning::util::ser::Writeable;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    /// [`PayerProof::tlv_records`] doesn't panic and consumes the stream
    /// exactly.
    #[test]
    fn tlv_records_walks_proofs() {
        proptest!(|(proof: PayerProof)| {
            let walked = proof
                .tlv_records()
                .map(|(tlv_type, value)| {
                    BigSize(tlv_type).serialized_length()
                        + BigSize(value.len() as u64).serialized_length()
                        + value.len()
                })
                .sum::<usize>();
            prop_assert_eq!(walked, proof.0.bytes().len());
        });
    }

    /// A proof roundtrips through its bech32 form.
    #[test]
    fn payer_proof_roundtrip() {
        proptest!(|(proof: PayerProof)| {
            let reparsed = PayerProof::from_str(&proof.to_string())
                .expect("Payer proof failed to verify");
            prop_assert_eq!(reparsed, proof);
        });
    }
}
