//! API request and response types exposed to Dart.

use std::str::FromStr;

use anyhow::{anyhow, Context};
use common::{
    api::{
        command::{
            CloseChannelRequest as CloseChannelRequestRs,
            CreateInvoiceRequest as CreateInvoiceRequestRs,
            CreateInvoiceResponse as CreateInvoiceResponseRs,
            CreateOfferRequest as CreateOfferRequestRs,
            CreateOfferResponse as CreateOfferResponseRs,
            FeeEstimate as FeeEstimateRs,
            ListChannelsResponse as ListChannelsResponseRs,
            NodeInfo as NodeInfoRs, OpenChannelRequest as OpenChannelRequestRs,
            OpenChannelResponse as OpenChannelResponseRs,
            PayInvoiceRequest as PayInvoiceRequestRs,
            PayInvoiceResponse as PayInvoiceResponseRs,
            PayOnchainRequest as PayOnchainRequestRs,
            PayOnchainResponse as PayOnchainResponseRs,
            PreflightCloseChannelResponse as PreflightCloseChannelResponseRs,
            PreflightOpenChannelRequest as PreflightOpenChannelRequestRs,
            PreflightOpenChannelResponse as PreflightOpenChannelResponseRs,
            PreflightPayInvoiceRequest as PreflightPayInvoiceRequestRs,
            PreflightPayInvoiceResponse as PreflightPayInvoiceResponseRs,
            PreflightPayOnchainRequest as PreflightPayOnchainRequestRs,
            PreflightPayOnchainResponse as PreflightPayOnchainResponseRs,
            UpdatePaymentNote as UpdatePaymentNoteRs,
        },
        fiat_rates::FiatRates as FiatRatesRs,
    },
    ln::{
        amount::Amount,
        channel::{LxChannelId, LxUserChannelId as LxUserChannelIdRs},
        invoice::LxInvoice,
        offer::MaxQuantity,
        payments::{
            ClientPaymentId as ClientPaymentIdRs, LxPaymentId as LxPaymentIdRs,
            PaymentIndex as PaymentIndexRs,
        },
    },
};
use flutter_rust_bridge::frb;

use crate::ffi::types::{
    ClientInfo, ClientPaymentId, ConfirmationPriority, Invoice,
    LxChannelDetails, Offer, PaymentIndex, Scope, UserChannelId,
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
    /// The top-level balance we'll show on the main wallet page. Just
    /// `onchain_sats + lightning_sats` but handles msat.
    pub total_sats: u64,
    /// The total amount of onchain funds.
    pub onchain_sats: u64,
    /// The sum channel balance of all usable _and_ pending channels.
    pub lightning_sats: u64,
    /// Upper-bound on the largest LN send amount we can make right now.
    /// Accounts for required Lexe fees. The user is unlikely to successfully
    /// send this value to any non-Lexe recipient.
    pub lightning_max_sendable_sats: u64,
}

impl From<&NodeInfoRs> for Balance {
    fn from(info: &NodeInfoRs) -> Self {
        // We discovered that you can in fact spend untrusted_pending outputs.
        // The only class that technically can't be spent yet is for immature
        // coinbase outputs, but I don't expect people to mine directly into
        // their Lexe wallet. It's conceptually simpler to use total here.
        let onchain = Amount::try_from(info.onchain_balance.total()).expect(
            "We somehow have over 21 million BTC in our on-chain wallet",
        );

        // We previously showed only `usable` for the top-level LN balance, but
        // that looks weird when you have a channel that's pending open and your
        // top-level balance shows the correct amount but your LN balance shows
        // 0 sats.
        let lightning = info.lightning_balance.total();

        // The total, top-level balance on the wallet page. Do this sum in Rust
        // so we handle sub-sat (msat) amounts correctly.
        let total = onchain + lightning;

        // Separate out `max_sendable`.
        let lightning_max_sendable = info.lightning_balance.max_sendable;

        Self {
            total_sats: total.sats_u64(),
            lightning_sats: lightning.sats_u64(),
            lightning_max_sendable_sats: lightning_max_sendable.sats_u64(),
            onchain_sats: onchain.sats_u64(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct ListChannelsResponse {
    pub channels: Vec<LxChannelDetails>,
}

impl From<ListChannelsResponseRs> for ListChannelsResponse {
    fn from(resp: ListChannelsResponseRs) -> Self {
        Self {
            channels: resp
                .channels
                .into_iter()
                .map(LxChannelDetails::from)
                .collect(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct OpenChannelRequest {
    pub user_channel_id: UserChannelId,
    pub value_sats: u64,
}

impl TryFrom<OpenChannelRequest> for OpenChannelRequestRs {
    type Error = anyhow::Error;
    fn try_from(req: OpenChannelRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            user_channel_id: LxUserChannelIdRs::from(req.user_channel_id),
            value: Amount::try_from_sats_u64(req.value_sats)?,
        })
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct OpenChannelResponse {
    pub channel_id: String,
}

impl From<OpenChannelResponseRs> for OpenChannelResponse {
    fn from(resp: OpenChannelResponseRs) -> Self {
        Self {
            channel_id: resp.channel_id.to_string(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct PreflightOpenChannelRequest {
    pub value_sats: u64,
}

impl TryFrom<PreflightOpenChannelRequest> for PreflightOpenChannelRequestRs {
    type Error = anyhow::Error;
    fn try_from(req: PreflightOpenChannelRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            value: Amount::try_from_sats_u64(req.value_sats)?,
        })
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct PreflightOpenChannelResponse {
    pub fee_estimate_sats: u64,
}

impl From<PreflightOpenChannelResponseRs> for PreflightOpenChannelResponse {
    fn from(resp: PreflightOpenChannelResponseRs) -> Self {
        Self {
            fee_estimate_sats: resp.fee_estimate.sats_u64(),
        }
    }
}

#[frb(dart_metadata=("freezed"))]
pub struct CloseChannelRequest {
    pub channel_id: String,
    // TODO(phlip9): force_close
}

impl TryFrom<CloseChannelRequest> for CloseChannelRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: CloseChannelRequest) -> anyhow::Result<Self> {
        Ok(Self {
            channel_id: LxChannelId::from_str(&value.channel_id)?,
            force_close: false,
            maybe_counterparty: None,
        })
    }
}

pub type PreflightCloseChannelRequest = CloseChannelRequest;

#[frb(dart_metadata=("freezed"))]
pub struct PreflightCloseChannelResponse {
    pub fee_estimate_sats: u64,
}

impl From<PreflightCloseChannelResponseRs> for PreflightCloseChannelResponse {
    fn from(value: PreflightCloseChannelResponseRs) -> Self {
        Self {
            fee_estimate_sats: value.fee_estimate.sats_u64(),
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
            timestamp_ms: value.timestamp_ms.to_i64(),
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

/// See [`common::api::command::CreateOfferRequest`].
#[frb(dart_metadata=("freezed"))]
pub struct CreateOfferRequest {
    pub expiry_secs: Option<u32>,
    pub amount_sats: Option<u64>,
    pub description: Option<String>,
}

impl TryFrom<CreateOfferRequest> for CreateOfferRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: CreateOfferRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            expiry_secs: value.expiry_secs,
            amount: value
                .amount_sats
                .map(Amount::try_from_sats_u64)
                .transpose()?,
            description: value.description,
            // TODO(phlip9): user settable max_quantity probably doesn't provide
            // much value in a p2p payments app.
            max_quantity: Some(MaxQuantity::ONE),
        })
    }
}

/// See [`common::api::command::CreateOfferResponse`].
#[frb(dart_metadata=("freezed"))]
pub struct CreateOfferResponse {
    pub offer: Offer,
}

impl From<CreateOfferResponseRs> for CreateOfferResponse {
    fn from(value: CreateOfferResponseRs) -> Self {
        Self {
            offer: Offer::from(value.offer),
        }
    }
}

/// See [`common::api::user::UpdatePaymentNote`].
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

pub struct CreateClientRequest {
    pub label: Option<String>,
    pub scope: Scope,
}

pub struct CreateClientResponse {
    pub client_info: ClientInfo,
    pub auth_json: String,
}
