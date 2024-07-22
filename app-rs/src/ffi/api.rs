//! API request and response types exposed to Dart.

use std::str::FromStr;

use anyhow::{anyhow, Context};
use common::{
    api::{
        command::{
            CreateInvoiceRequest as CreateInvoiceRequestRs,
            CreateInvoiceResponse as CreateInvoiceResponseRs,
            FeeEstimate as FeeEstimateRs, NodeInfo as NodeInfoRs,
            PayInvoiceRequest as PayInvoiceRequestRs,
            PayInvoiceResponse as PayInvoiceResponseRs,
            PayOnchainRequest as PayOnchainRequestRs,
            PayOnchainResponse as PayOnchainResponseRs,
            PreflightPayInvoiceRequest as PreflightPayInvoiceRequestRs,
            PreflightPayInvoiceResponse as PreflightPayInvoiceResponseRs,
            PreflightPayOnchainRequest as PreflightPayOnchainRequestRs,
            PreflightPayOnchainResponse as PreflightPayOnchainResponseRs,
        },
        fiat_rates::FiatRates as FiatRatesRs,
        qs::UpdatePaymentNote as UpdatePaymentNoteRs,
    },
    ln::{
        amount::Amount,
        invoice::LxInvoice,
        payments::{
            ClientPaymentId as ClientPaymentIdRs, LxPaymentId as LxPaymentIdRs,
            PaymentIndex as PaymentIndexRs,
        },
    },
};
use flutter_rust_bridge::frb;

use crate::ffi::types::{
    ClientPaymentId, ConfirmationPriority, Invoice, PaymentIndex,
};

#[frb(dart_metadata=("freezed"))]
pub struct NodeInfo {
    pub node_pk: String,
    pub version: String,
    pub measurement: String,
    pub balance: Balance,
}

impl From<NodeInfoRs> for NodeInfo {
    fn from(info: NodeInfoRs) -> Self {
        let balance = Balance::from(&info);
        Self {
            node_pk: info.node_pk.to_string(),
            version: info.version.to_string(),
            measurement: info.measurement.to_string(),
            balance,
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct Balance {
    /// The top-level balance we'll show on the user screen.
    pub total_sats: u64,
    /// The amount we can currently spend from our outbound LN channel
    /// capacity.
    pub lightning_sats: u64,
    /// The amount of spendable onchain funds, i.e., those that are confirmed
    /// or otherwise trusted but maybe pending (self-generated UTXOs).
    pub onchain_sats: u64,
}

impl From<&NodeInfoRs> for Balance {
    fn from(info: &NodeInfoRs) -> Self {
        let lightning_sats = info.lightning_balance.sats_u64();
        let onchain_sats = info.onchain_balance.get_spendable_sats();
        let total_sats = lightning_sats + onchain_sats;

        Self {
            total_sats,
            lightning_sats,
            onchain_sats,
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct FiatRates {
    pub timestamp_ms: i64,
    // Sadly, the bridge doesn't currently support maps or tuples so... we'll
    // settle for a list...
    pub rates: Vec<FiatRate>,
}

#[frb(dart_metadata=("freezed"))]
pub struct FiatRate {
    pub fiat: String,
    pub rate: f64,
}

impl From<FiatRatesRs> for FiatRates {
    fn from(value: FiatRatesRs) -> Self {
        Self {
            timestamp_ms: value.timestamp_ms.as_i64(),
            rates: value
                .rates
                .into_iter()
                .map(|(fiat, rate)| FiatRate {
                    fiat: fiat.as_str().to_owned(),
                    rate: rate.0,
                })
                .collect(),
        }
    }
}

/// The maximum allowed payment note size in bytes.
///
/// See [`common::constants::MAX_PAYMENT_NOTE_BYTES`].
pub const MAX_PAYMENT_NOTE_BYTES: usize = 512;
// Assert that these two constants are exactly equal at compile time.
const _: [(); MAX_PAYMENT_NOTE_BYTES] =
    [(); common::constants::MAX_PAYMENT_NOTE_BYTES];

fn validate_note(note: String) -> anyhow::Result<String> {
    if note.len() <= MAX_PAYMENT_NOTE_BYTES {
        Ok(note)
    } else {
        Err(anyhow!("The payment note is too long."))
    }
}

/// See [`common::api::command::PayOnchainRequest`].
#[frb(dart_metadata=("freezed"))]
pub struct PayOnchainRequest {
    pub cid: ClientPaymentId,
    pub address: String,
    pub amount_sats: u64,
    pub priority: ConfirmationPriority,
    pub note: Option<String>,
}

impl TryFrom<PayOnchainRequest> for PayOnchainRequestRs {
    type Error = anyhow::Error;

    fn try_from(req: PayOnchainRequest) -> anyhow::Result<Self> {
        let address = bitcoin::Address::from_str(&req.address)
            .map_err(|_| anyhow!("The bitcoin address isn't valid."))?;
        let amount = Amount::try_from_sats_u64(req.amount_sats)?;

        Ok(Self {
            cid: req.cid.into(),
            address,
            amount,
            priority: req.priority.into(),
            note: req.note.map(validate_note).transpose()?,
        })
    }
}

/// See [`common::api::command::PayOnchainResponse`].
#[frb(dart_metadata=("freezed"))]
pub struct PayOnchainResponse {
    pub index: PaymentIndex,
    pub txid: String,
}

impl PayOnchainResponse {
    pub(crate) fn from_cid_and_response(
        cid: ClientPaymentIdRs,
        resp: PayOnchainResponseRs,
    ) -> Self {
        let index = PaymentIndexRs {
            created_at: resp.created_at,
            id: LxPaymentIdRs::OnchainSend(cid),
        };
        Self {
            index: PaymentIndex::from(index),
            txid: resp.txid.to_string(),
        }
    }
}

/// See [`common::api::command::PreflightPayOnchainRequest`].
#[frb(dart_metadata=("freezed"))]
pub struct PreflightPayOnchainRequest {
    pub address: String,
    pub amount_sats: u64,
}

impl TryFrom<PreflightPayOnchainRequest> for PreflightPayOnchainRequestRs {
    type Error = anyhow::Error;

    fn try_from(req: PreflightPayOnchainRequest) -> anyhow::Result<Self> {
        let address = bitcoin::Address::from_str(&req.address)
            .map_err(|_| anyhow!("The bitcoin address isn't valid."))?;
        let amount = Amount::try_from_sats_u64(req.amount_sats)?;

        Ok(Self { address, amount })
    }
}

/// See [`common::api::command::PreflightPayOnchainResponse`].
#[frb(dart_metadata=("freezed"))]
pub struct PreflightPayOnchainResponse {
    pub high: Option<FeeEstimate>,
    pub normal: FeeEstimate,
    pub background: FeeEstimate,
}

impl From<PreflightPayOnchainResponseRs> for PreflightPayOnchainResponse {
    fn from(resp: PreflightPayOnchainResponseRs) -> Self {
        Self {
            high: resp.high.map(FeeEstimate::from),
            normal: FeeEstimate::from(resp.normal),
            background: FeeEstimate::from(resp.background),
        }
    }
}

/// See [`common::api::command::FeeEstimate`].
#[frb(dart_metadata=("freezed"))]
pub struct FeeEstimate {
    pub amount_sats: u64,
}

impl From<FeeEstimateRs> for FeeEstimate {
    fn from(value: FeeEstimateRs) -> Self {
        Self {
            amount_sats: value.amount.sats_u64(),
        }
    }
}

/// See [`common::api::command::CreateInvoiceRequest`].
#[frb(dart_metadata=("freezed"))]
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,
    pub amount_sats: Option<u64>,
    pub description: Option<String>,
}

impl TryFrom<CreateInvoiceRequest> for CreateInvoiceRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: CreateInvoiceRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            expiry_secs: value.expiry_secs,
            amount: value
                .amount_sats
                .map(Amount::try_from_sats_u64)
                .transpose()?,
            description: value.description,
        })
    }
}

/// See [`common::api::command::CreateInvoiceResponse`].
#[frb(dart_metadata=("freezed"))]
pub struct CreateInvoiceResponse {
    pub invoice: Invoice,
}

impl From<CreateInvoiceResponseRs> for CreateInvoiceResponse {
    fn from(value: CreateInvoiceResponseRs) -> Self {
        Self {
            invoice: Invoice::from(&value.invoice),
        }
    }
}

/// Mirrors the [`common::api::command::PayInvoiceRequest`] type.
#[frb(dart_metadata=("freezed"))]
pub struct PayInvoiceRequest {
    pub invoice: String,
    pub fallback_amount_sats: Option<u64>,
    pub note: Option<String>,
}

impl TryFrom<PayInvoiceRequest> for PayInvoiceRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: PayInvoiceRequest) -> Result<Self, Self::Error> {
        let invoice = LxInvoice::from_str(&value.invoice)
            .context("Failed to parse invoice")?;

        let fallback_amount = match value.fallback_amount_sats {
            Some(amount) => {
                debug_assert!(invoice.amount().is_none());
                Some(Amount::try_from_sats_u64(amount)?)
            }
            None => {
                debug_assert!(invoice.amount().is_some());
                None
            }
        };

        Ok(Self {
            invoice,
            fallback_amount,
            note: value.note,
        })
    }
}

/// Mirrors [`common::api::command::PayInvoiceResponse`] the type, but enriches
/// the response so we get the full `PaymentIndex`.
#[frb(dart_metadata=("freezed"))]
pub struct PayInvoiceResponse {
    pub index: PaymentIndex,
}

impl PayInvoiceResponse {
    pub(crate) fn from_id_and_response(
        id: LxPaymentIdRs,
        resp: PayInvoiceResponseRs,
    ) -> Self {
        let index = PaymentIndexRs {
            created_at: resp.created_at,
            id,
        };
        Self {
            index: PaymentIndex::from(index),
        }
    }
}

/// See [`common::api::command::PreflightPayInvoiceRequest`].
#[frb(dart_metadata=("freezed"))]
pub struct PreflightPayInvoiceRequest {
    pub invoice: String,
    pub fallback_amount_sats: Option<u64>,
}

impl TryFrom<PreflightPayInvoiceRequest> for PreflightPayInvoiceRequestRs {
    type Error = anyhow::Error;
    fn try_from(
        value: PreflightPayInvoiceRequest,
    ) -> Result<Self, Self::Error> {
        let invoice = LxInvoice::from_str(&value.invoice)
            .context("Failed to parse invoice")?;

        let fallback_amount = match value.fallback_amount_sats {
            Some(amount) => {
                debug_assert!(invoice.amount().is_none());
                Some(Amount::try_from_sats_u64(amount)?)
            }
            None => {
                debug_assert!(invoice.amount().is_some());
                None
            }
        };

        Ok(Self {
            invoice,
            fallback_amount,
        })
    }
}

/// See [`common::api::command::PreflightPayInvoiceResponse`].
#[frb(dart_metadata=("freezed"))]
pub struct PreflightPayInvoiceResponse {
    pub amount_sats: u64,
    pub fees_sats: u64,
}

impl From<PreflightPayInvoiceResponseRs> for PreflightPayInvoiceResponse {
    fn from(value: PreflightPayInvoiceResponseRs) -> Self {
        Self {
            amount_sats: value.amount.sats_u64(),
            fees_sats: value.fees.sats_u64(),
        }
    }
}

/// See [`common::api::qs::UpdatePaymentNote`].
#[frb(dart_metadata=("freezed"))]
pub struct UpdatePaymentNote {
    pub index: PaymentIndex,
    pub note: Option<String>,
}

impl TryFrom<UpdatePaymentNote> for UpdatePaymentNoteRs {
    type Error = anyhow::Error;
    fn try_from(value: UpdatePaymentNote) -> Result<Self, Self::Error> {
        Ok(Self {
            index: PaymentIndexRs::try_from(value.index)?,
            note: value.note,
        })
    }
}
