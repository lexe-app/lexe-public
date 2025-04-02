use std::{
    cmp::Ordering,
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure, Context};
use bitcoin_hashes::{sha256, Hash};
use byte_array::ByteArray;
use lightning::{
    ln::channelmanager::PaymentId,
    types::payment::{PaymentHash, PaymentPreimage, PaymentSecret},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use ref_cast::RefCast;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    hexstr_or_bytes,
    ln::{amount::Amount, hashes::LxTxid, invoice::LxInvoice},
    rng::{RngCore, RngExt},
    time::TimestampMs,
};

// --- Top-level payment types --- //

/// A basic payment type which contains all of the user-facing payment details
/// for any kind of payment. These details are exposed in the Lexe app.
///
/// It is essentially the `Payment` type flattened out such that each field is
/// the result of the corresponding `Payment` getter.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BasicPayment {
    pub index: PaymentIndex,

    pub kind: PaymentKind,
    pub direction: PaymentDirection,

    /// (Invoice payments only) The BOLT11 invoice used in this payment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<LxInvoice>,

    /// (Onchain payments only) The original txid.
    // NOTE: we're duplicating the txid here for onchain receives because its
    // less error prone to use, esp. for external API consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub txid: Option<LxTxid>,

    /// (Onchain payments only) The txid of the replacement tx, if one exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<LxTxid>,

    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, this is the amount we
    ///   received.
    /// - If this is a pending or failed inbound inbound invoice payment, this
    ///   is the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Amount>,

    /// The fees for this payment.
    ///
    /// - For outbound Lightning payments, these are the routing fees. If the
    ///   payment is not completed, this value is an estimation only. Iff the
    ///   payment completes, this value reflects actual fees paid.
    /// - For inbound Lightning payments, the routing fees are not paid by us
    ///   (the recipient), but if a JIT channel open was required to facilitate
    ///   this payment, then the on-chain fee is reflected here.
    pub fees: Amount,

    pub status: PaymentStatus,

    /// The payment status as a human-readable string. These strings are
    /// customized per payment type, e.g. "invoice generated", "timed out"
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub status_str: String,

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
    /// - Inbound spontaneous payment: There is no way for users to add the
    ///   note at the time of receiving an inbound spontaneous payment, so this
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized_at: Option<TimestampMs>,
}

/// An upgradeable version of [`Vec<BasicPayment>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecBasicPayment {
    pub payments: Vec<BasicPayment>,
}

/// An encrypted payment, as represented in the DB.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbPayment {
    pub created_at: i64,
    pub id: String,
    pub status: String,
    pub data: Vec<u8>,
}

/// An upgradeable version of [`Option<DbPayment>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeDbPayment {
    pub maybe_payment: Option<DbPayment>,
}

/// An upgradeable version of [`Vec<DbPayment>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecDbPayment {
    pub payments: Vec<DbPayment>,
}

/// Specifies whether this is an onchain payment, LN invoice payment, etc.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum PaymentKind {
    Onchain,
    Invoice,
    Spontaneous,
}

/// Specifies whether a payment is inbound or outbound.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum PaymentDirection {
    Inbound,
    Outbound,
}

/// A general payment status that abstracts over all payment types.
///
/// - Useful for filtering all payments by status in a high-level list view.
/// - Not suitable for getting detailed information about a specific payment; in
///   this case, use the payment-specific status enum or `status_str()` instead.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[cfg_attr(test, derive(strum::VariantArray))]
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

// --- Lexe newtypes --- //

/// A payment identifier which (1) retains uniqueness per payment and (2) is
/// ordered first by timestamp and then by [`LxPaymentId`].
///
/// It is essentially a [`(TimestampMs, LxPaymentId)`], suitable for use as a
/// key in a `BTreeMap<PaymentIndex, BasicPayment>` or similar.
///
/// It can also be degenerated (serialized) into a string and the
/// string-serialized ordering will be equivalent to the unserialized ordering.
///
/// ### Examples
///
/// ```ignore
/// 0002683862736062841-bc_95cc800f4f3b5669c71c85f7096be45a172ca86aef460e0e584affff3ea80bee
/// 0009557253037960566-ln_3ddcfd0e0b1eba77292c23a7de140c1e71327ac97486cc414b6826c434c560cc
/// 4237937319278351047-bc_3f6d2153bde1a0878717f46a1cbc63c48f7b4231224d78a50eb9e94b5d29f674
/// 6206503357534413026-bc_063a5be0218332a84f9a4f7f4160a7dcf8e9362b9f5043ad47360c7440037fa8
/// 6450440432938623603-ln_0db1f1ebed6f99574c7a048e6bbf68c7db69c6da328f0b6d699d4dc1cd477017
/// 7774176661032219027-bc_215ef16c8192c8d674b519a34b7b65454e1e18d48bf060bdc333df433ada0137
/// 8468903867373394879-ln_b8cbf827292c2b498e74763290012ed92a0f946d67e733e94a5fedf7f82710d5
/// 8776421933930532767-bc_ead3c01be0315dfd4e4c405aaca0f39076cff722a0f680c89c348e3bda9575f3
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct PaymentIndex {
    pub created_at: TimestampMs,
    pub id: LxPaymentId,
}

/// A globally-unique identifier for any type of payment, including both
/// on-chain and Lightning payments.
///
/// - On-chain sends use a [`ClientPaymentId`] as their id.
/// - On-chain receives use their [`LxTxid`] as their id.
/// - Lightning payments use their [`LxPaymentHash`] as their id.
/// - TODO(max): Revisit most appropriate ID type for BOLT 12
///
/// NOTE that this is NOT a drop-in replacement for LDK's [`PaymentId`], since
/// [`PaymentId`] is Lightning-specific, whereas [`LxPaymentId`] is not.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum LxPaymentId {
    OnchainSend(ClientPaymentId),
    OnchainRecv(LxTxid),
    Lightning(LxPaymentHash),
}

/// An upgradeable version of [`Vec<LxPaymentId>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecLxPaymentId {
    pub ids: Vec<LxPaymentId>,
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
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LxPaymentSecret(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

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
#[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast, Serialize, Deserialize)]
#[repr(transparent)]
pub struct LnClaimId(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

// --- impl BasicPayment --- //

impl BasicPayment {
    #[inline]
    pub fn index(&self) -> &PaymentIndex {
        &self.index
    }

    #[inline]
    pub fn created_at(&self) -> TimestampMs {
        self.index.created_at
    }

    #[inline]
    pub fn payment_id(&self) -> LxPaymentId {
        self.index.id
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
        let junk_amountless_invoice = self.status != PaymentStatus::Completed
            && self.kind == PaymentKind::Invoice
            && self.direction == PaymentDirection::Inbound
            && (self.amount.is_none() || self.note_or_description().is_none());

        // TODO(phlip9): also don't show pending/failed "superseded" invoices,
        // where the user edited the amount/description.

        junk_amountless_invoice
    }

    /// Returns the user's note or invoice description, prefering note over
    /// description.
    pub fn note_or_description(&self) -> Option<&str> {
        let maybe_note = self.note.as_deref().filter(|s| !s.is_empty());

        maybe_note.or_else(|| {
            self.invoice.as_ref().and_then(LxInvoice::description_str)
        })
    }
}

impl PartialOrd for BasicPayment {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.index.partial_cmp(&other.index)
    }
}

// --- impl PaymentIndex --- //

impl PaymentIndex {
    /// Quickly create a dummy [`PaymentIndex`] which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_u8(i: u8) -> Self {
        let created_at = TimestampMs::from(u32::from(i));
        let id = LxPaymentId::Lightning(LxPaymentHash([i; 32]));
        Self { created_at, id }
    }
}

// --- impl LxPaymentId --- //

impl LxPaymentId {
    /// Returns the prefix to use when serializing this payment id to a string.
    pub fn prefix(&self) -> &'static str {
        match self {
            Self::OnchainSend(_) => "os",
            Self::OnchainRecv(_) => "or",
            Self::Lightning(_) => "ln",
        }
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
byte_array::impl_byte_array!(LnClaimId, 32);

byte_array::impl_fromstr_from_hexstr!(ClientPaymentId);
byte_array::impl_fromstr_from_hexstr!(LxPaymentHash);
byte_array::impl_fromstr_from_hexstr!(LxPaymentPreimage);
byte_array::impl_fromstr_from_hexstr!(LxPaymentSecret);
byte_array::impl_fromstr_from_hexstr!(LnClaimId);

byte_array::impl_debug_display_as_hex!(ClientPaymentId);
byte_array::impl_debug_display_as_hex!(LxPaymentHash);
byte_array::impl_debug_display_as_hex!(LnClaimId);
// Redacted to prevent accidentally leaking secrets in logs
byte_array::impl_debug_display_redacted!(LxPaymentPreimage);
byte_array::impl_debug_display_redacted!(LxPaymentSecret);

// --- Newtype From impls --- //

// NOTE(phlip9): previously we had conversions for:
//      ClientPaymentId -> LxPaymentId::OnchainSend
//               LxTxid -> LxPaymentId::OnchainRecv
//
// but this ended up causing some bugs after refactoring:
//  OnchainSend(LxTxid) -> OnchainSend(ClientPaymentId)
//
// on that note... <eyes emoji>
// ...we should probably reevalute this conversion, since OutboundSpontaneous
// will probably need a separate idempotency id.

impl From<LxPaymentHash> for LxPaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self::Lightning(hash)
    }
}

// LxPaymentId -> ClientPaymentId / Txid / LxPaymentHash
impl TryFrom<LxPaymentId> for ClientPaymentId {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        use LxPaymentId::*;
        match id {
            OnchainSend(cid) => Ok(cid),
            OnchainRecv(_) | Lightning(_) => bail!("Not an onchain send"),
        }
    }
}
impl TryFrom<LxPaymentId> for LxPaymentHash {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        use LxPaymentId::*;
        match id {
            Lightning(hash) => Ok(hash),
            OnchainSend(_) | OnchainRecv(_) => bail!("Not a lightning payment"),
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
impl From<LnClaimId> for lightning::ln::channelmanager::PaymentId {
    fn from(id: LnClaimId) -> Self {
        Self(id.0)
    }
}

// For BOLT11 payments, we use LxPaymentHash as our PaymentId.
// For BOLT12 payments, we use a client-generated id.
// TODO(max): Revisit for BOLT 12: Use separate Bolt12PaymentId?
impl From<LxPaymentHash> for PaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
    }
}

// --- impl PaymentKind --- //

impl PaymentKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Onchain => "onchain",
            Self::Invoice => "invoice",
            Self::Spontaneous => "spontaneous",
        }
    }
}
impl FromStr for PaymentKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "onchain" => Ok(Self::Onchain),
            "invoice" => Ok(Self::Invoice),
            "spontaneous" => Ok(Self::Spontaneous),
            _ => Err(anyhow!("Must be onchain|invoice|spontaneous")),
        }
    }
}
impl Display for PaymentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl Serialize for PaymentKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

// --- impl PaymentDirection --- //

impl PaymentDirection {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}
impl FromStr for PaymentDirection {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inbound" => Ok(Self::Inbound),
            "outbound" => Ok(Self::Outbound),
            _ => Err(anyhow!("Must be inbound|outbound")),
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
    fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
            Self::Failed => "failed",
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

// --- PaymentIndex FromStr / Display impl --- //

/// `<created_at>-<id>`
// We use the - separator because LxPaymentId already uses _
impl FromStr for PaymentIndex {
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
impl Display for PaymentIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let created_at = self.created_at.as_i64();
        let id = &self.id;
        // i64 contains a maximum of 19 digits in base 10.
        write!(f, "{created_at:019}-{id}")
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
            "os" => ClientPaymentId::from_str(id_str)
                .map(Self::OnchainSend)
                .context("Invalid ClientPaymentId"),
            "or" => LxTxid::from_str(id_str)
                .map(Self::OnchainRecv)
                .context("Invalid Txid"),
            "ln" => LxPaymentHash::from_str(id_str)
                .map(Self::Lightning)
                .context("Invalid payment hash"),
            _ => bail!("<kind> should be 'os', 'or', or 'ln'"),
        }
    }
}

/// `<kind>_<id>`
impl Display for LxPaymentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = self.prefix();
        match self {
            Self::OnchainSend(client_id) => write!(f, "{prefix}_{client_id}"),
            Self::OnchainRecv(txid) => write!(f, "{prefix}_{txid}"),
            Self::Lightning(hash) => write!(f, "{prefix}_{hash}"),
        }
    }
}

// --- impl Ord for LxPaymentId --- //

/// Defines an ordering such that the string-serialized and unserialized
/// orderings are equivalent.
impl Ord for LxPaymentId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            // If the kinds match, use their inner orderings
            (Self::OnchainSend(self_cid), Self::OnchainSend(other_cid)) =>
                self_cid.cmp(other_cid),
            (Self::OnchainRecv(self_txid), Self::OnchainRecv(other_txid)) =>
                self_txid.cmp(other_txid),
            (Self::Lightning(self_hash), Self::Lightning(other_hash)) =>
                self_hash.cmp(other_hash),
            // Otherwise, use the string prefix ordering 'ln' < 'or' < 'os'
            (s, o) => s.prefix().cmp(o.prefix()),
        }
    }
}

impl PartialOrd for LxPaymentId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;
    use crate::test_utils::{roundtrip, snapshot};

    #[test]
    fn enums_roundtrips() {
        let expected_ser = r#"["inbound","outbound"]"#;
        roundtrip::json_unit_enum_backwards_compat::<PaymentDirection>(
            expected_ser,
        );
        let expected_ser = r#"["pending","completed","failed"]"#;
        roundtrip::json_unit_enum_backwards_compat::<PaymentStatus>(
            expected_ser,
        );
        let expected_ser = r#"["onchain","invoice","spontaneous"]"#;
        roundtrip::json_unit_enum_backwards_compat::<PaymentKind>(expected_ser);

        roundtrip::fromstr_display_roundtrip_proptest::<PaymentDirection>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentStatus>();
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentKind>();
    }

    #[test]
    fn newtype_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<PaymentIndex>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentId>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentPreimage>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentSecret>();
        roundtrip::json_string_roundtrip_proptest::<LnClaimId>();
    }

    #[test]
    fn newtype_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentIndex>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentId>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::fromstr_display_roundtrip_proptest::<LnClaimId>();
        // `Display` for `LxPaymentPreimage` and `LxPaymentSecret` are redacted
        // roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentPreimage>();
        // roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentSecret>();
    }

    #[test]
    fn payment_index_createdat_precedence() {
        let time1 = TimestampMs::from(1);
        let time2 = TimestampMs::from(2);
        let id1 = LxPaymentId::Lightning(LxPaymentHash([1; 32]));
        let id2 = LxPaymentId::Lightning(LxPaymentHash([2; 32]));

        let index12 = PaymentIndex {
            created_at: time1,
            id: id2,
        };
        let index21 = PaymentIndex {
            created_at: time2,
            id: id1,
        };

        assert!(index12 < index21, "created_at should take precedence");
    }

    #[test]
    fn payment_index_ordering_equivalence() {
        proptest!(|(
            idx1 in any::<PaymentIndex>(),
            idx2 in any::<PaymentIndex>()
        )| {
            let idx1_str = idx1.to_string();
            let idx2_str = idx2.to_string();

            let unserialized_order = idx1.cmp(&idx2);
            let string_serialized_order = idx1_str.cmp(&idx2_str);
            prop_assert_eq!(unserialized_order, string_serialized_order);
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

    // NOTE: see `lexe_ln::payments::test::gen_basic_payment_sample_data` to
    // generate new sample data.
    #[test]
    fn basic_payment_deser_compat() {
        let inputs = r#"
--- v1
--- OnchainSend
{"index":"2352250271958571112-os_0c83e7486ccf8b662e1a57eaa67b3a9cf73d312625ffaf155e27e19c6896330e","kind":"onchain","direction":"outbound","invoice":null,"replacement":null,"amount":"426295955721124.011","fees":"106243014155694.28","status":"pending","status_str":"partially confirmed (1-5 confirmations)","note":"foo bar","finalized_at":922031411621277542}
{"index":"6662034773613201778-os_160d1659b347941dcf067ef4b7053355b058640a86e4fc8f3865c82da9b5a6cb","kind":"onchain","direction":"outbound","invoice":null,"replacement":"21767abd07cfa29e6f1415ae0ea4b447b0aa212a01ac974c9d680ab199d79a28","amount":"1283455089118142.425","fees":"1195902608470070.026","status":"pending","status_str":"broadcasted","note":null,"finalized_at":7724772872675692842}
{"index":"7513172493835928492-os_189a8ddf5f0910096c5610dabce731a2a7d27100df1185f3d9c1944596c96199","kind":"onchain","direction":"outbound","invoice":null,"replacement":null,"amount":"2069602468658948.217","fees":"1347441650278795.035","status":"pending","status_str":"created","note":null,"finalized_at":8681440472811127301}
--- OnchainReceive
{"index":"5806002936929143706-or_6e2a71f56aab3a33a2b0c2cfe8a092883f9783e5cb599b5d5c5b264d5c4e2669","kind":"onchain","direction":"inbound","invoice":null,"replacement":null,"amount":"1026373441666394.174","fees":"0","status":"pending","status_str":"partially confirmed (1-5 confirmations)","note":null,"finalized_at":3872673315358973283}
{"index":"8270747644397781506-or_805b79f828994ebb2e06b6b0a2f3fda1b52da1417335a35a826452c988be6e1c","kind":"onchain","direction":"inbound","invoice":null,"replacement":"454737de4ff8795fb3b86b817a68701c90241190b425946cb3404232a10302f7","amount":"414234636362640.08","fees":"0","status":"failed","status_str":"dropped from mempool","note":null,"finalized_at":6727882137524031451}
{"index":"1667254218639325659-or_92ca95723279675661dc51bcacbf4bf6d7d96ab13c1fd71fcc8fbbe08aa6f81d","kind":"onchain","direction":"inbound","invoice":null,"replacement":null,"amount":"2077078592479909.515","fees":"0","status":"pending","status_str":"being replaced (replacement has 1-5 confirmations)","note":"foo bar","finalized_at":7581178487696199344}
--- InboundInvoice
{"index":"4725737965850879943-ln_bc29d8f882abed198ce434cd50d01b5ca840fdaef3d239289c24b4b6efb5053f","kind":"invoice","direction":"inbound","invoice":"lntb18446744073709551610p1ptueg0qdr97xtfnwlwnxdjvznl8tc6f9dg8ghzfudcn7hjw0mq72jgrgtrcwgj4u9p56tzdmamhaxn4uu2jj3h7283s75f8u5unxn08y4f5u9yspp57z23mnrrsvpehpshk4k9g6y5su4nk2ka74lts8pyc4m43g6mkvtqsp5pfg2c0wuxft2ueaqpp2n83egyjrs22mlsjqtlkcqynqrzmm3jgxs9qyysgqcqr0r9np4qdfsmtw7dv7z66qw24t4zcsdyuy07tgrelewu3ll2qsdtjdh3mfuvfppq756g7n33q4s2kym4udpnh8g4u5pxd2zvr9yqth0u8lycwzxxr8pkszl4qz035cm7knx7naly983rr3pvjkm0rn0zr9cn8utuxju02d6jzq0nz5npjvurypjlwd9t04nwpnke7mky4ajekj2gd8exnydnumra06gm390he0vldj8zhnyuhdw3jf87phmckswkmx2284s5ush5h2f7r0pwlm8pwrmlevcgecqclkflq7c7wr3dqv65g5v8wuqwgg6uynpxvstvxr4ka7janu873kqc5r36jzd0gwv8r5jl97tsdgphh5a5l","replacement":null,"amount":"1675262874117511.115","fees":"1986679708464435.976","status":"pending","status_str":"claiming","note":"foo bar","finalized_at":null}
{"index":"2256768884000796422-ln_c34e891542810683ad1a0792f8eb783bbc8b090e469e3a91debe286d1b9e6383","kind":"invoice","direction":"inbound","invoice":"lntb1283l5kphp534m26n0hqdj7xu8cf2jjnguw46qe4485ssptyqnmq6w2e20vcvmspp57wxq77d89q6qvva3h30h9whd586uys3w8g6w5wnay9cqsc09ygvqsp5vyk5n3rppdxqfvgyzthx85ydy2pclw9e4nxxkm44khca7eehef8q9qyysgqcqrs7txq8lllllllfppjf45w7h9luhsx3m7x4zsckl9arc64yuus4xlxt9gzcnj7rmhgculj59zyh5lqa8serjpz6ltu0w9vgzyydal4h5685jn9n9m5e2je77z5uj9s9r8hkfcqhx7lftqh959p93lthpgp5xwsmd","replacement":null,"amount":"1071637668292635.607","fees":"0","status":"pending","status_str":"claiming","note":null,"finalized_at":4909338776287845535}
{"index":"0966462693570159466-ln_f690bc3827b3de355373f3259367875a05c3dd502d726398a217989ba3e9e337","kind":"invoice","direction":"inbound","invoice":"lnbc1pw83lxzds5ts93kvmm7xgm8rf29mhml0gtp9sqpevyhujn66kghg5wlwaluz3m7f8sn72mfu9hj6v0rw5l3sxu99hjs7neyaqf7xrgmr05s6d2cf83n6ct8ay23kn0r99ch3wdr2pl7xetltwrs3au98f6vmp2tmalh5h9eeu8kmhml02ua7amluyljk6qh5dg6x5z7t207z0640hsj2k6fuucj73z2tl3szdejwnlvgdkvqp87w72fdkzjlcm9pdta7lmmmalh5lkts49yghh7q8zszhrej96yp7u8z7r30hmh0e685syls4etuuzfay95knz4malh5qxqrf27xn68yn7a7lm620j46h2tuumjxlzk7cdxsjv9ftq7w063d2fqq3z2h32w0eghzv072z2pw7r5nhml00tnw508yvns57s5uh3n74mw5327wugmd8s5z6t0j96yle2fzvdynhmh0m8gyyjfs4y7z0etdqq7zkg4zmmycdl80akntcfl9d5ycjzstey9g3nw52vp53za5dgc2yqpp5k6zathnul34ttcl5y5nq8z2rv4g8mv5ec6lfnqcxu02x4kn6sk5qsp5wkj3qnuxwsj59j26wnydu0yng77jz94dfat3zcway24dgcg4vngq9qyysgqcqrlz4fppj6pnpnz5fgvk9v8v5g0l6rqpjwywwzhhwrzjq2ju7ggyarj7f3y8yc6qpv4h3a67fnvmwy9d2w96w0y2jt9njhwg28fan3f82dfdm3kzahez2fxkdsjpycw40zvc5trq972gyhsl44plx3v66t44hhk6c2f28vzkqzqft3r9dxvvtkngl88kdljdtssmzetzrj454qy67txsffe87hhzyjt8yddyqp5hte3p","replacement":null,"amount":"563859865662737.147","fees":"1360802405875876.968","status":"failed","status_str":"invoice expired","note":"foo bar","finalized_at":null}
--- InboundSpontaneous
{"index":"0263933984505265604-ln_2000815f864791a5d9b10e0e72f75d09096ee4800d52d3bb90e8649135e3815a","kind":"spontaneous","direction":"inbound","invoice":null,"replacement":null,"amount":"913120818704807.97","fees":"0","status":"completed","status_str":"completed","note":null,"finalized_at":3136200261222996439}
{"index":"0559588758736175637-ln_8b416294218b1042148a4a2715e501aad76350d8616cef3b54ca3f7de53e310e","kind":"spontaneous","direction":"inbound","invoice":null,"replacement":null,"amount":"658204045720138.099","fees":"1437970496572162.885","status":"pending","status_str":"claiming","note":"foo bar","finalized_at":null}
{"index":"8251254704841858744-ln_1bd44f2cacfac849ef1657c047f890be52fd675985c9cab82038665380594863","kind":"spontaneous","direction":"inbound","invoice":null,"replacement":null,"amount":"534425667243574.803","fees":"0","status":"completed","status_str":"completed","note":null,"finalized_at":5744690233519079911}
--- OutboundInvoice
{"index":"3967145603839053663-ln_c28ce106b0ec5393371dde088a91465db79137c242b993f39cbd93463d62303a","kind":"invoice","direction":"outbound","invoice":"lnbc16617464075412908110p1du587g2hp5j25fhdvhz66tctmq6xga76mdddz62xm7fk6n7alekxd4d8y2rqyspp5axv2wz5w2upckqf0exq3wwg09kkha9zushwm6tzmqsenkgafeyeqsp5usuarqswxgkpk6skvydnyaw56xpv400xy0auh8zjg3r4t9mqp8fs9qyysgqcqypvxfx04nrfq3khmpat4e9k5a2d52eup290rkts344t7z942zzhmk70h9ywtwu6kqc8hpx3af6tjakute4xq29h3rhdq50vcecjrxredxh0gqlm8nvn","replacement":null,"amount":"1388110017620496.458","fees":"1989134516334956.22","status":"pending","status_str":"unknown error, app is likely out-of-date","note":null,"finalized_at":null}
{"index":"1506781100894234982-ln_723bea0a83ee7d8f4747cabd15292d1eaaaff42fee95479d7aca41cf6f68a954","kind":"invoice","direction":"outbound","invoice":"lnbc1mmj7z2hd427xtea2gtw8et4p5ta7lm6xe02nemhxvg7zse98734qudr2pucwaz3ua647tl9tv8nsnv3whzszhv89lnhwum9u4vk284sfr6c2jnee9yhcn08qd65ghznuac4w5l9fahknyt4sugg0p22tqqdsxlrf465fwzvupw7z7tm9p97wyerp8j5676zafx8s9rlj967x4cld6dty3w9q9w7zvm8wl3kkacj4l3hkzcauutj630rx5pnp4l805v50g63ayd5w7lr8yrsvqxq3ljh6mf0uuxsjlq5fqtfsdsqrgd8unzuxl35zvgemamhu9kreu9kc7jduuq5ksdr2r4pd74ct39ytpgku6u6x5y5h8zszhrnuuqs6qz2uex7z3m9v7r5lc2lval8nhml0wrja0lp89yj4rv8gcm7xammz7rnuj7l0aap929c7rva7lmmmypk5jlrw4m5eqrmuap46szva32a7lm6fl0hwljauvrjzjj7r00hwljfuvn46vdr2x34pvjz37r3unj5ne8yshreuuxhzhphu4l46hlrvak4q3q4u40jkn9euveh2cxqhpy9me23r9neza870p67xug4qk34refmtufu2wg5wju7wlm4tjurvxlrvu8n8cfdd538tcm8z9nezar5tcm7z0etd834kkmgwny7zgt4y7ghgpp5f5a605yw4snw7vw44vdzyk9stc04t0993vqeau6f4c4qnvkmdc7ssp5hae8vd4p95e3shsu44ewsc7g0yy723htt3zgdd82dnn6agvj0ups9qyysgqcqypaqaxq8lllllllfppqsmsyt0w88nycxpxa4v8rd0yevu6e436vrzjqwjpdn2djrehkzaaydyzrk6kg5nhtsjrmdqalv4qvuvfhjg0uev9rund8d9uqxt5kadklnrf28wd4crjscqkhvztylj52y75en2z7jmsjs6pz72kujhcn46v0wvh6wcy7xsgzqkrr5m7gf35eps9pkuln0upsw6jkunmnv8ma3tqph5r8yyaj9vkgp0ymxnf","replacement":null,"amount":"1241156895152450.411","fees":"1782501629037774.722","status":"pending","status_str":"pending","note":null,"finalized_at":4218842088330302797}
{"index":"2686775637741482092-ln_850d5861376a06b28e0b055c3f427b05c2bbab5dae1216186fadb81ddcfc818a","kind":"invoice","direction":"outbound","invoice":"lnbc17pflccndtzdgnlpdu0ns3zyhx34qq0r84ysncfl9d59ref8t9x6x5qmuyljk6x0sahymg6s0e9yfrj50rq7wxttfzdync608vvu2q2u06c7wn6nxl3sx3eufhjh7nf5mc29mpt7zsmtnetmq5gw0ct0y4ta7lm6ll3hjxg0ud5j74zdm5g38cf8guuytc6rz4sp4sr6gn9tn3gptkz5heem89yfgnzgtfkeza0rgdm5qnqhmalhhe29qagfmcctgvaysxl89vynqnxpuuj32czuf388legfg49vdek4ay0nkwv3whnk7ytuler7z0etdqm7xpmmqmu7xv28003k62ewg348gny4uyljk69jjpu0ypp5wgwlk5ax33el897jx2d5kjkm2s85v4m4ml5s56vqh8r4llt4v6pqsp59v63n2p632n6w7ynf76kegjml9htrfx823zktcqq0vpsu494a2zs9qyysgqcqrtlmxq8lllllllylgzg9qrdwku74kzfyk9xr08nc35rnt75qsqjz3r09z5tqdn6y0szunqx8uh069n7q0mnx6des82tfkgakqsehglqzq2f0srmweh39qptpn7xg","replacement":null,"amount":"1946291676982854.935","fees":"1577512301561215.369","status":"completed","status_str":"recipient rejected our invoice request","note":"foo bar","finalized_at":null}
--- OutboundSpontaneous
{"index":"5088726529312448170-ln_2767ee2432d14da350e9ae60fb64184b125947540fb928fd9023b8a9d2a0923c","kind":"spontaneous","direction":"outbound","invoice":null,"replacement":null,"amount":"1707980374871680.224","fees":"279186991440371.32","status":"failed","status_str":"failed","note":"foo bar","finalized_at":3793002393196153970}
{"index":"3389316343333198151-ln_299cacbe6cccedfca30d054bd0220ce7822e759ac45e6697448020e2dd56f947","kind":"spontaneous","direction":"outbound","invoice":null,"replacement":null,"amount":"382669342508142.204","fees":"1550387309366020.838","status":"failed","status_str":"failed","note":null,"finalized_at":null}
{"index":"5155186382553476589-ln_2877475d06893a3c833caf5a0872253ca30372d5f79241fcf917d893fd0184f6","kind":"spontaneous","direction":"outbound","invoice":null,"replacement":null,"amount":"1775777429636972.896","fees":"686803029182910.189","status":"pending","status_str":"pending","note":null,"finalized_at":null}

--- v2 (1) add txid for OnchainSend, (2) don't serialize empty fields
--- OnchainSend
{"index":"2352250271958571112-os_0c83e7486ccf8b662e1a57eaa67b3a9cf73d312625ffaf155e27e19c6896330e","kind":"onchain","direction":"outbound","txid":"e911c836046ae0da0f8aef4549c7dd18da916932ef018ed7a9de6a74b42fef58","amount":"426295955721124.011","fees":"106243014155694.28","status":"pending","status_str":"partially confirmed (1-5 confirmations)","note":"foo bar","finalized_at":922031411621277542}
{"index":"6662034773613201778-os_160d1659b347941dcf067ef4b7053355b058640a86e4fc8f3865c82da9b5a6cb","kind":"onchain","direction":"outbound","txid":"8540d62a5926e3279dbd7471f5cf81deb8049b4a7d2b58af2626583218cc3761","replacement":"21767abd07cfa29e6f1415ae0ea4b447b0aa212a01ac974c9d680ab199d79a28","amount":"1283455089118142.425","fees":"1195902608470070.026","status":"pending","status_str":"broadcasted","finalized_at":7724772872675692842}
{"index":"7513172493835928492-os_189a8ddf5f0910096c5610dabce731a2a7d27100df1185f3d9c1944596c96199","kind":"onchain","direction":"outbound","txid":"b329dd242e2cd1716abc3ff827fde82e5ebc8f7e6db2f6585091f1f32a211f3b","amount":"2069602468658948.217","fees":"1347441650278795.035","status":"pending","status_str":"created","finalized_at":8681440472811127301}
--- OnchainReceive
{"index":"5806002936929143706-or_6e2a71f56aab3a33a2b0c2cfe8a092883f9783e5cb599b5d5c5b264d5c4e2669","kind":"onchain","direction":"inbound","txid":"6e2a71f56aab3a33a2b0c2cfe8a092883f9783e5cb599b5d5c5b264d5c4e2669","amount":"1026373441666394.174","fees":"0","status":"pending","status_str":"partially confirmed (1-5 confirmations)","finalized_at":3872673315358973283}
{"index":"8270747644397781506-or_805b79f828994ebb2e06b6b0a2f3fda1b52da1417335a35a826452c988be6e1c","kind":"onchain","direction":"inbound","txid":"805b79f828994ebb2e06b6b0a2f3fda1b52da1417335a35a826452c988be6e1c","replacement":"454737de4ff8795fb3b86b817a68701c90241190b425946cb3404232a10302f7","amount":"414234636362640.08","fees":"0","status":"failed","status_str":"dropped from mempool","finalized_at":6727882137524031451}
{"index":"1667254218639325659-or_92ca95723279675661dc51bcacbf4bf6d7d96ab13c1fd71fcc8fbbe08aa6f81d","kind":"onchain","direction":"inbound","txid":"92ca95723279675661dc51bcacbf4bf6d7d96ab13c1fd71fcc8fbbe08aa6f81d","amount":"2077078592479909.515","fees":"0","status":"pending","status_str":"being replaced (replacement has 1-5 confirmations)","note":"foo bar","finalized_at":7581178487696199344}
--- InboundInvoice
{"index":"4725737965850879943-ln_bc29d8f882abed198ce434cd50d01b5ca840fdaef3d239289c24b4b6efb5053f","kind":"invoice","direction":"inbound","invoice":"lntb18446744073709551610p1ptueg0qdr97xtfnwlwnxdjvznl8tc6f9dg8ghzfudcn7hjw0mq72jgrgtrcwgj4u9p56tzdmamhaxn4uu2jj3h7283s75f8u5unxn08y4f5u9yspp57z23mnrrsvpehpshk4k9g6y5su4nk2ka74lts8pyc4m43g6mkvtqsp5pfg2c0wuxft2ueaqpp2n83egyjrs22mlsjqtlkcqynqrzmm3jgxs9qyysgqcqr0r9np4qdfsmtw7dv7z66qw24t4zcsdyuy07tgrelewu3ll2qsdtjdh3mfuvfppq756g7n33q4s2kym4udpnh8g4u5pxd2zvr9yqth0u8lycwzxxr8pkszl4qz035cm7knx7naly983rr3pvjkm0rn0zr9cn8utuxju02d6jzq0nz5npjvurypjlwd9t04nwpnke7mky4ajekj2gd8exnydnumra06gm390he0vldj8zhnyuhdw3jf87phmckswkmx2284s5ush5h2f7r0pwlm8pwrmlevcgecqclkflq7c7wr3dqv65g5v8wuqwgg6uynpxvstvxr4ka7janu873kqc5r36jzd0gwv8r5jl97tsdgphh5a5l","amount":"1675262874117511.115","fees":"1986679708464435.976","status":"pending","status_str":"claiming","note":"foo bar"}
{"index":"2256768884000796422-ln_c34e891542810683ad1a0792f8eb783bbc8b090e469e3a91debe286d1b9e6383","kind":"invoice","direction":"inbound","invoice":"lntb1283l5kphp534m26n0hqdj7xu8cf2jjnguw46qe4485ssptyqnmq6w2e20vcvmspp57wxq77d89q6qvva3h30h9whd586uys3w8g6w5wnay9cqsc09ygvqsp5vyk5n3rppdxqfvgyzthx85ydy2pclw9e4nxxkm44khca7eehef8q9qyysgqcqrs7txq8lllllllfppjf45w7h9luhsx3m7x4zsckl9arc64yuus4xlxt9gzcnj7rmhgculj59zyh5lqa8serjpz6ltu0w9vgzyydal4h5685jn9n9m5e2je77z5uj9s9r8hkfcqhx7lftqh959p93lthpgp5xwsmd","amount":"1071637668292635.607","fees":"0","status":"pending","status_str":"claiming","finalized_at":4909338776287845535}
{"index":"0966462693570159466-ln_f690bc3827b3de355373f3259367875a05c3dd502d726398a217989ba3e9e337","kind":"invoice","direction":"inbound","invoice":"lnbc1pw83lxzds5ts93kvmm7xgm8rf29mhml0gtp9sqpevyhujn66kghg5wlwaluz3m7f8sn72mfu9hj6v0rw5l3sxu99hjs7neyaqf7xrgmr05s6d2cf83n6ct8ay23kn0r99ch3wdr2pl7xetltwrs3au98f6vmp2tmalh5h9eeu8kmhml02ua7amluyljk6qh5dg6x5z7t207z0640hsj2k6fuucj73z2tl3szdejwnlvgdkvqp87w72fdkzjlcm9pdta7lmmmalh5lkts49yghh7q8zszhrej96yp7u8z7r30hmh0e685syls4etuuzfay95knz4malh5qxqrf27xn68yn7a7lm620j46h2tuumjxlzk7cdxsjv9ftq7w063d2fqq3z2h32w0eghzv072z2pw7r5nhml00tnw508yvns57s5uh3n74mw5327wugmd8s5z6t0j96yle2fzvdynhmh0m8gyyjfs4y7z0etdqq7zkg4zmmycdl80akntcfl9d5ycjzstey9g3nw52vp53za5dgc2yqpp5k6zathnul34ttcl5y5nq8z2rv4g8mv5ec6lfnqcxu02x4kn6sk5qsp5wkj3qnuxwsj59j26wnydu0yng77jz94dfat3zcway24dgcg4vngq9qyysgqcqrlz4fppj6pnpnz5fgvk9v8v5g0l6rqpjwywwzhhwrzjq2ju7ggyarj7f3y8yc6qpv4h3a67fnvmwy9d2w96w0y2jt9njhwg28fan3f82dfdm3kzahez2fxkdsjpycw40zvc5trq972gyhsl44plx3v66t44hhk6c2f28vzkqzqft3r9dxvvtkngl88kdljdtssmzetzrj454qy67txsffe87hhzyjt8yddyqp5hte3p","amount":"563859865662737.147","fees":"1360802405875876.968","status":"failed","status_str":"invoice expired","note":"foo bar"}
--- InboundSpontaneous
{"index":"0263933984505265604-ln_2000815f864791a5d9b10e0e72f75d09096ee4800d52d3bb90e8649135e3815a","kind":"spontaneous","direction":"inbound","amount":"913120818704807.97","fees":"0","status":"completed","status_str":"completed","finalized_at":3136200261222996439}
{"index":"0559588758736175637-ln_8b416294218b1042148a4a2715e501aad76350d8616cef3b54ca3f7de53e310e","kind":"spontaneous","direction":"inbound","amount":"658204045720138.099","fees":"1437970496572162.885","status":"pending","status_str":"claiming","note":"foo bar"}
{"index":"8251254704841858744-ln_1bd44f2cacfac849ef1657c047f890be52fd675985c9cab82038665380594863","kind":"spontaneous","direction":"inbound","amount":"534425667243574.803","fees":"0","status":"completed","status_str":"completed","finalized_at":5744690233519079911}
--- OutboundInvoice
{"index":"3967145603839053663-ln_c28ce106b0ec5393371dde088a91465db79137c242b993f39cbd93463d62303a","kind":"invoice","direction":"outbound","invoice":"lnbc16617464075412908110p1du587g2hp5j25fhdvhz66tctmq6xga76mdddz62xm7fk6n7alekxd4d8y2rqyspp5axv2wz5w2upckqf0exq3wwg09kkha9zushwm6tzmqsenkgafeyeqsp5usuarqswxgkpk6skvydnyaw56xpv400xy0auh8zjg3r4t9mqp8fs9qyysgqcqypvxfx04nrfq3khmpat4e9k5a2d52eup290rkts344t7z942zzhmk70h9ywtwu6kqc8hpx3af6tjakute4xq29h3rhdq50vcecjrxredxh0gqlm8nvn","amount":"1388110017620496.458","fees":"1989134516334956.22","status":"pending","status_str":"unknown error, app is likely out-of-date"}
{"index":"1506781100894234982-ln_723bea0a83ee7d8f4747cabd15292d1eaaaff42fee95479d7aca41cf6f68a954","kind":"invoice","direction":"outbound","invoice":"lnbc1mmj7z2hd427xtea2gtw8et4p5ta7lm6xe02nemhxvg7zse98734qudr2pucwaz3ua647tl9tv8nsnv3whzszhv89lnhwum9u4vk284sfr6c2jnee9yhcn08qd65ghznuac4w5l9fahknyt4sugg0p22tqqdsxlrf465fwzvupw7z7tm9p97wyerp8j5676zafx8s9rlj967x4cld6dty3w9q9w7zvm8wl3kkacj4l3hkzcauutj630rx5pnp4l805v50g63ayd5w7lr8yrsvqxq3ljh6mf0uuxsjlq5fqtfsdsqrgd8unzuxl35zvgemamhu9kreu9kc7jduuq5ksdr2r4pd74ct39ytpgku6u6x5y5h8zszhrnuuqs6qz2uex7z3m9v7r5lc2lval8nhml0wrja0lp89yj4rv8gcm7xammz7rnuj7l0aap929c7rva7lmmmypk5jlrw4m5eqrmuap46szva32a7lm6fl0hwljauvrjzjj7r00hwljfuvn46vdr2x34pvjz37r3unj5ne8yshreuuxhzhphu4l46hlrvak4q3q4u40jkn9euveh2cxqhpy9me23r9neza870p67xug4qk34refmtufu2wg5wju7wlm4tjurvxlrvu8n8cfdd538tcm8z9nezar5tcm7z0etd834kkmgwny7zgt4y7ghgpp5f5a605yw4snw7vw44vdzyk9stc04t0993vqeau6f4c4qnvkmdc7ssp5hae8vd4p95e3shsu44ewsc7g0yy723htt3zgdd82dnn6agvj0ups9qyysgqcqypaqaxq8lllllllfppqsmsyt0w88nycxpxa4v8rd0yevu6e436vrzjqwjpdn2djrehkzaaydyzrk6kg5nhtsjrmdqalv4qvuvfhjg0uev9rund8d9uqxt5kadklnrf28wd4crjscqkhvztylj52y75en2z7jmsjs6pz72kujhcn46v0wvh6wcy7xsgzqkrr5m7gf35eps9pkuln0upsw6jkunmnv8ma3tqph5r8yyaj9vkgp0ymxnf","amount":"1241156895152450.411","fees":"1782501629037774.722","status":"pending","status_str":"pending","finalized_at":4218842088330302797}
{"index":"2686775637741482092-ln_850d5861376a06b28e0b055c3f427b05c2bbab5dae1216186fadb81ddcfc818a","kind":"invoice","direction":"outbound","invoice":"lnbc17pflccndtzdgnlpdu0ns3zyhx34qq0r84ysncfl9d59ref8t9x6x5qmuyljk6x0sahymg6s0e9yfrj50rq7wxttfzdync608vvu2q2u06c7wn6nxl3sx3eufhjh7nf5mc29mpt7zsmtnetmq5gw0ct0y4ta7lm6ll3hjxg0ud5j74zdm5g38cf8guuytc6rz4sp4sr6gn9tn3gptkz5heem89yfgnzgtfkeza0rgdm5qnqhmalhhe29qagfmcctgvaysxl89vynqnxpuuj32czuf388legfg49vdek4ay0nkwv3whnk7ytuler7z0etdqm7xpmmqmu7xv28003k62ewg348gny4uyljk69jjpu0ypp5wgwlk5ax33el897jx2d5kjkm2s85v4m4ml5s56vqh8r4llt4v6pqsp59v63n2p632n6w7ynf76kegjml9htrfx823zktcqq0vpsu494a2zs9qyysgqcqrtlmxq8lllllllylgzg9qrdwku74kzfyk9xr08nc35rnt75qsqjz3r09z5tqdn6y0szunqx8uh069n7q0mnx6des82tfkgakqsehglqzq2f0srmweh39qptpn7xg","amount":"1946291676982854.935","fees":"1577512301561215.369","status":"completed","status_str":"recipient rejected our invoice request","note":"foo bar"}
--- OutboundSpontaneous
{"index":"5088726529312448170-ln_2767ee2432d14da350e9ae60fb64184b125947540fb928fd9023b8a9d2a0923c","kind":"spontaneous","direction":"outbound","amount":"1707980374871680.224","fees":"279186991440371.32","status":"failed","status_str":"failed","note":"foo bar","finalized_at":3793002393196153970}
{"index":"3389316343333198151-ln_299cacbe6cccedfca30d054bd0220ce7822e759ac45e6697448020e2dd56f947","kind":"spontaneous","direction":"outbound","amount":"382669342508142.204","fees":"1550387309366020.838","status":"failed","status_str":"failed"}
{"index":"5155186382553476589-ln_2877475d06893a3c833caf5a0872253ca30372d5f79241fcf917d893fd0184f6","kind":"spontaneous","direction":"outbound","amount":"1775777429636972.896","fees":"686803029182910.189","status":"pending","status_str":"pending"}
"#;

        for input in snapshot::parse_sample_data(inputs) {
            let value1: BasicPayment = serde_json::from_str(input).unwrap();
            let output = serde_json::to_string(&value1).unwrap();
            let value2: BasicPayment = serde_json::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }
}
