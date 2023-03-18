//! Payment types.
//!
//! The full hierarchy of payments types is as follows:
//!
//! [`Payment`]
//! |
//! |___[`OnchainPayment`]
//! |   |
//! |   |___[`OnchainDeposit`]
//! |   |
//! |   |___[`OnchainWithdrawal`]
//! |   |
//! |   |___`SpliceIn` TODO(max): Implement
//! |   |
//! |   |___`SpliceOut` TODO(max): Implement
//! |
//! |___[`LightningPayment`]
//!     |
//!     |___[`InboundInvoicePayment`]
//!     |
//!     |___[`InboundSpontaneousPayment`]
//!     |
//!     |___[`OutboundInvoicePayment`]
//!     |
//!     |___[`OutboundSpontaneousPayment`]
//!
//! NOTE: Everything in this hierarchy impls [`Serialize`] and [`Deserialize`],
//! so be mindful of backwards compatibility.
//!
//! [`Payment`]: payments::Payment
//! [`OnchainPayment`]: payments::onchain::OnchainPayment
//! [`LightningPayment`]: payments::offchain::LightningPayment
//! [`OnchainDeposit`]: payments::onchain::OnchainDeposit
//! [`OnchainWithdrawal`]: payments::onchain::OnchainWithdrawal
//! [`InboundInvoicePayment`]: payments::offchain::inbound::InboundInvoicePayment
//! [`InboundSpontaneousPayment`]: payments::offchain::inbound::InboundSpontaneousPayment
//! [`OutboundInvoicePayment`]: payments::offchain::outbound::OutboundInvoicePayment
//! [`OutboundSpontaneousPayment`]: payments::offchain::outbound::OutboundSpontaneousPayment
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize

use std::convert::TryFrom;
use std::fmt::{self, Display};
use std::str::FromStr;

use anyhow::{bail, ensure, Context};
use bitcoin::Txid;
use common::hex::{self, FromHex};
use common::hexstr_or_bytes;
use common::time::TimestampMillis;
use lightning::ln::channelmanager::{PaymentId, PaymentSendFailure};
use lightning::ln::{PaymentHash, PaymentPreimage, PaymentSecret};
use lightning_invoice::payment::PaymentError;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::payments::offchain::inbound::{
    InboundInvoicePayment, InboundSpontaneousPayment,
};
use crate::payments::offchain::outbound::{
    OutboundInvoicePayment, OutboundSpontaneousPayment,
};
use crate::payments::offchain::LightningPayment;
use crate::payments::onchain::{
    OnchainDeposit, OnchainPayment, OnchainWithdrawal,
};

/// `PaymentsManager`.
pub mod manager;
/// Lightning payment types and state machines.
pub mod offchain;
/// On-chain payment types and state machines.
pub mod onchain;

// --- Top-level payment types --- //

/// The top level [`Payment`] type which abstracts over all types of payments,
/// including both onchain and off-chain (Lightning) payments.
#[derive(Clone, Serialize, Deserialize)]
pub enum Payment {
    Onchain(OnchainPayment),
    Lightning(LightningPayment),
}

/// Specifies whether a payment is inbound or outbound.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
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
pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

// --- Lexe newtypes --- //

/// A globally-unique identifier for any type of payment, including both
/// on-chain and Lightning payments.
///
/// - On-chain payments use their [`Txid`] as their id.
/// - Lightning payments use their [`LxPaymentHash`] as their id.
///
/// NOTE that this is NOT a drop-in replacement for LDK's [`PaymentId`], since
/// [`PaymentId`] is Lightning-specific, whereas [`LxPaymentId`] is not.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
pub enum LxPaymentId {
    Onchain(Txid),
    Lightning(LxPaymentHash),
}

/// Newtype for [`PaymentHash`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentHash(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentPreimage`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentPreimage(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

/// Newtype for [`PaymentSecret`] which impls [`Serialize`] / [`Deserialize`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LxPaymentSecret(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

// --- PaymentTrait --- //

/// A trait for common payment methods.
pub(crate) trait PaymentTrait {
    /// Whether this payment is inbound or outbound. Useful for filtering.
    fn direction(&self) -> PaymentDirection;

    /// The amount of this payment in millisatoshis.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn amt_msat(&self) -> Option<u64>;

    /// The fees paid or expected to be paid for this payment.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn fees_msat(&self) -> u64;

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    fn status(&self) -> PaymentStatus;

    /// Get the payment status as a human-readable `&'static str`
    fn status_str(&self) -> &str;

    /// When this payment was created.
    fn created_at(&self) -> TimestampMillis;

    /// When this payment was completed or failed.
    fn finalized_at(&self) -> Option<TimestampMillis>;
}

// --- impl Payment --- //

impl Payment {
    pub fn id(&self) -> LxPaymentId {
        match self {
            Self::Onchain(onchain) => LxPaymentId::Onchain(*onchain.txid()),
            Self::Lightning(ln) => LxPaymentId::Lightning(*ln.hash()),
        }
    }
}

impl PaymentTrait for Payment {
    /// Whether this payment is inbound or outbound. Useful for filtering.
    fn direction(&self) -> PaymentDirection {
        match self {
            Self::Onchain(onchain) => onchain.direction(),
            Self::Lightning(lightning) => lightning.direction(),
        }
    }

    /// The amount of this payment in millisatoshis.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn amt_msat(&self) -> Option<u64> {
        match self {
            Self::Onchain(onchain) => onchain.amt_msat(),
            Self::Lightning(lightning) => lightning.amt_msat(),
        }
    }

    /// The fees paid or expected to be paid for this payment.
    // TODO(max): Use LDK-provided Amount newtype when available
    fn fees_msat(&self) -> u64 {
        match self {
            Self::Onchain(onchain) => onchain.fees_msat(),
            Self::Lightning(lightning) => lightning.fees_msat(),
        }
    }

    /// Get a general [`PaymentStatus`] for this payment. Useful for filtering.
    fn status(&self) -> PaymentStatus {
        match self {
            Self::Onchain(onchain) => onchain.status(),
            Self::Lightning(lightning) => lightning.status(),
        }
    }

    /// Get the payment status as a human-readable `&'static str`
    fn status_str(&self) -> &str {
        match self {
            Self::Onchain(onchain) => onchain.status_str(),
            Self::Lightning(lightning) => lightning.status_str(),
        }
    }

    /// When this payment was created.
    fn created_at(&self) -> TimestampMillis {
        match self {
            Self::Onchain(onchain) => onchain.created_at(),
            Self::Lightning(lightning) => lightning.created_at(),
        }
    }

    /// When this payment was completed or failed.
    fn finalized_at(&self) -> Option<TimestampMillis> {
        match self {
            Self::Onchain(onchain) => onchain.finalized_at(),
            Self::Lightning(lightning) => lightning.finalized_at(),
        }
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

// LxPaymentId -> Txid / LxPaymentHash
impl From<Txid> for LxPaymentId {
    fn from(txid: Txid) -> Self {
        Self::Onchain(txid)
    }
}
impl From<LxPaymentHash> for LxPaymentId {
    fn from(hash: LxPaymentHash) -> Self {
        Self::Lightning(hash)
    }
}

// LxPaymentId -> Txid / LxPaymentHash
impl TryFrom<LxPaymentId> for Txid {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        match id {
            LxPaymentId::Onchain(txid) => Ok(txid),
            LxPaymentId::Lightning(..) => bail!("Not an onchain payment"),
        }
    }
}
impl TryFrom<LxPaymentId> for LxPaymentHash {
    type Error = anyhow::Error;
    fn try_from(id: LxPaymentId) -> anyhow::Result<Self> {
        match id {
            LxPaymentId::Onchain(..) => bail!("Not a lightning payment"),
            LxPaymentId::Lightning(hash) => Ok(hash),
        }
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

// --- LxPaymentId FromStr / Display impls --- //

/// `<kind>_<id>`
impl FromStr for LxPaymentId {
    type Err = anyhow::Error;
    fn from_str(kind_id: &str) -> anyhow::Result<Self> {
        let mut parts = kind_id.split('_');
        let kind_str = parts.next().context("Missing kind in <kind>_<id>")?;
        let id_str = parts.next().context("Missing kind in <kind>_<id>")?;
        ensure!(
            parts.next().is_none(),
            "Wrong format; should be <kind>_<id>"
        );
        match kind_str {
            "onchain" => Txid::from_str(id_str)
                .map(Self::Onchain)
                .context("Invalid Txid"),
            "lightning" => LxPaymentHash::from_str(id_str)
                .map(Self::Lightning)
                .context("Invalid payment hash"),
            _ => bail!("<kind> should be 'onchain' or 'lightning'"),
        }
    }
}

/// `<kind>_<id>`
impl Display for LxPaymentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Onchain(txid) => write!(f, "onchain_{txid}"),
            Self::Lightning(hash) => write!(f, "lightning_{hash}"),
        }
    }
}

// --- Newtype FromStr / Display impls -- //

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

// --- Specific payment type -> top-level Payment type --- //

impl From<OnchainDeposit> for Payment {
    fn from(p: OnchainDeposit) -> Self {
        Self::Onchain(OnchainPayment::Inbound(p))
    }
}
impl From<OnchainWithdrawal> for Payment {
    fn from(p: OnchainWithdrawal) -> Self {
        Self::Onchain(OnchainPayment::Outbound(p))
    }
}
impl From<InboundInvoicePayment> for Payment {
    fn from(p: InboundInvoicePayment) -> Self {
        Self::Lightning(LightningPayment::InboundInvoice(p))
    }
}
impl From<InboundSpontaneousPayment> for Payment {
    fn from(p: InboundSpontaneousPayment) -> Self {
        Self::Lightning(LightningPayment::InboundSpontaneous(p))
    }
}
impl From<OutboundInvoicePayment> for Payment {
    fn from(p: OutboundInvoicePayment) -> Self {
        Self::Lightning(LightningPayment::OutboundInvoice(p))
    }
}
impl From<OutboundSpontaneousPayment> for Payment {
    fn from(p: OutboundSpontaneousPayment) -> Self {
        Self::Lightning(LightningPayment::OutboundSpontaneous(p))
    }
}

// --- Types inherited from ldk-sample --- //
// TODO(max): Gradually remove / replace these with our own

pub struct PaymentInfo {
    pub preimage: Option<PaymentPreimage>,
    pub secret: Option<PaymentSecret>,
    pub status: HTLCStatus,
    pub amt_msat: Option<u64>,
}

#[allow(dead_code)]
pub enum HTLCStatus {
    Pending,
    Succeeded,
    Failed,
}

impl Display for HTLCStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Succeeded => write!(f, "succeeded"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A newtype for [`PaymentError`] that impls [`Display`] and [`Error`].
///
/// [`Error`]: std::error::Error
#[derive(Debug, thiserror::Error)]
pub enum LxPaymentError {
    #[error("Invalid invoice: {0}")]
    Invoice(&'static str),
    #[error("Payment send failure: {0:?}")]
    Sending(Box<PaymentSendFailure>),
}

impl From<PaymentError> for LxPaymentError {
    fn from(ldk_err: PaymentError) -> Self {
        match ldk_err {
            PaymentError::Invoice(inner) => Self::Invoice(inner),
            PaymentError::Sending(inner) => Self::Sending(Box::new(inner)),
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::{arbitrary, roundtrip};
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::prop_oneof;
    use proptest::strategy::{BoxedStrategy, Strategy};
    use proptest::test_runner::Config;

    use super::*;

    impl Arbitrary for LxPaymentId {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            prop_oneof![
                arbitrary::any_txid().prop_map(Self::Onchain),
                any::<LxPaymentHash>().prop_map(Self::Lightning),
            ]
            .boxed()
        }
    }
    impl Arbitrary for LxPaymentHash {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>().prop_map(Self).boxed()
        }
    }
    impl Arbitrary for LxPaymentPreimage {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>().prop_map(Self).boxed()
        }
    }
    impl Arbitrary for LxPaymentSecret {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>().prop_map(Self).boxed()
        }
    }

    #[test]
    fn newtype_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxPaymentId>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentPreimage>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentSecret>();
    }

    #[test]
    fn newtype_fromstr_display_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxPaymentId>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentHash>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentPreimage>();
        roundtrip::json_string_roundtrip_proptest::<LxPaymentSecret>();
        // LxPaymentId's impls rely on Txid's FromStr/Display impls
        roundtrip::fromstr_display_custom(
            arbitrary::any_txid(),
            Config::default(),
        );
    }
}
