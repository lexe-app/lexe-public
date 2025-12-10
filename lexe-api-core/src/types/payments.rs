use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::HashSet,
    convert::Infallible,
    fmt::{self, Display},
    num::NonZeroU64,
    str::FromStr,
    sync::Arc,
};

use anyhow::{Context, anyhow, bail, ensure};
use bitcoin::{
    address::NetworkUnchecked,
    hashes::{Hash, sha256},
};
use byte_array::ByteArray;
#[cfg(any(test, feature = "test-utils"))]
use common::test_utils::arbitrary;
use common::{
    debug_panic_release_log,
    ln::{amount::Amount, hashes::LxTxid, priority::ConfirmationPriority},
    rng::{RngCore, RngExt},
    serde_helpers::{base64_or_bytes, hexstr_or_bytes},
    time::TimestampMs,
};
use lexe_std::const_assert_mem_size;
use lightning::{
    offers::offer::OfferId,
    types::payment::{PaymentHash, PaymentPreimage, PaymentSecret},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{prelude::Just, strategy::Strategy, strategy::Union};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
#[cfg(test)]
use strum::VariantArray;

use crate::types::{invoice::LxInvoice, offer::LxOffer};

// --- Top-level payment types --- //

/// A basic payment type which contains all of the user-facing payment details
/// for any kind of payment. These details are exposed in the Lexe app.
///
/// It is essentially the `Payment` type flattened out such that each field is
/// the result of the corresponding `Payment` getter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BasicPaymentV2 {
    // --- Identifier and basic info fields --- //
    ///
    /// Payment identifier; globally unique from the user's perspective.
    pub id: LxPaymentId,

    /// The ids of payments related to this payment.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_hashset::<LxPaymentId>()")
    )]
    pub related_ids: HashSet<LxPaymentId>,

    /// Payment kind: Application-level purpose of this payment.
    pub kind: PaymentKind,

    /// The payment direction: `Inbound`, `Outbound`, or `Info`.
    pub direction: PaymentDirection,

    /// (Offer payments only) The id of the BOLT12 offer used in this payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_id: Option<LxOfferId>,

    /// (Onchain payments only) The original txid.
    // NOTE: we're duplicating the txid here for onchain receives because its
    // less error prone to use, esp. for external API consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<LxTxid>,

    // --- Amounts --- //
    ///
    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, this is the amount we
    ///   received.
    /// - If this is a pending or failed inbound inbound invoice payment, this
    ///   is the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Amount>,

    // --- Fees --- //
    ///
    /// The fees for this payment.
    ///
    /// Use this whenever you need a singular value to display.
    ///
    /// - For outbound Lightning payments, these are the routing fees. If the
    ///   payment is not completed, this value is an estimation only. Iff the
    ///   payment completes, this value reflects actual fees paid.
    /// - For inbound Lightning payments, this is the skimmed fee, which may
    ///   also cover the on-chain fees incurred by a JIT channel open.
    /// - For on-chain sends, this is the on-chain fee paid in the spending tx.
    // Renamed in node-v0.8.10.
    // Can be removed only after *all* payments have migrated to payments v2.
    #[serde(rename = "fees", alias = "fee")]
    pub fee: Amount,

    /* TODO(max): Implement JIT channel fees
    /// (Inbound payments only) The portion of the skimmed amount that was used
    /// to cover the on-chain fees incurred by a JIT channel opened to receive
    /// this payment. Zero if no channel fees were incurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_fee: Option<Amount>,
    */
    // --- Status --- //
    ///
    /// General payment status: pending, completed, or failed.
    pub status: PaymentStatus,

    /// The payment status as a human-readable string. These strings are
    /// customized per payment type, e.g. "invoice generated", "timed out"
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub status_str: String,

    // --- Payment methods --- //
    ///
    /// (Onchain send only) The address that we're sending to.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(
            strategy = "arbitrary::any_option_arc_mainnet_addr_unchecked()"
        )
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<Arc<bitcoin::Address<NetworkUnchecked>>>,

    /// (Invoice payments only) The BOLT11 invoice used in this payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<Arc<LxInvoice>>,

    /// (Outbound offer payments only) The BOLT12 offer used in this payment.
    /// Until we store offers out-of-line, this is not yet available for
    /// inbound offer payments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<Arc<LxOffer>>,

    /// The on-chain transaction, if there is one.
    /// Always [`Some`] for on-chain sends and receives.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_arc_raw_tx()")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx: Option<Arc<bitcoin::Transaction>>,

    // --- Notes and sender/receiver identifiers --- //
    ///
    /// An optional personal note which a user can attach to any payment. A
    /// note can always be added or modified when a payment already exists,
    /// but this may not always be possible at creation time. These
    /// differences are documented below:
    ///
    /// - Onchain send: The user has the option to set this at creation time.
    /// - Onchain receive: The user can only add a note after the onchain
    ///   receive has been detected.
    ///
    /// - Inbound invoice payments: Since a user-provided description is
    ///   already required when creating an invoice, at invoice creation time
    ///   this field is not exposed to the user and is simply initialized to
    ///   [`None`]. Useful primarily if a user wants to update their note
    ///   later.
    /// - Inbound offer reusable payments and Inbound spontaneous payment:
    ///   There is no way for users to add the note at receive time, so this
    ///   field can only be added or updated later.
    ///
    /// - Outbound invoice payments: Since the receiver sets the invoice
    ///   description, which might just be a useless 🍆 emoji, the user has the
    ///   option to add this note at the time of invoice payment.
    /// - Outbound spontaneous payment: Since there is no invoice description
    ///   field, the user has the option to set this at payment creation time.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,

    /// (Offer payments only) The payer's self-reported human-readable name.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_name: Option<String>,

    /// (Offer payments only) A payer-provided note for this payment.
    /// LDK truncates this to 512 bytes.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer_note: Option<String>,

    // --- Other --- //
    ///
    /// (Onchain send only) The confirmation priority used for this payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<ConfirmationPriority>,

    /// (Inbound offer reusable only) The number of items the payer bought.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<NonZeroU64>,

    /// (Onchain payments only) The txid of the replacement tx, if one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    // Renamed in node-v0.8.10.
    // Can be removed only after *all* payments have migrated to payments v2.
    #[serde(rename = "replacement", alias = "replacement_txid")]
    pub replacement_txid: Option<LxTxid>,

    // --- Timestamps --- //
    ///
    /// The invoice or offer expiry time.
    /// `None` otherwise, or if the timestamp overflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<TimestampMs>,

    /// When this payment was finalized (completed or failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,

    /// When this payment was created.
    pub created_at: TimestampMs,

    /// When this payment was last updated.
    pub updated_at: TimestampMs,
}

// Debug the size_of `BasicPaymentV2`
const_assert_mem_size!(BasicPaymentV2, 432);

/// An upgradeable version of [`Option<BasicPaymentV2>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeBasicPaymentV2 {
    pub maybe_payment: Option<BasicPaymentV2>,
}

/// An upgradeable version of [`Vec<BasicPaymentV2>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecBasicPaymentV2 {
    pub payments: Vec<BasicPaymentV2>,
}

/// The old version of [`BasicPaymentV2`]; see [`BasicPaymentV2`] for docs.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BasicPaymentV1 {
    pub index: PaymentCreatedIndex,
    // What is now "payment rail" we used to refer to as "payment kind"
    #[serde(rename = "kind")]
    pub rail: PaymentRail,
    pub direction: PaymentDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<Arc<LxInvoice>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer_id: Option<LxOfferId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<Arc<LxOffer>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<LxTxid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<LxTxid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Amount>,
    pub fees: Amount,
    pub status: PaymentStatus,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub status_str: String,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,
}

// Debug the size_of `BasicPaymentV1`
const_assert_mem_size!(BasicPaymentV1, 296);

/// An upgradeable version of [`Vec<BasicPaymentV1>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecBasicPaymentV1 {
    pub payments: Vec<BasicPaymentV1>,
}

/// An encrypted payment, as represented in the DB.
/// V1 has an extremely inefficient JSON encoding, so we're migrating from it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbPaymentV1 {
    pub id: String,
    pub status: String,
    pub data: Vec<u8>,
    pub created_at: i64,
}

/// An upgradeable version of [`Option<DbPaymentV1>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeDbPaymentV1 {
    pub maybe_payment: Option<DbPaymentV1>,
}

/// An upgradeable version of [`Vec<DbPaymentV1>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecDbPaymentV1 {
    pub payments: Vec<DbPaymentV1>,
}

/// An encrypted payment, as represented in the DB.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbPaymentV2 {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Cow<'static, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Cow<'static, str>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<Amount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee: Option<Amount>,
    pub status: Cow<'static, str>,
    #[serde(with = "base64_or_bytes")]
    pub data: Vec<u8>,
    #[serde(default = "default_version")]
    pub version: i16,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_version() -> i16 {
    1
}

impl DbPaymentV2 {
    pub fn from_v1(v1: DbPaymentV1, updated_at: i64) -> Self {
        Self {
            id: v1.id,
            kind: None,
            direction: None,
            amount: None,
            fee: None,
            status: Cow::Owned(v1.status),
            data: v1.data,
            version: 2,
            created_at: v1.created_at,
            updated_at,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl PartialEq<DbPaymentV2> for DbPaymentV1 {
    fn eq(&self, other: &DbPaymentV2) -> bool {
        self.id == other.id
            && self.status == other.status
            && self.data == other.data
            && self.created_at == other.created_at
            && other.kind.is_none()
            && other.direction.is_none()
            && other.amount.is_none()
            && other.fee.is_none()
    }
}
#[cfg(any(test, feature = "test-utils"))]
impl PartialEq<DbPaymentV1> for DbPaymentV2 {
    fn eq(&self, other: &DbPaymentV1) -> bool {
        self.id == other.id
            && self.status == other.status
            && self.data == other.data
            && self.created_at == other.created_at
            && self.kind.is_none()
            && self.direction.is_none()
            && self.amount.is_none()
            && self.fee.is_none()
    }
}

impl From<DbPaymentV2> for DbPaymentV1 {
    fn from(v2: DbPaymentV2) -> Self {
        Self {
            id: v2.id,
            status: v2.status.into_owned(),
            data: v2.data,
            created_at: v2.created_at,
        }
    }
}

/// An upgradeable version of [`Option<DbPaymentV2>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeDbPaymentV2 {
    pub maybe_payment: Option<DbPaymentV2>,
}

/// An upgradeable version of [`Vec<DbPaymentV2>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecDbPaymentV2 {
    pub payments: Vec<DbPaymentV2>,
}

/// An encrypted payment metadata, as represented in the DB.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbPaymentMetadata {
    pub id: String,
    #[serde(with = "base64_or_bytes")]
    pub data: Vec<u8>,
    pub updated_at: i64,
}

/// An upgradeable version of [`Option<DbPaymentMetadata>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeDbPaymentMetadata {
    pub maybe_metadata: Option<DbPaymentMetadata>,
}

/// An upgradeable version of [`Vec<DbPaymentMetadata>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecDbPaymentMetadata {
    pub metadatas: Vec<DbPaymentMetadata>,
}

/// The technical 'rail' used to fulfill a payment:
/// onchain, invoice, offer, spontaneous, etc.
#[derive(Clone, Debug, Eq, PartialEq, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum PaymentRail {
    Onchain,
    Invoice,
    Offer,
    Spontaneous,
    WaivedFee,
    /// Unknown rail; used for forward compatibility.
    Unknown(
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_string().prop_map(Box::from)")
        )]
        Box<str>,
    ),
}

/// A granular application-level 'type' of a payment.
///
/// In Lexe's DB, payment information is encrypted, but this type is exposed.
/// This is because without this type, the DB cannot identify which payments are
/// relevant to a application level queries like
///
/// - "Show me my last N liquidity fee payments from this index"
/// - "Show me my history of channel opens and closes"
/// - "Show me the last N times I paid a channel fee."
///
/// These payment kinds are also useful for accounting and analytics, allowing
/// users to breakdown their payments history using fine-grained categories.
///
/// When implementing new kind of payment flows, err on the side of adding a new
/// [`PaymentKind`] for it, instead of incorporating it into an existing kind.
/// We can always add another OR in a WHERE clause, but cannot easily separate
/// out data once it has already been unified.
#[rustfmt::skip]
#[derive(Clone, Debug, Eq, PartialEq, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum PaymentKind {
    // --- Generic kind or Legacy --- //

    /// A regular on-chain send or receive.
    /// All v1 on-chain payments which had no kind are given this kind.
    Onchain, // rail: Onchain

    /// A regular Lightning send or receive.
    /// All v1 invoice payments which had no kind are given this kind.
    Invoice, // rail: Invoice

    /// A regular Lightning offer payment.
    /// All v1 offer payments which had no kind are given this kind.
    Offer, // rail: Offer

    /// A regular Lightning spontaneous payment.
    /// All v1 spontaneous payments which had no kind are given this kind.
    Spontaneous, // rail: Spontaneous

    // /// A splice into or out of a channel.
    // Splice, // rail: Splice

    // TODO(max): Think about inbound vs outbound channel opens.
    // Post-splicing, it seems we should only worry about inbound channel
    // opens, since the user will never use on-chain funds to open a
    // second channel (they would only splice into an existing channel)?
    // <https://app.clickup.com/t/86a5nr7h9>
    //
    // /// A channel open.
    // ChannelOpen, // rail: Channel

    // /// A channel close.
    // ChannelClose, // rail: Channel

    // --- General --- //

    /// A channel fee that would have been paid but was waived.
    WaivedChannelFee, // rail: WaivedFee

    /// A liquidity fee that would have been paid but was waived.
    WaivedLiquidityFee, // rail: WaivedFee

    // /// A routing fee that would have been paid but was waived.
    // WaivedRoutingFee, // rail: WaivedFee

    // /// A payment to cover the on-chain costs of an inbound channel.
    // ChannelFeePayment, // rail: Spontaneous

    // /// An interest payment on our current amount of inbound liquidity.
    // LiquidityFeePayment, // rail: Spontaneous

    // /// A payment to cover the on-chain costs of changing the size of our
    // /// channel, typically to adjust our amount of inbound liquidity.
    // ///
    // /// Paid when:
    // /// - We want to increase our inbound liquidity (LSP splices in)
    // /// - We want to decrease our inbound liquidity (LSP splices out)
    // LiquidityAdjustmentPayment, // rail: Spontaneous

    // /// Revshare earnings from a partner fee we levied.
    // PartnerFeeRevenue, // rail: Spontaneous

    // /// Referral fees earned from users we referred.
    // ReferralFeeRevenue, // rail: Spontaneous

    // --- Fallback --- //

    /// Unknown kind; used for forward compatibility.
    Unknown(
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_string().prop_map(Box::from)")
        )]
        Box<str>,
    ),
}

/// Specifies whether a payment is inbound or outbound.
#[derive(Copy, Clone, Debug, Eq, PartialEq, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(VariantArray))]
pub enum PaymentDirection {
    /// Inbound payment; we received money and our balance increased.
    Inbound,
    /// Outbound payment; we spent money and our balance decreased.
    Outbound,
    /// A journal entry which didn't increase or decrease our balance.
    Info,
}

/// A general payment status that abstracts over all payment types.
///
/// - Useful for filtering all payments by status in a high-level list view.
/// - Not suitable for getting detailed information about a specific payment; in
///   this case, use the payment-specific status enum or `status_str()` instead.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(VariantArray))]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

// --- Lexe newtypes --- //

/// A payment identifier which:
///
/// 1) retains uniqueness per payment
/// 2) is ordered first by `created_at` timestamp and then by [`LxPaymentId`].
///
/// It is essentially a [`(TimestampMs, LxPaymentId)`], suitable for use as a
/// key in a `BTreeMap<PaymentCreatedIndex, BasicPaymentV1>` or similar.
///
/// It can also be degenerated (serialized) into a string and the
/// string-serialized ordering will be equivalent to the unserialized ordering.
///
/// ### Examples
///
/// ```ignore
/// 0002683862736062841-os_95cc800f4f3b5669c71c85f7096be45a172ca86aef460e0e584affff3ea80bee
/// 0009557253037960566-ln_3ddcfd0e0b1eba77292c23a7de140c1e71327ac97486cc414b6826c434c560cc
/// 4237937319278351047-or_3f6d2153bde1a0878717f46a1cbc63c48f7b4231224d78a50eb9e94b5d29f674
/// 6206503357534413026-ln_063a5be0218332a84f9a4f7f4160a7dcf8e9362b9f5043ad47360c7440037fa8
/// 6450440432938623603-or_0db1f1ebed6f99574c7a048e6bbf68c7db69c6da328f0b6d699d4dc1cd477017
/// 7774176661032219027-or_215ef16c8192c8d674b519a34b7b65454e1e18d48bf060bdc333df433ada0137
/// 8468903867373394879-ln_b8cbf827292c2b498e74763290012ed92a0f946d67e733e94a5fedf7f82710d5
/// 8776421933930532767-os_ead3c01be0315dfd4e4c405aaca0f39076cff722a0f680c89c348e3bda9575f3
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentCreatedIndex {
    pub created_at: TimestampMs,
    pub id: LxPaymentId,
}

/// A payment identifier, conceptually a [`(TimestampMs, LxPaymentId)`], which:
///
/// 1) retains uniqueness per payment
/// 2) is ordered first by `updated_at` timestamp and then by [`LxPaymentId`].
///
/// It can also be degenerated (serialized) into a string and the
/// string-serialized ordering will be equivalent to the unserialized ordering.
///
/// ### Examples
///
/// ```ignore
/// u0002683862736062841-os_95cc800f4f3b5669c71c85f7096be45a172ca86aef460e0e584affff3ea80bee
/// u0009557253037960566-ln_3ddcfd0e0b1eba77292c23a7de140c1e71327ac97486cc414b6826c434c560cc
/// u4237937319278351047-or_3f6d2153bde1a0878717f46a1cbc63c48f7b4231224d78a50eb9e94b5d29f674
/// u6206503357534413026-ln_063a5be0218332a84f9a4f7f4160a7dcf8e9362b9f5043ad47360c7440037fa8
/// u6450440432938623603-or_0db1f1ebed6f99574c7a048e6bbf68c7db69c6da328f0b6d699d4dc1cd477017
/// u7774176661032219027-or_215ef16c8192c8d674b519a34b7b65454e1e18d48bf060bdc333df433ada0137
/// u8468903867373394879-ln_b8cbf827292c2b498e74763290012ed92a0f946d67e733e94a5fedf7f82710d5
/// u8776421933930532767-os_ead3c01be0315dfd4e4c405aaca0f39076cff722a0f680c89c348e3bda9575f3
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentUpdatedIndex {
    pub updated_at: TimestampMs,
    pub id: LxPaymentId,
}

/// A globally-unique identifier for any type of payment, including both
/// on-chain and Lightning payments.
///
/// - Lightning inbound+outbound invoice+spontaneous payments use their
///   [`LxPaymentHash`] as their id. TODO(phlip9): inbound spontaneous payments
///   should use `LnClaimId` as their id.
/// - Lightning _reusable_ inbound offer payments use the [`LnClaimId`] as their
///   id.
/// - Lightning _single-use_ inbound offer payments use the [`OfferId`] as their
///   id. TODO(phlip9): impl
/// - Lightning outbound offer payments use a [`ClientPaymentId`] as their id.
/// - On-chain sends use a [`ClientPaymentId`] as their id.
/// - On-chain receives use their [`LxTxid`] as their id.
///
/// NOTE that this is NOT a drop-in replacement for LDK's [`PaymentId`], since
/// [`PaymentId`] is Lightning-specific, whereas [`LxPaymentId`] is not.
///
/// [`PaymentId`]: lightning::ln::channelmanager::PaymentId
// TODO(phlip9): bolt12 refunds
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxPaymentId {
    // NOTE: the enum order is important. `LxPaymentId::prefix()` determines
    // the order ("fi" < .. < "os").
    // Added `Offer*` variants in `node-v0.7.8`
    // TODO(phlip9): single-use offer payments would require a different id
    // OfferRecvInvoice(LxOfferId),  // "fi"
    OfferRecvReusable(LnClaimId), // "fr"
    OfferSend(ClientPaymentId),   // "fs"
    Lightning(LxPaymentHash),     // "ln"
    OnchainRecv(LxTxid),          // "or"
    OnchainSend(ClientPaymentId), // "os"
}

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
///
/// Its primary purpose is to prevent accidental double payments. Internal
/// structure (if any) is opaque to the node.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ClientPaymentId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

/// Newtype for [`PaymentHash`] which impls [`Serialize`] / [`Deserialize`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxPaymentHash(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentPreimage`] which impls [`Serialize`] / [`Deserialize`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxPaymentPreimage(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentSecret`] which impls [`Serialize`] / [`Deserialize`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxPaymentSecret(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`OfferId`] which impls [`Serialize`] / [`Deserialize`].
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxOfferId(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for LDK's [`PaymentId`] but used specifically for inbound lightning
/// payment idempotency.
///
/// It is the hash of the HTLC(s) paying for a specific payment hash. There can
/// be multiple `LnClaimId`s for a single payment hash if e.g. a payer
/// mistakenly pays the same invoice twice.
///
/// We get this value from LDK's [`PaymentClaimable`] and [`PaymentClaimed`]
/// events.
///
/// [`PaymentId`]: lightning::ln::channelmanager::PaymentId
/// [`PaymentClaimable`]: lightning::events::Event::PaymentClaimable
/// [`PaymentClaimed`]: lightning::events::Event::PaymentClaimed
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LnClaimId(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

// --- impl BasicPaymentV2 --- //

impl BasicPaymentV2 {
    pub fn from_v1(v1: BasicPaymentV1, updated_at: TimestampMs) -> Self {
        let kind = match v1.rail {
            PaymentRail::Onchain => PaymentKind::Onchain,
            PaymentRail::Invoice => PaymentKind::Invoice,
            PaymentRail::Offer => PaymentKind::Offer,
            PaymentRail::Spontaneous => PaymentKind::Spontaneous,
            // V1 rails are supported exhaustively
            PaymentRail::Unknown(_) => unreachable!(),
            // V1 payments don't have these kinds
            PaymentRail::WaivedFee => unreachable!(),
        };

        Self {
            id: v1.index.id,
            related_ids: HashSet::new(),
            kind,
            direction: v1.direction,
            offer_id: v1.offer_id,
            txid: v1.txid,
            amount: v1.amount,
            fee: v1.fees,
            // channel_fee: None,
            status: v1.status,
            status_str: v1.status_str,
            address: None,
            invoice: v1.invoice,
            offer: v1.offer,
            tx: None,
            note: v1.note,
            payer_name: None,
            payer_note: None,
            priority: None,
            quantity: None,
            replacement_txid: v1.replacement,
            expires_at: None,
            finalized_at: v1.finalized_at,
            created_at: v1.index.created_at,
            updated_at,
        }
    }

    #[inline]
    pub fn payment_id(&self) -> LxPaymentId {
        self.id
    }

    #[inline]
    pub fn is_pending(&self) -> bool {
        use PaymentStatus::*;
        match self.status {
            Pending => true,
            Completed | Failed => false,
        }
    }

    #[inline]
    pub fn is_finalized(&self) -> bool {
        !self.is_pending()
    }

    pub fn is_pending_not_junk(&self) -> bool {
        self.is_pending() && !self.is_junk()
    }

    pub fn is_finalized_not_junk(&self) -> bool {
        self.is_finalized() && !self.is_junk()
    }

    /// "Junk" payments are unimportant, usually not-user-initiated payments
    /// that we don't display by default, unless a user explicitly opts-in to a
    /// a "show everything" filter for debugging.
    ///
    /// For example, the current receive UI generates a "junk" BOLT11 invoice on
    /// every page open, but we don't want this invoice to show up in the
    /// payments list unless it actually gets paid.
    pub fn is_junk(&self) -> bool {
        // amount-less, description-less inbound BOLT11 invoices are junk
        // payments unless paid.
        // TODO(phlip9): also don't show pending/failed "superseded" invoices,
        // where the user edited the amount/description.
        self.status != PaymentStatus::Completed
            && self.kind.rail() == PaymentRail::Invoice
            && self.direction == PaymentDirection::Inbound
            && (self.amount.is_none() || self.note_or_description().is_none())
    }

    /// Returns the user's note or invoice description, prefering note over
    /// description.
    pub fn note_or_description(&self) -> Option<&str> {
        let maybe_note = self.note.as_deref().filter(|s| !s.is_empty());

        maybe_note
            .or_else(|| {
                self.invoice.as_deref().and_then(LxInvoice::description_str)
            })
            .or_else(|| self.offer.as_deref().and_then(LxOffer::description))
    }

    #[inline]
    pub fn created_at(&self) -> TimestampMs {
        self.created_at
    }

    #[inline]
    pub fn updated_at(&self) -> TimestampMs {
        self.updated_at
    }

    #[inline]
    pub fn created_index(&self) -> PaymentCreatedIndex {
        PaymentCreatedIndex {
            created_at: self.created_at,
            id: self.id,
        }
    }

    #[inline]
    pub fn updated_index(&self) -> PaymentUpdatedIndex {
        PaymentUpdatedIndex {
            updated_at: self.updated_at,
            id: self.id,
        }
    }
}

// --- impl BasicPaymentV1 --- //

impl BasicPaymentV1 {
    pub fn index(&self) -> &PaymentCreatedIndex {
        &self.index
    }
    pub fn created_at(&self) -> TimestampMs {
        self.index.created_at
    }
    pub fn payment_id(&self) -> LxPaymentId {
        self.index.id
    }
    pub fn is_pending(&self) -> bool {
        use PaymentStatus::*;
        match self.status {
            Pending => true,
            Completed | Failed => false,
        }
    }
    pub fn is_finalized(&self) -> bool {
        !self.is_pending()
    }
    pub fn is_pending_not_junk(&self) -> bool {
        self.is_pending() && !self.is_junk()
    }
    pub fn is_finalized_not_junk(&self) -> bool {
        self.is_finalized() && !self.is_junk()
    }
    pub fn is_junk(&self) -> bool {
        self.status != PaymentStatus::Completed
            && self.rail == PaymentRail::Invoice
            && self.direction == PaymentDirection::Inbound
            && (self.amount.is_none() || self.note_or_description().is_none())
    }
    pub fn note_or_description(&self) -> Option<&str> {
        let maybe_note = self.note.as_deref().filter(|s| !s.is_empty());
        maybe_note
            .or_else(|| {
                self.invoice.as_deref().and_then(LxInvoice::description_str)
            })
            .or_else(|| self.offer.as_deref().and_then(LxOffer::description))
    }
}

impl From<BasicPaymentV2> for BasicPaymentV1 {
    fn from(v2: BasicPaymentV2) -> Self {
        Self {
            index: PaymentCreatedIndex {
                created_at: v2.created_at,
                id: v2.id,
            },
            rail: v2.kind.rail(),
            direction: v2.direction,
            invoice: v2.invoice,
            offer_id: v2.offer_id,
            offer: v2.offer,
            txid: v2.txid,
            replacement: v2.replacement_txid,
            amount: v2.amount,
            fees: v2.fee,
            status: v2.status,
            status_str: v2.status_str,
            note: v2.note,
            finalized_at: v2.finalized_at,
        }
    }
}

impl PartialOrd for BasicPaymentV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.index.partial_cmp(&other.index)
    }
}

// --- impl PaymentCreatedIndex --- //

impl PaymentCreatedIndex {
    /// The index that is lexicographically <= all other indexes.
    pub const MIN: Self = Self {
        created_at: TimestampMs::MIN,
        id: LxPaymentId::MIN,
    };

    /// The index that is lexicographically >= all other indexes.
    pub const MAX: Self = Self {
        created_at: TimestampMs::MAX,
        id: LxPaymentId::MAX,
    };

    /// Quickly create a dummy index which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_u8(i: u8) -> Self {
        let created_at = TimestampMs::from_u8(i);
        let id = LxPaymentId::Lightning(LxPaymentHash([i; 32]));
        Self { created_at, id }
    }
}

impl PaymentUpdatedIndex {
    /// The index that is lexicographically <= all other indexes.
    pub const MIN: Self = Self {
        updated_at: TimestampMs::MIN,
        id: LxPaymentId::MIN,
    };

    /// The index that is lexicographically >= all other indexes.
    pub const MAX: Self = Self {
        updated_at: TimestampMs::MAX,
        id: LxPaymentId::MAX,
    };

    /// Quickly create a dummy index which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_u8(i: u8) -> Self {
        let updated_at = TimestampMs::from_u8(i);
        let id = LxPaymentId::from_u8(i);
        Self { updated_at, id }
    }
}

// --- impl LxPaymentId --- //

impl LxPaymentId {
    /// The `LxPaymentId` that is lexicographically <= all other ids.
    pub const MIN: Self = Self::OfferRecvReusable(LnClaimId([0; 32]));

    /// The `LxPaymentId` that is lexicographically >= all other ids.
    pub const MAX: Self = Self::OnchainSend(ClientPaymentId([255; 32]));

    /// Returns the prefix to use when serializing this payment id to a string.
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::OfferRecvReusable(_) => "fr",
            Self::OfferSend(_) => "fs",
            Self::Lightning(_) => "ln",
            Self::OnchainRecv(_) => "or",
            Self::OnchainSend(_) => "os",
        }
    }

    /// From the data we get in a `PaymentSent` event, determine the payment id
    /// for this outbound lightning payment.
    pub fn from_payment_sent(
        ldk_payment_id: Option<lightning::ln::channelmanager::PaymentId>,
        payment_hash: LxPaymentHash,
    ) -> Self {
        match ldk_payment_id {
            Some(ldk_payment_id) => {
                // BOLT11 invoice and spontaneous should always use the payment
                // hash as the payment id
                if &ldk_payment_id.0 == payment_hash.as_array() {
                    LxPaymentId::Lightning(payment_hash)
                } else {
                    LxPaymentId::OfferSend(ClientPaymentId(ldk_payment_id.0))
                }
            }
            None => {
                // We should always be setting a payment id, but maybe this is
                // just an ancient event?
                debug_panic_release_log!("event did not include a PaymentId");
                LxPaymentId::Lightning(payment_hash)
            }
        }
    }

    /// From the data we get in a `PaymentFailed` event, determine the payment
    /// id for this outbound lightning payment.
    pub fn from_payment_failed(
        ldk_payment_id: lightning::ln::channelmanager::PaymentId,
        payment_hash: Option<LxPaymentHash>,
    ) -> Self {
        match payment_hash {
            Some(payment_hash) => {
                // BOLT11 invoice and spontaneous should always use the payment
                // hash as the payment id
                if &ldk_payment_id.0 == payment_hash.as_array() {
                    LxPaymentId::Lightning(payment_hash)
                } else {
                    LxPaymentId::OfferSend(ClientPaymentId(ldk_payment_id.0))
                }
            }
            None => {
                // Payment hash is `None` if this was an offer payment and it
                // failed before we managed to fetch the BOLT12 invoice.
                LxPaymentId::OfferSend(ClientPaymentId(ldk_payment_id.0))
            }
        }
    }

    /// Quickly create a dummy id which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_u8(i: u8) -> Self {
        Self::Lightning(LxPaymentHash([i; 32]))
    }
}

// --- impl ClientPaymentId --- //

impl ClientPaymentId {
    /// Sample a random [`ClientPaymentId`].
    /// The rng is not required to be cryptographically secure.
    pub fn from_rng(rng: &mut impl RngCore) -> Self {
        Self(rng.gen_bytes())
    }
}

// --- impl LxPaymentPreimage --- //

impl LxPaymentPreimage {
    /// Computes the [`LxPaymentHash`] corresponding to this preimage.
    pub fn compute_hash(&self) -> LxPaymentHash {
        let sha256_hash = sha256::Hash::hash(&self.0);
        LxPaymentHash::from(sha256_hash)
    }
}

// --- Boilerplate: ByteArray / FromStr / Display / Debug --- //

byte_array::impl_byte_array!(ClientPaymentId, 32);
byte_array::impl_byte_array!(LxPaymentHash, 32);
byte_array::impl_byte_array!(LxPaymentPreimage, 32);
byte_array::impl_byte_array!(LxPaymentSecret, 32);
byte_array::impl_byte_array!(LxOfferId, 32);
byte_array::impl_byte_array!(LnClaimId, 32);

byte_array::impl_fromstr_fromhex!(ClientPaymentId, 32);
byte_array::impl_fromstr_fromhex!(LxPaymentHash, 32);
byte_array::impl_fromstr_fromhex!(LxPaymentPreimage, 32);
byte_array::impl_fromstr_fromhex!(LxPaymentSecret, 32);
byte_array::impl_fromstr_fromhex!(LxOfferId, 32);
byte_array::impl_fromstr_fromhex!(LnClaimId, 32);

byte_array::impl_debug_display_as_hex!(ClientPaymentId);
byte_array::impl_debug_display_as_hex!(LxPaymentHash);
byte_array::impl_debug_display_as_hex!(LxOfferId);
byte_array::impl_debug_display_as_hex!(LnClaimId);
// Redacted to prevent accidentally leaking secrets in logs
byte_array::impl_debug_display_redacted!(LxPaymentPreimage);
byte_array::impl_debug_display_redacted!(LxPaymentSecret);

// --- Newtype From impls --- //

// LxPaymentId -> ClientPaymentId / Txid / LxPaymentHash
impl TryFrom<LxPaymentId> for ClientPaymentId {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        use LxPaymentId::*;
        match id {
            OnchainSend(cid) | OfferSend(cid) => Ok(cid),
            OfferRecvReusable(_) | OnchainRecv(_) | Lightning(_) =>
                bail!("Not an onchain send"),
        }
    }
}
impl TryFrom<LxPaymentId> for LxPaymentHash {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        use LxPaymentId::*;
        match id {
            Lightning(hash) => Ok(hash),
            OnchainSend(_) | OfferSend(_) | OfferRecvReusable(_)
            | OnchainRecv(_) => bail!("Not a lightning payment"),
        }
    }
}

// Bitcoin -> Lexe
impl From<sha256::Hash> for LxPaymentHash {
    fn from(hash: sha256::Hash) -> Self {
        Self(hash.to_byte_array())
    }
}

// LDK -> Lexe
impl From<PaymentHash> for LxPaymentHash {
    fn from(hash: PaymentHash) -> Self {
        Self(hash.0)
    }
}
impl From<PaymentPreimage> for LxPaymentPreimage {
    fn from(preimage: PaymentPreimage) -> Self {
        Self(preimage.0)
    }
}
impl From<PaymentSecret> for LxPaymentSecret {
    fn from(secret: PaymentSecret) -> Self {
        Self(secret.0)
    }
}
impl From<OfferId> for LxOfferId {
    fn from(id: OfferId) -> Self {
        Self(id.0)
    }
}
impl From<lightning::ln::channelmanager::PaymentId> for LnClaimId {
    fn from(id: lightning::ln::channelmanager::PaymentId) -> Self {
        Self(id.0)
    }
}

// Lexe -> LDK
impl From<LxPaymentHash> for PaymentHash {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
    }
}
impl From<LxPaymentPreimage> for PaymentPreimage {
    fn from(preimage: LxPaymentPreimage) -> Self {
        Self(preimage.0)
    }
}
impl From<LxPaymentSecret> for PaymentSecret {
    fn from(secret: LxPaymentSecret) -> Self {
        Self(secret.0)
    }
}
impl From<LxOfferId> for OfferId {
    fn from(id: LxOfferId) -> Self {
        Self(id.0)
    }
}
impl From<LnClaimId> for lightning::ln::channelmanager::PaymentId {
    fn from(id: LnClaimId) -> Self {
        Self(id.0)
    }
}

impl From<LxPaymentHash> for lightning::ln::channelmanager::PaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
    }
}

impl From<ClientPaymentId> for lightning::ln::channelmanager::PaymentId {
    fn from(cid: ClientPaymentId) -> Self {
        Self(cid.0)
    }
}

// --- impl PaymentKind --- //

impl PaymentRail {
    /// All non-unknown variants.
    pub const KNOWN_VARIANTS: &'static [Self] = const {
        // Trigger a compilation failure if a new variant is added.
        // If you added a new variant, add it to the array below.
        match Self::Onchain {
            Self::Onchain
            | Self::Invoice
            | Self::Offer
            | Self::Spontaneous
            | Self::WaivedFee
            | Self::Unknown(_) => (),
        }

        &[
            Self::Onchain,
            Self::Invoice,
            Self::Offer,
            Self::Spontaneous,
            Self::WaivedFee,
        ]
    };

    pub fn to_str(&self) -> Cow<'static, str> {
        match self {
            Self::Onchain => Cow::Borrowed("onchain"),
            Self::Invoice => Cow::Borrowed("invoice"),
            Self::Offer => Cow::Borrowed("offer"),
            Self::Spontaneous => Cow::Borrowed("spontaneous"),
            Self::WaivedFee => Cow::Borrowed("waived_fee"),
            Self::Unknown(s) => Cow::Owned(s.to_string()),
        }
    }

    /// Returns a strategy generating [`PaymentKind`] variants matching this
    /// payment rail, uniformly distributed (weight 1 each).
    #[cfg(any(test, feature = "test-utils"))]
    pub fn any_child_kind(self) -> Union<Just<PaymentKind>> {
        let kinds = PaymentKind::KNOWN_VARIANTS
            .iter()
            .filter(|c| c.rail() == self)
            .cloned()
            .map(Just);
        Union::new(kinds)
    }
}

impl FromStr for PaymentRail {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "invoice" => Ok(Self::Invoice),
            "offer" => Ok(Self::Offer),
            "onchain" => Ok(Self::Onchain),
            "spontaneous" => Ok(Self::Spontaneous),
            "waived_fee" => Ok(Self::WaivedFee),
            _ => Ok(Self::Unknown(Box::from(s))),
        }
    }
}
impl Display for PaymentRail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_str())
    }
}
impl Serialize for PaymentRail {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_str().serialize(serializer)
    }
}

// --- impl PaymentKind --- //

impl PaymentKind {
    /// All non-unknown variants.
    pub const KNOWN_VARIANTS: &'static [Self] = const {
        // Trigger a compilation failure if a new variant is added.
        // If you added a new variant, add it to the array below.
        match Self::Onchain {
            Self::Onchain
            | Self::Invoice
            | Self::Offer
            | Self::Spontaneous
            | Self::WaivedChannelFee
            | Self::WaivedLiquidityFee
            | Self::Unknown(_) => (),
        }

        &[
            Self::Onchain,
            Self::Invoice,
            Self::Offer,
            Self::Spontaneous,
            Self::WaivedChannelFee,
            Self::WaivedLiquidityFee,
        ]
    };

    pub fn to_str(&self) -> Cow<'static, str> {
        match self {
            Self::Onchain => Cow::Borrowed("onchain"),
            Self::Invoice => Cow::Borrowed("invoice"),
            Self::Offer => Cow::Borrowed("offer"),
            Self::Spontaneous => Cow::Borrowed("spontaneous"),
            Self::WaivedChannelFee => Cow::Borrowed("waived_channel_fee"),
            Self::WaivedLiquidityFee => Cow::Borrowed("waived_liquidity_fee"),
            Self::Unknown(s) => Cow::Owned(s.to_string()),
        }
    }

    pub fn rail(&self) -> PaymentRail {
        match self {
            Self::Onchain => PaymentRail::Onchain,
            Self::Invoice => PaymentRail::Invoice,
            Self::Offer => PaymentRail::Offer,
            Self::Spontaneous => PaymentRail::Spontaneous,
            Self::WaivedChannelFee => PaymentRail::WaivedFee,
            Self::WaivedLiquidityFee => PaymentRail::WaivedFee,
            Self::Unknown(s) => PaymentRail::Unknown(Box::from(format!(
                "(Unknown: parent of '{s}')"
            ))),
        }
    }

    /// Validates this payment kind against an expected parent rail.
    pub fn expect_rail(&self, expected: PaymentRail) -> anyhow::Result<()> {
        let actual = self.rail();

        ensure!(actual == expected, "Expected rail {expected}, got {actual}");

        Ok(())
    }
}
impl FromStr for PaymentKind {
    type Err = Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let kind = match s {
            "invoice" => Self::Invoice,
            "offer" => Self::Offer,
            "onchain" => Self::Onchain,
            "spontaneous" => Self::Spontaneous,
            "waived_channel_fee" => Self::WaivedChannelFee,
            "waived_liquidity_fee" => Self::WaivedLiquidityFee,
            unknown => Self::Unknown(Box::from(unknown)),
        };
        Ok(kind)
    }
}
impl Display for PaymentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_str())
    }
}
impl Serialize for PaymentKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_str().serialize(serializer)
    }
}

// --- impl PaymentDirection --- //

impl PaymentDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
            Self::Info => "info",
        }
    }
}
impl FromStr for PaymentDirection {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inbound" => Ok(Self::Inbound),
            "outbound" => Ok(Self::Outbound),
            "info" => Ok(Self::Info),
            _ => Err(anyhow!("Must be inbound|outbound|info")),
        }
    }
}
impl Display for PaymentDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl Serialize for PaymentDirection {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

// --- impl PaymentStatus --- //

impl PaymentStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    pub fn is_pending(&self) -> bool {
        match self {
            Self::Pending => true,
            Self::Completed | Self::Failed => false,
        }
    }

    pub fn is_finalized(&self) -> bool {
        match self {
            Self::Pending => false,
            Self::Completed | Self::Failed => true,
        }
    }
}
impl FromStr for PaymentStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(anyhow!("Must be pending|completed|failed")),
        }
    }
}
impl Display for PaymentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl Serialize for PaymentStatus {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

// --- PaymentCreatedIndex FromStr / Display impl --- //

/// `<created_at>-<id>`
// We use the - separator because LxPaymentId already uses _
impl FromStr for PaymentCreatedIndex {
    type Err = anyhow::Error;
    fn from_str(createdat_id: &str) -> anyhow::Result<Self> {
        let mut parts = createdat_id.split('-');

        let createdat_str = parts
            .next()
            .context("Missing created_at in <created_at>-<id>")?;
        let id_str = parts.next().context("Missing id in <created_at>-<id>")?;
        ensure!(
            parts.next().is_none(),
            "Wrong format; should be <created_at>-<id>"
        );

        let created_at = TimestampMs::from_str(createdat_str)
            .context("Invalid timestamp in <created_at>-<id>")?;
        let id = LxPaymentId::from_str(id_str)
            .context("Invalid payment id in <created_at>-<id>")?;

        Ok(Self { created_at, id })
    }
}

/// `<created_at>-<id>`
///
/// When serializing to string, pad the timestamp with leading zeroes (up to the
/// maximum number of digits in an [`i64`]) so that the lexicographic ordering
/// is equivalent to the non-serialized ordering.
// We use the - separator because LxPaymentId already uses _
impl Display for PaymentCreatedIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let created_at = self.created_at.to_i64();
        let id = &self.id;
        // i64 contains a maximum of 19 digits in base 10.
        write!(f, "{created_at:019}-{id}")
    }
}

// --- PaymentUpdatedIndex FromStr / Display impl --- //
//
// Format: `u<updated_at>-<id>`
//
// - We use the '-' separator because LxPaymentId already uses '_'.
// - We require a 'u' prefix to so that no `PaymentCreatedIndex` can be
//   interpreted as a `PaymentUpdatedIndex` and vice versa.

/// `u<updated_at>-<id>`
impl FromStr for PaymentUpdatedIndex {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let updatedat_id = s.strip_prefix('u').context(
            "PaymentUpdatedIndex must start with 'u'. \
                 Did you accidentally supply a PaymentCreatedIndex? ",
        )?;

        let mut parts = updatedat_id.split('-');

        let updatedat_str = parts
            .next()
            .context("Missing updated_at in u<updated_at>-<id>")?;
        let id_str =
            parts.next().context("Missing id in u<updated_at>-<id>")?;
        ensure!(
            parts.next().is_none(),
            "Wrong format; should be u<updated_at>-<id>"
        );

        let updated_at = TimestampMs::from_str(updatedat_str)
            .context("Invalid timestamp in u<updated_at>-<id>")?;
        let id = LxPaymentId::from_str(id_str)
            .context("Invalid payment id in u<updated_at>-<id>")?;

        Ok(Self { updated_at, id })
    }
}

/// `u<updated_at>-<id>`
//
// When serializing to string, pad the timestamp with leading zeroes (up to the
// maximum number of digits in an [`i64`]) so that the lexicographic ordering
// is equivalent to the non-serialized ordering.
impl Display for PaymentUpdatedIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let updated_at = self.updated_at.to_i64();
        let id = &self.id;
        // i64 contains a maximum of 19 digits in base 10.
        write!(f, "u{updated_at:019}-{id}")
    }
}

// --- LxPaymentId FromStr / Display impl --- //

/// `<kind>_<id>`
impl FromStr for LxPaymentId {
    type Err = anyhow::Error;
    fn from_str(kind_id: &str) -> anyhow::Result<Self> {
        let mut parts = kind_id.split('_');
        let kind_str = parts.next().context("Missing kind in <kind>_<id>")?;
        let id_str = parts.next().context("Missing id in <kind>_<id>")?;
        ensure!(
            parts.next().is_none(),
            "Wrong format; should be <kind>_<id>"
        );
        match kind_str {
            "fr" => LnClaimId::from_str(id_str)
                .map(Self::OfferRecvReusable)
                .context("Invalid claim id"),
            "fs" => ClientPaymentId::from_str(id_str)
                .map(Self::OfferSend)
                .context("Invalid ClientPaymentId"),
            "ln" => LxPaymentHash::from_str(id_str)
                .map(Self::Lightning)
                .context("Invalid payment hash"),
            "or" => LxTxid::from_str(id_str)
                .map(Self::OnchainRecv)
                .context("Invalid Txid"),
            "os" => ClientPaymentId::from_str(id_str)
                .map(Self::OnchainSend)
                .context("Invalid ClientPaymentId"),
            _ => bail!("<kind> should be fi|fr|fs|ln|or|os"),
        }
    }
}

/// `<kind>_<id>`
impl Display for LxPaymentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = self.prefix();
        match self {
            Self::OfferRecvReusable(claim_id) =>
                write!(f, "{prefix}_{claim_id}"),
            Self::OfferSend(cid) => write!(f, "{prefix}_{cid}"),
            Self::Lightning(hash) => write!(f, "{prefix}_{hash}"),
            Self::OnchainRecv(txid) => write!(f, "{prefix}_{txid}"),
            Self::OnchainSend(cid) => write!(f, "{prefix}_{cid}"),
        }
    }
}

#[cfg(test)]
mod test {
    use std::fs;

    use common::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip, snapshot},
    };
    use proptest::{arbitrary::any, prop_assert, prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn enums_roundtrips() {
        // Unit enums: full backwards compat check
        let expected_ser = r#"["inbound","outbound","info"]"#;
        roundtrip::json_unit_enum_backwards_compat::<PaymentDirection>(
            expected_ser,
        );
        let expected_ser = r#"["pending","completed","failed"]"#;
        roundtrip::json_unit_enum_backwards_compat::<PaymentStatus>(
            expected_ser,
        );

        // PaymentRail and PaymentKind have Unknown variants
        let expected_ser =
            r#"["onchain","invoice","offer","spontaneous","waived_fee"]"#;
        roundtrip::json_unit_enum_backwards_compat_with_unknown(
            PaymentRail::KNOWN_VARIANTS,
            expected_ser,
        );
        let expected_ser = r#"["onchain","invoice","offer","spontaneous","waived_channel_fee","waived_liquidity_fee"]"#;
        roundtrip::json_unit_enum_backwards_compat_with_unknown(
            PaymentKind::KNOWN_VARIANTS,
            expected_ser,
        );

        // FromStr/Display roundtrip
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentDirection>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentStatus>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentRail>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentKind>();

        // JSON string roundtrip
        roundtrip::json_string_roundtrip_proptest::<PaymentDirection>();
        roundtrip::json_string_roundtrip_proptest::<PaymentStatus>();
        roundtrip::json_string_roundtrip_proptest::<PaymentRail>();
        roundtrip::json_string_roundtrip_proptest::<PaymentKind>();
    }

    #[test]
    fn newtype_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<PaymentCreatedIndex>();
        roundtrip::json_string_roundtrip_proptest::<PaymentUpdatedIndex>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentId>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentPreimage>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentSecret>();
        roundtrip::json_string_roundtrip_proptest::<LxOfferId>();
        roundtrip::json_string_roundtrip_proptest::<LnClaimId>();
    }

    #[test]
    fn newtype_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentCreatedIndex>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentUpdatedIndex>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentId>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxOfferId>();
        roundtrip::fromstr_display_roundtrip_proptest::<LnClaimId>();
        // `Display` for `LxPaymentPreimage` and `LxPaymentSecret` are redacted
        // roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentPreimage>();
        // roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentSecret>();
    }

    #[test]
    fn payment_index_createdat_precedence() {
        let time1 = TimestampMs::from_secs_u32(1);
        let time2 = TimestampMs::from_secs_u32(2);
        let id1 = LxPaymentId::Lightning(LxPaymentHash([1; 32]));
        let id2 = LxPaymentId::Lightning(LxPaymentHash([2; 32]));

        let index12 = PaymentCreatedIndex {
            created_at: time1,
            id: id2,
        };
        let index21 = PaymentCreatedIndex {
            created_at: time2,
            id: id1,
        };

        assert!(index12 < index21, "created_at should take precedence");
    }

    #[test]
    fn payment_index_updatedat_precedence() {
        let time1 = TimestampMs::from_secs_u32(1);
        let time2 = TimestampMs::from_secs_u32(2);
        let id1 = LxPaymentId::Lightning(LxPaymentHash([1; 32]));
        let id2 = LxPaymentId::Lightning(LxPaymentHash([2; 32]));

        let index12 = PaymentUpdatedIndex {
            updated_at: time1,
            id: id2,
        };
        let index21 = PaymentUpdatedIndex {
            updated_at: time2,
            id: id1,
        };

        assert!(index12 < index21, "updated_at should take precedence");
    }

    #[test]
    fn payment_created_index_ordering_equivalence() {
        proptest!(|(
            idx1 in any::<PaymentCreatedIndex>(),
            idx2 in any::<PaymentCreatedIndex>()
        )| {
            let idx1_str = idx1.to_string();
            let idx2_str = idx2.to_string();

            let unserialized_order = idx1.cmp(&idx2);
            let string_serialized_order = idx1_str.cmp(&idx2_str);
            prop_assert_eq!(unserialized_order, string_serialized_order);
        });
    }

    #[test]
    fn payment_updated_index_ordering_equivalence() {
        proptest!(|(
            idx1 in any::<PaymentUpdatedIndex>(),
            idx2 in any::<PaymentUpdatedIndex>()
        )| {
            let idx1_str = idx1.to_string();
            let idx2_str = idx2.to_string();

            let unserialized_order = idx1.cmp(&idx2);
            let string_serialized_order = idx1_str.cmp(&idx2_str);
            prop_assert_eq!(unserialized_order, string_serialized_order);
        });
    }

    // ∀ idx ∈ PaymentCreatedIndex, MIN <= idx <= MAX
    // ∀  id ∈ LxPaymentId , MIN <= id <= MAX
    #[test]
    fn payment_created_index_bounds() {
        fn assert_bounds(
            idx: PaymentCreatedIndex,
        ) -> Result<(), proptest::prelude::TestCaseError> {
            // PaymentCreatedIndex bounds
            prop_assert!(matches!(
                PaymentCreatedIndex::MIN.cmp(&idx),
                Ordering::Less | Ordering::Equal,
            ));
            prop_assert!(matches!(
                idx.cmp(&PaymentCreatedIndex::MAX),
                Ordering::Less | Ordering::Equal,
            ));

            // LxPaymentId bounds
            prop_assert!(matches!(
                LxPaymentId::MIN.cmp(&idx.id),
                Ordering::Less | Ordering::Equal,
            ));
            prop_assert!(matches!(
                idx.id.cmp(&LxPaymentId::MAX),
                Ordering::Less | Ordering::Equal,
            ));

            Ok(())
        }

        proptest!(|(idx in any::<PaymentCreatedIndex>())| {
            assert_bounds(idx)?;

            assert_bounds(PaymentCreatedIndex {
                created_at: TimestampMs::MIN,
                id: idx.id,
            })?;
            assert_bounds(PaymentCreatedIndex {
                created_at: TimestampMs::MAX,
                id: idx.id,
            })?;
            assert_bounds(PaymentCreatedIndex {
                created_at: idx.created_at,
                id: LxPaymentId::MIN,
            })?;
            assert_bounds(PaymentCreatedIndex {
                created_at: idx.created_at,
                id: LxPaymentId::MAX,
            })?;
        });
    }

    // ∀ idx ∈ PaymentUpdatedIndex, MIN <= idx <= MAX
    // ∀  id ∈ LxPaymentId , MIN <= id <= MAX
    #[test]
    fn payment_updated_index_bounds() {
        fn assert_bounds(
            idx: PaymentUpdatedIndex,
        ) -> Result<(), proptest::prelude::TestCaseError> {
            // PaymentUpdatedIndex bounds
            prop_assert!(matches!(
                PaymentUpdatedIndex::MIN.cmp(&idx),
                Ordering::Less | Ordering::Equal,
            ));
            prop_assert!(matches!(
                idx.cmp(&PaymentUpdatedIndex::MAX),
                Ordering::Less | Ordering::Equal,
            ));

            // LxPaymentId bounds
            prop_assert!(matches!(
                LxPaymentId::MIN.cmp(&idx.id),
                Ordering::Less | Ordering::Equal,
            ));
            prop_assert!(matches!(
                idx.id.cmp(&LxPaymentId::MAX),
                Ordering::Less | Ordering::Equal,
            ));

            Ok(())
        }

        proptest!(|(idx in any::<PaymentUpdatedIndex>())| {
            assert_bounds(idx)?;

            assert_bounds(PaymentUpdatedIndex {
                updated_at: TimestampMs::MIN,
                id: idx.id,
            })?;
            assert_bounds(PaymentUpdatedIndex {
                updated_at: TimestampMs::MAX,
                id: idx.id,
            })?;
            assert_bounds(PaymentUpdatedIndex {
                updated_at: idx.updated_at,
                id: LxPaymentId::MIN,
            })?;
            assert_bounds(PaymentUpdatedIndex {
                updated_at: idx.updated_at,
                id: LxPaymentId::MAX,
            })?;
        });
    }

    #[test]
    fn payment_index_incompatible() {
        // Parsing `PaymentCreatedIndex` from `PaymentUpdatedIndex` string fails
        proptest!(|(
            created_idx in any::<PaymentCreatedIndex>(),
        )| {
            let created_str = created_idx.to_string();
            let parsed_updated =
                PaymentUpdatedIndex::from_str(&created_str);
            prop_assert!(parsed_updated.is_err());
        });

        // Parsing `PaymentUpdatedIndex` from `PaymentCreatedIndex` string fails
        proptest!(|(
            updated_idx in any::<PaymentUpdatedIndex>(),
        )| {
            let updated_str = updated_idx.to_string();
            let parsed_created =
                PaymentCreatedIndex::from_str(&updated_str);
            prop_assert!(parsed_created.is_err());
        });
    }

    #[test]
    fn payment_id_ordering_equivalence() {
        proptest!(|(id1 in any::<LxPaymentId>(), id2 in any::<LxPaymentId>())| {
            let id1_str = id1.to_string();
            let id2_str = id2.to_string();

            let unserialized_order = id1.cmp(&id2);
            let string_serialized_order = id1_str.cmp(&id2_str);
            prop_assert_eq!(unserialized_order, string_serialized_order);
        });
    }

    // ```bash
    // cargo test -p lexe-api-core --lib -- payment_id_gen_sample_data --ignored --nocapture
    // ```
    #[ignore]
    #[test]
    fn payment_id_gen_sample_data() {
        let mut rng = FastRng::from_u64(202504212022);
        let mut ids =
            arbitrary::gen_values(&mut rng, any::<LxPaymentId>(), 100);
        // only need one of each prefix
        ids.sort_unstable();
        ids.dedup_by_key(|id| id.prefix());
        for id in ids {
            println!("{id}");
        }
    }

    #[test]
    fn payment_id_deser_compat() {
        let inputs = r#"
--- v1
ln_003690453dac3e6c29db4e930c80f797fe0a05bc43ed2d9cff2a62fb7407d3e0
or_3045b3cf002d40b2ae2e2f0f4b3d657cad3d2d8995988fb78ce488d9ec7d8f30
os_0a19f5f961bc67b109ce060743141b59dad6cd1edc28a7dd72241fe97da407b3
--- v2 (bolt12 offers)
fr_00e32fe42d1249bd1299a2839c017584b09a924f935a5da5b121346950d2676d
fs_00996e6b999900e8e7273934a7f272eb367fd2ac394f10b3ea1c7164d212c5c5
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let value1 = LxPaymentId::from_str(input).unwrap();
            let output = value1.to_string();
            let value2 = LxPaymentId::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }

    // NOTE: see `lexe_ln::payments::test::gen_basic_payment_sample_data` to
    // generate new sample data.
    #[test]
    #[cfg_attr(target_env = "sgx", ignore = "Can't read files in SGX")]
    fn basic_payment_deser_compat() {
        let snapshot =
            fs::read_to_string("data/basic_payment_snapshot.txt").unwrap();

        for input in snapshot::parse_sample_data(&snapshot) {
            let value1: BasicPaymentV1 = serde_json::from_str(input).unwrap();
            let output = serde_json::to_string(&value1).unwrap();
            let value2: BasicPaymentV1 = serde_json::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }
}
