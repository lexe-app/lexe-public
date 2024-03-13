use std::{
    cmp::Ordering,
    fmt::{self, Display},
    ops::Deref,
    str::FromStr,
};

use anyhow::{anyhow, bail, ensure, Context};
use bitcoin_hashes::{sha256, Hash};
use lightning::ln::{
    channelmanager::PaymentId, PaymentHash, PaymentPreimage, PaymentSecret,
};
use lightning_invoice::Bolt11InvoiceDescription;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    hex::{self, FromHex},
    hexstr_or_bytes,
    ln::{amount::Amount, hashes::LxTxid, invoice::LxInvoice},
    rng::RngCore,
    time::TimestampMs,
};

// --- Top-level payment types --- //

/// A basic payment type which contains all of the user-facing payment details
/// for any kind of payment. These details are exposed in the Lexe app.
///
/// It is essentially the `Payment` type flattened out such that each field is
/// the result of the corresponding `Payment` getter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(all(any(test, feature = "test-utils")), derive(Arbitrary))]
pub struct BasicPayment {
    pub index: PaymentIndex,

    pub kind: PaymentKind,
    pub direction: PaymentDirection,

    /// (Invoice payments only) The BOLT11 invoice used in this payment.
    pub invoice: Option<LxInvoice>,

    /// (Onchain payments only) The txid of the replacement tx, if one exists.
    pub replacement: Option<LxTxid>,

    /// The amount of this payment.
    ///
    /// - If this is a completed inbound invoice payment, this is the amount we
    ///   received.
    /// - If this is a pending or failed inbound inbound invoice payment, this
    ///   is the amount encoded in our invoice, which may be null.
    /// - For all other payment types, an amount is always included.
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
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    /// The payment status as a human-readable string. These strings are
    /// customized per payment type, e.g. "invoice generated", "timed out"
    pub status_str: String,

    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_option_string()")
    )]
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
    pub note: Option<String>,

    pub finalized_at: Option<TimestampMs>,
}

/// An encrypted payment, as represented in the DB.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DbPayment {
    pub created_at: i64,
    pub id: String,
    pub status: String,
    pub data: Vec<u8>,
}

/// Specifies whether this is an onchain payment, LN invoice payment, etc.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum PaymentKind {
    Onchain,
    Invoice,
    Spontaneous,
}

/// Specifies whether a payment is inbound or outbound.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
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
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
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

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
///
/// Its primary purpose is to prevent accidental double payments. Internal
/// structure (if any) is opaque to the node.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct ClientPaymentId(#[serde(with = "hexstr_or_bytes")] pub [u8; 32]);

/// Newtype for [`PaymentHash`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxPaymentHash(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentPreimage`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxPaymentPreimage(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentSecret`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LxPaymentSecret(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

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

    /// Returns the user's note or invoice description, prefering note over
    /// description.
    pub fn note_or_description(&self) -> Option<&str> {
        let maybe_note = self.note.as_deref().filter(|s| !s.is_empty());

        maybe_note.or_else(|| {
            self.invoice.as_ref().and_then(|invoice| {
                match invoice.0.description() {
                    Bolt11InvoiceDescription::Direct(description)
                        if !description.is_empty() =>
                        Some(description.deref()),
                    // Hash description is not useful yet
                    _ => None,
                }
            })
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
        let mut random_buf = [0u8; 32];
        rng.fill_bytes(&mut random_buf);
        Self(random_buf)
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

// --- Redact secret information --- //

impl fmt::Debug for LxPaymentPreimage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LxPaymentPreimage(..)")
    }
}

impl fmt::Debug for LxPaymentSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LxPaymentSecret(..)")
    }
}

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
        Self(hash.into_inner())
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

// As recommended by LDK, we use LxPaymentHash as our PaymentId
impl From<PaymentId> for LxPaymentHash {
    fn from(id: PaymentId) -> Self {
        Self(id.0)
    }
}
impl From<LxPaymentHash> for PaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self(hash.0)
    }
}

// --- FromStr / Display for the simple enums --- //

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
        match self {
            Self::Onchain => write!(f, "onchain"),
            Self::Invoice => write!(f, "invoice"),
            Self::Spontaneous => write!(f, "spontaneous"),
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
        match self {
            Self::Inbound => write!(f, "inbound"),
            Self::Outbound => write!(f, "outbound"),
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
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
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

// --- Newtype FromStr / Display impls -- //

impl FromStr for ClientPaymentId {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self)
    }
}
impl FromStr for LxPaymentHash {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self)
    }
}
impl FromStr for LxPaymentPreimage {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self)
    }
}
impl FromStr for LxPaymentSecret {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self)
    }
}

impl Display for ClientPaymentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_display = hex::display(&self.0);
        write!(f, "{hex_display}")
    }
}
impl Display for LxPaymentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_display = hex::display(&self.0);
        write!(f, "{hex_display}")
    }
}
impl Display for LxPaymentPreimage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_display = hex::display(&self.0);
        write!(f, "{hex_display}")
    }
}
impl Display for LxPaymentSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_display = hex::display(&self.0);
        write!(f, "{hex_display}")
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
    use crate::test_utils::roundtrip;

    #[test]
    fn enums_roundtrips() {
        roundtrip::json_string_roundtrip_proptest::<PaymentDirection>();
        roundtrip::json_string_roundtrip_proptest::<PaymentStatus>();
        roundtrip::json_string_roundtrip_proptest::<PaymentKind>();
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
    }

    #[test]
    fn newtype_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<PaymentIndex>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentId>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentPreimage>();
        roundtrip::fromstr_display_roundtrip_proptest::<LxPaymentSecret>();
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
}
