//! API request and response types exposed to Dart.

use std::str::FromStr;

use anyhow::{Context, anyhow};
use lexe::types::{
    bitcoin::LnurlWithdrawRequest as LnurlWithdrawRequestRs,
    command::{
        CreateClientRequest as CreateClientRequestRs,
        CreateClientResponse as CreateClientResponseRs,
        GetHumanBitcoinAddressResponse as GetHumanBitcoinAddressResponseRs,
        RevokeClientRequest as RevokeClientRequestRs,
        WithdrawLnurlRequest as WithdrawLnurlRequestRs,
    },
};
use lexe_api::{
    models::command::{
        CloseChannelPreflightResponse as CloseChannelPreflightResponseRs,
        CloseChannelRequest as CloseChannelRequestRs,
        CreateInvoiceRequest as CreateInvoiceRequestRs,
        CreateInvoiceResponse as CreateInvoiceResponseRs,
        CreateOfferRequest as CreateOfferRequestRs,
        CreateOfferResponse as CreateOfferResponseRs,
        FeeEstimate as FeeEstimateRs,
        ListChannelsResponse as ListChannelsResponseRs, NodeInfo as NodeInfoRs,
        OpenChannelPreflightRequest as OpenChannelPreflightRequestRs,
        OpenChannelPreflightResponse as OpenChannelPreflightResponseRs,
        OpenChannelRequest as OpenChannelRequestRs,
        OpenChannelResponse as OpenChannelResponseRs,
        PayInvoicePreflightRequest as PayInvoicePreflightRequestRs,
        PayInvoicePreflightResponse as PayInvoicePreflightResponseRs,
        PayInvoiceRequest as PayInvoiceRequestRs,
        PayInvoiceResponse as PayInvoiceResponseRs,
        PayOfferPreflightRequest as PayOfferPreflightRequestRs,
        PayOfferPreflightResponse as PayOfferPreflightResponseRs,
        PayOfferRequest as PayOfferRequestRs,
        PayOfferResponse as PayOfferResponseRs,
        PayOnchainPreflightRequest as PayOnchainPreflightRequestRs,
        PayOnchainPreflightResponse as PayOnchainPreflightResponseRs,
        PayOnchainRequest as PayOnchainRequestRs,
        PayOnchainResponse as PayOnchainResponseRs,
        UpdatePersonalNote as UpdatePersonalNoteRs,
    },
    types::{
        bounded_string::BoundedString,
        invoice::Invoice as InvoiceRs,
        offer::{MaxQuantity, Offer as OfferRs},
        payments::{
            ClientPaymentId as ClientPaymentIdRs,
            PaymentCreatedIndex as PaymentCreatedIndexRs,
            PaymentId as PaymentIdRs, PaymentKind as PaymentKindRs,
        },
    },
};
use lexe_common::{
    api::fiat_rates::FiatRates as FiatRatesRs,
    ln::{
        amount::Amount,
        channel::{ChannelId, UserChannelId as UserChannelIdRs},
    },
};
use lexe_crypto::ed25519;

use crate::ffi::types::{
    ClientPaymentId, ConfirmationPriority, Invoice, LnurlWithdrawRequest,
    LxChannelDetails, Offer, PaymentCreatedIndex, PaymentKind, UserChannelId,
};

/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct Balance {
    /// The top-level balance we'll show on the main wallet page. Just
    /// `onchain_sats + lightning_sats` but handles msat.
    pub total_sats: u64,
    /// The total amount of onchain funds.
    pub onchain_sats: u64,
    /// The sum channel balance of all usable _and_ pending channels.
    pub lightning_sats: u64,
    /// The sum channel balance of all usable channels.
    pub lightning_usable_sats: u64,
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

        // Separate out `usable` and `max_sendable`.
        let lightning_usable = info.lightning_balance.usable;
        let lightning_max_sendable = info.lightning_balance.max_sendable;

        Self {
            total_sats: total.sats_u64(),
            lightning_sats: lightning.sats_u64(),
            lightning_usable_sats: lightning_usable.sats_u64(),
            lightning_max_sendable_sats: lightning_max_sendable.sats_u64(),
            onchain_sats: onchain.sats_u64(),
        }
    }
}

/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct OpenChannelRequest {
    pub user_channel_id: UserChannelId,
    pub value_sats: u64,
}

impl TryFrom<OpenChannelRequest> for OpenChannelRequestRs {
    type Error = anyhow::Error;
    fn try_from(req: OpenChannelRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            user_channel_id: UserChannelIdRs::from(req.user_channel_id),
            value: Amount::try_from_sats_u64(req.value_sats)?,
        })
    }
}

/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct OpenChannelPreflightRequest {
    pub value_sats: u64,
}

impl TryFrom<OpenChannelPreflightRequest> for OpenChannelPreflightRequestRs {
    type Error = anyhow::Error;
    fn try_from(req: OpenChannelPreflightRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            value: Amount::try_from_sats_u64(req.value_sats)?,
        })
    }
}

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct OpenChannelPreflightResponse {
    pub fee_estimate_sats: u64,
}

impl From<OpenChannelPreflightResponseRs> for OpenChannelPreflightResponse {
    fn from(resp: OpenChannelPreflightResponseRs) -> Self {
        Self {
            fee_estimate_sats: resp.fee_estimate.sats_u64(),
        }
    }
}

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct CloseChannelRequest {
    pub channel_id: String,
    // TODO(phlip9): force_close
}

impl TryFrom<CloseChannelRequest> for CloseChannelRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: CloseChannelRequest) -> anyhow::Result<Self> {
        Ok(Self {
            channel_id: ChannelId::from_str(&value.channel_id)?,
            force_close: false,
            maybe_counterparty: None,
        })
    }
}

pub type CloseChannelPreflightRequest = CloseChannelRequest;

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct CloseChannelPreflightResponse {
    pub fee_estimate_sats: u64,
}

impl From<CloseChannelPreflightResponseRs> for CloseChannelPreflightResponse {
    fn from(value: CloseChannelPreflightResponseRs) -> Self {
        Self {
            fee_estimate_sats: value.fee_estimate.sats_u64(),
        }
    }
}

/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct FiatRates {
    pub timestamp_ms: i64,
    // Sadly, the bridge doesn't currently support maps or tuples so... we'll
    // settle for a list...
    pub rates: Vec<FiatRate>,
}

/// flutter_rust_bridge:dart_metadata=("freezed")
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

fn validate_note(note: String) -> anyhow::Result<BoundedString> {
    BoundedString::new(note).map_err(|e| anyhow!("{e}"))
}

/// See `lexe_api::command::PayOnchainRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOnchainRequest {
    pub cid: ClientPaymentId,
    pub address: String,
    pub amount_sats: u64,
    pub priority: ConfirmationPriority,
    pub personal_note: Option<String>,
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
            personal_note: req.personal_note.map(validate_note).transpose()?,
        })
    }
}

/// See `lexe_api::command::PayOnchainResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOnchainResponse {
    pub index: PaymentCreatedIndex,
    pub txid: String,
}

impl PayOnchainResponse {
    pub(crate) fn from_cid_and_response(
        cid: ClientPaymentIdRs,
        resp: PayOnchainResponseRs,
    ) -> Self {
        let index = PaymentCreatedIndexRs {
            created_at: resp.created_at,
            id: PaymentIdRs::OnchainSend(cid),
        };
        Self {
            index: PaymentCreatedIndex::from(index),
            txid: resp.txid.to_string(),
        }
    }
}

/// See `lexe_api::command::PayOnchainPreflightRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOnchainPreflightRequest {
    pub address: String,
    pub amount_sats: u64,
}

impl TryFrom<PayOnchainPreflightRequest> for PayOnchainPreflightRequestRs {
    type Error = anyhow::Error;

    fn try_from(req: PayOnchainPreflightRequest) -> anyhow::Result<Self> {
        let address = bitcoin::Address::from_str(&req.address)
            .map_err(|_| anyhow!("The bitcoin address isn't valid."))?;
        let amount = Amount::try_from_sats_u64(req.amount_sats)?;

        Ok(Self { address, amount })
    }
}

/// See `lexe_api::command::PayOnchainPreflightResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOnchainPreflightResponse {
    pub high: Option<FeeEstimate>,
    pub normal: FeeEstimate,
    pub background: FeeEstimate,
}

impl From<PayOnchainPreflightResponseRs> for PayOnchainPreflightResponse {
    fn from(resp: PayOnchainPreflightResponseRs) -> Self {
        Self {
            high: resp.high.map(FeeEstimate::from),
            normal: FeeEstimate::from(resp.normal),
            background: FeeEstimate::from(resp.background),
        }
    }
}

/// See `lexe_api::command::FeeEstimate`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// See `lexe_api::command::CreateInvoiceRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct CreateInvoiceRequest {
    pub expiry_secs: u32,
    pub amount_sats: Option<u64>,
    pub description: Option<String>,
    pub personal_note: Option<String>,
    pub kind: PaymentKind,
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
            description_hash: None,
            message: None,
            personal_note: value
                .personal_note
                .map(BoundedString::new)
                .transpose()?,
            kind: PaymentKindRs::from(value.kind),
            partner_pk: None,
            partner_prop_fee: None,
            partner_base_fee: None,
        })
    }
}

/// See `lexe_api::command::CreateInvoiceResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// Mirrors the `lexe_api::command::PayInvoiceRequest` type.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayInvoiceRequest {
    pub invoice: String,
    pub fallback_amount_sats: Option<u64>,
    pub message: Option<String>,
    pub personal_note: Option<String>,
    pub kind: PaymentKind,
    pub ldk_route: Option<Vec<u8>>,
}

impl TryFrom<PayInvoiceRequest> for PayInvoiceRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: PayInvoiceRequest) -> Result<Self, Self::Error> {
        let invoice = InvoiceRs::from_str(&value.invoice)
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
            message: value.message.map(validate_note).transpose()?,
            personal_note: value
                .personal_note
                .map(validate_note)
                .transpose()?,
            kind: PaymentKindRs::from(value.kind),
            ldk_route: value.ldk_route,
        })
    }
}

/// Mirrors `lexe_api::command::PayInvoiceResponse` the type, but enriches
/// the response so we get the full `PaymentCreatedIndex`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayInvoiceResponse {
    pub index: PaymentCreatedIndex,
}

impl PayInvoiceResponse {
    pub(crate) fn from_id_and_response(
        id: PaymentIdRs,
        resp: PayInvoiceResponseRs,
    ) -> Self {
        let index = PaymentCreatedIndexRs {
            created_at: resp.created_at,
            id,
        };
        Self {
            index: PaymentCreatedIndex::from(index),
        }
    }
}

/// See `lexe_api::command::PayInvoicePreflightRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayInvoicePreflightRequest {
    pub invoice: String,
    pub fallback_amount_sats: Option<u64>,
    pub kind: PaymentKind,
}

impl TryFrom<PayInvoicePreflightRequest> for PayInvoicePreflightRequestRs {
    type Error = anyhow::Error;
    fn try_from(
        value: PayInvoicePreflightRequest,
    ) -> Result<Self, Self::Error> {
        let invoice = InvoiceRs::from_str(&value.invoice)
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
            kind: PaymentKindRs::from(value.kind),
        })
    }
}

/// See `lexe_api::command::PayInvoicePreflightResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayInvoicePreflightResponse {
    pub amount_sats: u64,
    pub fees_sats: u64,
    pub ldk_route: Vec<u8>,
}

impl From<PayInvoicePreflightResponseRs> for PayInvoicePreflightResponse {
    fn from(value: PayInvoicePreflightResponseRs) -> Self {
        // TODO(phlip9): display some route visualization in UI?
        Self {
            amount_sats: value.amount.sats_u64(),
            fees_sats: value.fees.sats_u64(),
            ldk_route: value.ldk_route,
        }
    }
}

/// See `lexe_api::command::CreateOfferRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct CreateOfferRequest {
    pub expiry_secs: Option<u32>,
    pub min_amount_sats: Option<u64>,
    pub description: Option<String>,
    pub issuer: Option<String>,
}

impl TryFrom<CreateOfferRequest> for CreateOfferRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: CreateOfferRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            expiry_secs: value.expiry_secs,
            min_amount: value
                .min_amount_sats
                .map(Amount::try_from_sats_u64)
                .transpose()?,
            description: value
                .description
                .map(BoundedString::new)
                .transpose()
                .context("Invalid description")?,
            // TODO(phlip9): user settable max_quantity probably doesn't provide
            // much value in a p2p payments app.
            max_quantity: Some(MaxQuantity::ONE),
            issuer: value
                .issuer
                .map(BoundedString::new)
                .transpose()
                .context("Invalid issuer")?,
        })
    }
}

/// See `lexe_api::command::CreateOfferResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
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

/// See `lexe_api::command::PayOfferPreflightRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOfferPreflightRequest {
    pub cid: ClientPaymentId,
    pub offer: String,
    pub amount_sats: u64,
}

impl TryFrom<PayOfferPreflightRequest> for PayOfferPreflightRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: PayOfferPreflightRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            cid: ClientPaymentIdRs::from(value.cid),
            offer: OfferRs::from_str(&value.offer)
                .context("Failed to parse offer")?,
            amount: Amount::try_from_sats_u64(value.amount_sats)?,
        })
    }
}

/// See `lexe_api::command::PayOfferPreflightResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOfferPreflightResponse {
    pub amount_sats: u64,
    pub fees_sats: u64,
}

impl From<PayOfferPreflightResponseRs> for PayOfferPreflightResponse {
    fn from(value: PayOfferPreflightResponseRs) -> Self {
        // TODO(phlip9): display some route visualization in UI?
        Self {
            amount_sats: value.amount.sats_u64(),
            fees_sats: value.fees.sats_u64(),
        }
    }
}

/// See `lexe_api::command::PayOfferResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOfferRequest {
    pub cid: ClientPaymentId,
    pub offer: String,
    pub amount_sats: u64,
    pub message: Option<String>,
    pub personal_note: Option<String>,
    pub kind: PaymentKind,
}

impl TryFrom<PayOfferRequest> for PayOfferRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: PayOfferRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            cid: ClientPaymentIdRs::from(value.cid),
            offer: OfferRs::from_str(&value.offer)
                .context("Failed to parse offer")?,
            amount: Amount::try_from_sats_u64(value.amount_sats)?,
            message: value.message.map(validate_note).transpose()?,
            personal_note: value
                .personal_note
                .map(validate_note)
                .transpose()?,
            kind: PaymentKindRs::from(value.kind),
        })
    }
}

/// See `lexe_api::command::PayOfferResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PayOfferResponse {
    /// When the node registered this payment. Used in the
    /// [`PaymentCreatedIndex`].
    pub index: PaymentCreatedIndex,
}

impl PayOfferResponse {
    pub(crate) fn from_id_and_response(
        id: PaymentIdRs,
        resp: PayOfferResponseRs,
    ) -> Self {
        let index = PaymentCreatedIndexRs {
            created_at: resp.created_at,
            id,
        };
        Self {
            index: PaymentCreatedIndex::from(index),
        }
    }
}

/// See [`WithdrawLnurlRequestRs`].
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct WithdrawLnurlRequest {
    pub withdraw_request: LnurlWithdrawRequest,
    pub amount_msat: u64,
    pub description: Option<String>,
    pub personal_note: Option<String>,
}

impl From<WithdrawLnurlRequest> for WithdrawLnurlRequestRs {
    fn from(value: WithdrawLnurlRequest) -> WithdrawLnurlRequestRs {
        WithdrawLnurlRequestRs {
            lnurl: None,
            withdraw_request: Some(LnurlWithdrawRequestRs::from(
                value.withdraw_request,
            )),
            amount: Some(Amount::from_msat(value.amount_msat)),
            description: value.description,
            personal_note: value.personal_note,
        }
    }
}

/// See `lexe_common::api::user::UpdatePersonalNote`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct UpdatePersonalNote {
    pub index: PaymentCreatedIndex,
    pub personal_note: Option<String>,
}

impl TryFrom<UpdatePersonalNote> for UpdatePersonalNoteRs {
    type Error = anyhow::Error;
    fn try_from(value: UpdatePersonalNote) -> Result<Self, Self::Error> {
        Ok(Self {
            index: PaymentCreatedIndexRs::try_from(value.index)?,
            personal_note: value
                .personal_note
                .map(validate_note)
                .transpose()?,
        })
    }
}

/// See `lexe::types::command::CreateClientRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
#[derive(Clone)]
pub struct CreateClientRequest {
    pub label: Option<String>,
}

impl From<CreateClientRequest> for CreateClientRequestRs {
    fn from(value: CreateClientRequest) -> Self {
        Self {
            expires_at: None,
            label: value.label,
        }
    }
}

/// See `lexe::types::command::CreateClientResponse`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct CreateClientResponse {
    pub pubkey: String,
    pub credentials: String,
}

impl From<CreateClientResponseRs> for CreateClientResponse {
    fn from(value: CreateClientResponseRs) -> Self {
        Self {
            pubkey: value.client_pk.to_string(),
            credentials: value.client_credentials.export_string(),
        }
    }
}

/// See `lexe::types::command::RevokeClientRequest`.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct RevokeClientRequest {
    pub pubkey: String,
}

impl TryFrom<RevokeClientRequest> for RevokeClientRequestRs {
    type Error = anyhow::Error;
    fn try_from(value: RevokeClientRequest) -> Result<Self, Self::Error> {
        let client_pk = ed25519::PublicKey::from_str(&value.pubkey)
            .context("Invalid pubkey")?;
        Ok(Self { client_pk })
    }
}

/// The user's Human Bitcoin Address.
///
/// The FFI form of the SDK's [`GetHumanBitcoinAddressResponseRs`].
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct GetHumanBitcoinAddressResponse {
    /// The Human Bitcoin Address (BIP 353), e.g. `₿satoshi@lexe.app`.
    pub human_bitcoin_address: String,
    /// The Lightning Address, e.g. `satoshi@lexe.app`.
    pub lightning_address: String,
    /// The BOLT 12 offer that the Human Bitcoin Address resolves to.
    pub offer: Offer,
    /// Whether the username can currently be changed.
    pub updatable: bool,
}

impl From<GetHumanBitcoinAddressResponseRs> for GetHumanBitcoinAddressResponse {
    fn from(resp: GetHumanBitcoinAddressResponseRs) -> Self {
        Self {
            human_bitcoin_address: resp.human_bitcoin_address,
            lightning_address: resp.lightning_address,
            offer: resp.offer.into(),
            updatable: resp.updatable,
        }
    }
}

impl TryFrom<GetHumanBitcoinAddressResponse>
    for GetHumanBitcoinAddressResponseRs
{
    type Error = anyhow::Error;

    fn try_from(ffi: GetHumanBitcoinAddressResponse) -> anyhow::Result<Self> {
        Ok(Self {
            human_bitcoin_address: ffi.human_bitcoin_address,
            lightning_address: ffi.lightning_address,
            offer: OfferRs::from_str(&ffi.offer.string)?,
            updatable: ffi.updatable,
        })
    }
}
