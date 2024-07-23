use std::str::FromStr;

use anyhow::anyhow;
use common::{
    env::DeployEnv as DeployEnvRs,
    ln::{
        invoice::LxInvoice,
        network::LxNetwork as NetworkRs,
        payments::{
            BasicPayment as BasicPaymentRs,
            ClientPaymentId as ClientPaymentIdRs,
            PaymentDirection as PaymentDirectionRs,
            PaymentIndex as PaymentIndexRs, PaymentKind as PaymentKindRs,
            PaymentStatus as PaymentStatusRs,
        },
        priority::ConfirmationPriority as ConfirmationPriorityRs,
    },
    rng::SysRng,
};
use flutter_rust_bridge::frb;

use crate::app::AppConfig;

/// See [`common::env::DeployEnv`]
#[frb(dart_metadata=("freezed"))]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeployEnv {
    Dev,
    Staging,
    Prod,
}

impl DeployEnv {
    #[frb(sync)]
    pub fn from_str(s: &str) -> anyhow::Result<Self> {
        DeployEnvRs::from_str(s).map(DeployEnv::from)
    }
}

impl From<DeployEnvRs> for DeployEnv {
    fn from(env: DeployEnvRs) -> Self {
        match env {
            DeployEnvRs::Dev => Self::Dev,
            DeployEnvRs::Staging => Self::Staging,
            DeployEnvRs::Prod => Self::Prod,
        }
    }
}

impl From<DeployEnv> for DeployEnvRs {
    fn from(env: DeployEnv) -> Self {
        match env {
            DeployEnv::Dev => Self::Dev,
            DeployEnv::Staging => Self::Staging,
            DeployEnv::Prod => Self::Prod,
        }
    }
}

/// See [`common::ln::network::LxNetwork`]
#[derive(Copy, Clone, Debug)]
pub enum Network {
    Mainnet,
    Testnet,
    Regtest,
}

impl Network {
    #[frb(sync)]
    pub fn from_str(s: &str) -> anyhow::Result<Network> {
        NetworkRs::from_str(s).and_then(Network::try_from)
    }
}

impl From<Network> for NetworkRs {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => NetworkRs::Mainnet,
            Network::Testnet => NetworkRs::Testnet,
            Network::Regtest => NetworkRs::Regtest,
        }
    }
}

impl TryFrom<NetworkRs> for Network {
    type Error = anyhow::Error;

    fn try_from(network: NetworkRs) -> anyhow::Result<Self> {
        match network {
            NetworkRs::Mainnet => Ok(Self::Mainnet),
            NetworkRs::Testnet => Ok(Self::Testnet),
            NetworkRs::Regtest => Ok(Self::Regtest),
            _ => Err(anyhow!("unsupported NETWORK: '{network}'")),
        }
    }
}

/// Dart-serializable configuration we get from the flutter side.
#[frb(dart_metadata=("freezed"))]
pub struct Config {
    pub deploy_env: DeployEnv,
    pub network: Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub base_app_data_dir: String,
    pub use_mock_secret_store: bool,
}

impl From<Config> for AppConfig {
    fn from(c: Config) -> Self {
        AppConfig::from_dart_config(
            DeployEnvRs::from(c.deploy_env),
            NetworkRs::from(c.network),
            c.gateway_url,
            c.use_sgx,
            c.base_app_data_dir,
            c.use_mock_secret_store,
        )
    }
}

pub enum PaymentDirection {
    Inbound,
    Outbound,
}

impl From<PaymentDirectionRs> for PaymentDirection {
    fn from(value: PaymentDirectionRs) -> Self {
        match value {
            PaymentDirectionRs::Inbound => Self::Inbound,
            PaymentDirectionRs::Outbound => Self::Outbound,
        }
    }
}

pub enum PaymentStatus {
    Pending,
    Completed,
    Failed,
}

impl From<PaymentStatusRs> for PaymentStatus {
    fn from(value: PaymentStatusRs) -> Self {
        match value {
            PaymentStatusRs::Pending => Self::Pending,
            PaymentStatusRs::Completed => Self::Completed,
            PaymentStatusRs::Failed => Self::Failed,
        }
    }
}

pub enum PaymentKind {
    Onchain,
    Invoice,
    Spontaneous,
}

impl From<PaymentKindRs> for PaymentKind {
    fn from(value: PaymentKindRs) -> Self {
        match value {
            PaymentKindRs::Onchain => Self::Onchain,
            PaymentKindRs::Invoice => Self::Invoice,
            PaymentKindRs::Spontaneous => Self::Spontaneous,
        }
    }
}

/// See [`common::ln::payments::PaymentIndex`].
#[frb(dart_metadata=("freezed"))]
pub struct PaymentIndex(pub String);

impl From<PaymentIndexRs> for PaymentIndex {
    fn from(value: PaymentIndexRs) -> Self {
        Self(value.to_string())
    }
}

impl TryFrom<PaymentIndex> for PaymentIndexRs {
    type Error = anyhow::Error;
    fn try_from(value: PaymentIndex) -> Result<Self, Self::Error> {
        PaymentIndexRs::from_str(&value.0)
    }
}

/// Just the info we need to display an entry in the payments list UI.
#[frb(dart_metadata=("freezed"))]
pub struct ShortPayment {
    pub index: PaymentIndex,

    pub kind: PaymentKind,
    pub direction: PaymentDirection,

    pub amount_sat: Option<u64>,

    pub status: PaymentStatus,

    pub note: Option<String>,

    pub created_at: i64,
}

impl From<&BasicPaymentRs> for ShortPayment {
    fn from(payment: &BasicPaymentRs) -> Self {
        Self {
            index: PaymentIndex::from(*payment.index()),

            kind: PaymentKind::from(payment.kind),
            direction: PaymentDirection::from(payment.direction),

            amount_sat: payment.amount.map(|amt| amt.sats_u64()),

            status: PaymentStatus::from(payment.status),

            note: payment.note_or_description().map(String::from),

            created_at: payment.created_at().as_i64(),
        }
    }
}

/// Just a `(usize, ShortPayment)`, but packaged in a struct until
/// `flutter_rust_bridge` stops breaking on tuples.
// TODO(phlip9): remove this after updating frb
pub struct ShortPaymentAndIndex {
    pub vec_idx: usize,
    pub payment: ShortPayment,
}

/// The complete payment info, used in the payment detail page. Mirrors the
/// [`BasicPaymentRs`] type.
#[frb(dart_metadata=("freezed"))]
pub struct Payment {
    pub index: PaymentIndex,

    pub kind: PaymentKind,
    pub direction: PaymentDirection,

    pub invoice: Option<Invoice>,

    pub replacement: Option<String>,

    pub amount_sat: Option<u64>,
    pub fees_sat: u64,

    pub status: PaymentStatus,
    pub status_str: String,

    pub note: Option<String>,

    pub created_at: i64,
    pub finalized_at: Option<i64>,
}

impl From<&BasicPaymentRs> for Payment {
    fn from(payment: &BasicPaymentRs) -> Self {
        Self {
            index: PaymentIndex::from(*payment.index()),

            kind: PaymentKind::from(payment.kind),
            direction: PaymentDirection::from(payment.direction),

            invoice: payment.invoice.as_ref().map(Invoice::from),

            replacement: payment.replacement.map(|txid| txid.to_string()),

            amount_sat: payment.amount.map(|amt| amt.sats_u64()),
            fees_sat: payment.fees.sats_u64(),

            status: PaymentStatus::from(payment.status),
            status_str: payment.status_str.clone(),

            note: payment.note_or_description().map(String::from),

            created_at: payment.created_at().as_i64(),
            finalized_at: payment.finalized_at.map(|t| t.as_i64()),
        }
    }
}

/// A potential scanned/pasted payment.
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(Invoice),
    Offer, // TODO(phlip9): support BOLT12 offers
}

impl From<payment_uri::PaymentMethod> for PaymentMethod {
    fn from(value: payment_uri::PaymentMethod) -> Self {
        match value {
            payment_uri::PaymentMethod::Onchain(x) =>
                Self::Onchain(Onchain::from(x)),
            payment_uri::PaymentMethod::Invoice(x) =>
                Self::Invoice(Invoice::from(x)),
            payment_uri::PaymentMethod::Offer(_) => Self::Offer,
        }
    }
}

/// A potential onchain Bitcoin payment.
#[frb(dart_metadata=("freezed"))]
pub struct Onchain {
    pub address: String,
    pub amount_sats: Option<u64>,
    pub label: Option<String>,
    pub message: Option<String>,
}

impl From<payment_uri::Onchain> for Onchain {
    fn from(value: payment_uri::Onchain) -> Self {
        Self {
            address: value.address.assume_checked().to_string(),
            amount_sats: value.amount.map(|amt| amt.sats_u64()),
            label: value.label,
            message: value.message,
        }
    }
}

/// A lightning invoice with useful fields parsed out for the flutter frontend.
/// Mirrors the [`LxInvoice`] type.
#[frb(dart_metadata=("freezed"))]
pub struct Invoice {
    pub string: String,

    pub description: Option<String>,

    pub created_at: i64,
    pub expires_at: i64,

    pub amount_sats: Option<u64>,

    pub payee_pubkey: String,
}

impl From<&LxInvoice> for Invoice {
    fn from(invoice: &LxInvoice) -> Self {
        Self {
            string: invoice.to_string(),

            description: invoice.description_str().map(String::from),

            created_at: invoice.saturating_created_at().as_i64(),
            expires_at: invoice.saturating_expires_at().as_i64(),

            amount_sats: invoice.amount_sats(),

            payee_pubkey: invoice.payee_node_pk().to_string(),
        }
    }
}

impl From<LxInvoice> for Invoice {
    #[inline]
    fn from(value: LxInvoice) -> Self {
        Self::from(&value)
    }
}

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
#[frb(dart_metadata=("freezed"))]
pub struct ClientPaymentId {
    pub id: [u8; 32],
}

impl ClientPaymentId {
    #[frb(sync)]
    pub fn gen() -> Self {
        ClientPaymentId {
            id: ClientPaymentIdRs::from_rng(&mut SysRng::new()).0,
        }
    }
}

impl From<ClientPaymentId> for ClientPaymentIdRs {
    fn from(value: ClientPaymentId) -> ClientPaymentIdRs {
        ClientPaymentIdRs(value.id)
    }
}

pub enum ConfirmationPriority {
    High,
    Normal,
    Background,
}

impl From<ConfirmationPriority> for ConfirmationPriorityRs {
    fn from(value: ConfirmationPriority) -> Self {
        match value {
            ConfirmationPriority::High => Self::High,
            ConfirmationPriority::Normal => Self::Normal,
            ConfirmationPriority::Background => Self::Background,
        }
    }
}
