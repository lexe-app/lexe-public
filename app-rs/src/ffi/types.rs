use std::str::FromStr;

use anyhow::anyhow;
pub(crate) use common::root_seed::RootSeed as RootSeedRs;
use common::{
    ExposeSecret,
    api::{
        auth::Scope as ScopeRs,
        revocable_clients::RevocableClient as RevocableClientRs,
    },
    env::DeployEnv as DeployEnvRs,
    ln::{
        amount::Amount as AmountRs,
        channel::{
            LxChannelDetails as LxChannelDetailsRs,
            LxUserChannelId as LxUserChannelIdRs,
        },
        network::LxNetwork as NetworkRs,
        priority::ConfirmationPriority as ConfirmationPriorityRs,
    },
    rng::SysRng,
    time::TimestampMs,
};
use flutter_rust_bridge::RustOpaqueNom;
use lexe_api::{
    models::command::{
        BackupInfo as BackupInfoRs, GDriveStatus as GDriveStatusRs,
    },
    types::{
        invoice::LxInvoice,
        offer::LxOffer,
        payments::{
            BasicPayment as BasicPaymentRs,
            ClientPaymentId as ClientPaymentIdRs,
            PaymentCreatedIndex as PaymentCreatedIndexRs,
            PaymentDirection as PaymentDirectionRs,
            PaymentKind as PaymentKindRs, PaymentStatus as PaymentStatusRs,
        },
    },
};

use crate::{
    app::AppConfig, types::GDriveSignupCredentials as GDriveSignupCredentialsRs,
};

/// See [`common::env::DeployEnv`]
///
/// flutter_rust_bridge:dart_metadata=("freezed")
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DeployEnv {
    Dev,
    Staging,
    Prod,
}

impl DeployEnv {
    /// flutter_rust_bridge:sync
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
    Testnet3,
    Testnet4,
    Regtest,
}

impl Network {
    /// flutter_rust_bridge:sync
    pub fn from_str(s: &str) -> anyhow::Result<Network> {
        NetworkRs::from_str(s).and_then(Network::try_from)
    }
}

impl From<Network> for NetworkRs {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => NetworkRs::Mainnet,
            Network::Testnet3 => NetworkRs::Testnet3,
            Network::Testnet4 => NetworkRs::Testnet4,
            Network::Regtest => NetworkRs::Regtest,
        }
    }
}

impl TryFrom<NetworkRs> for Network {
    type Error = anyhow::Error;

    fn try_from(network: NetworkRs) -> anyhow::Result<Self> {
        match network {
            NetworkRs::Mainnet => Ok(Self::Mainnet),
            NetworkRs::Testnet3 => Ok(Self::Testnet3),
            NetworkRs::Testnet4 => Ok(Self::Testnet4),
            NetworkRs::Regtest => Ok(Self::Regtest),
            _ => Err(anyhow!("unsupported NETWORK: '{network}'")),
        }
    }
}

/// Dart-serializable configuration we get from the flutter side.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct Config {
    pub deploy_env: DeployEnv,
    pub network: Network,
    pub gateway_url: String,
    pub use_sgx: bool,
    pub base_app_data_dir: String,
    pub use_mock_secret_store: bool,
    pub user_agent: String,
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
            c.user_agent,
        )
    }
}

/// The user's root seed from which we derive all child secrets.
pub struct RootSeed {
    pub(crate) inner: RustOpaqueNom<RootSeedRs>,
}

impl RootSeed {
    /// Generate a new RootSeed from the secure system RNG.
    ///
    /// flutter_rust_bridge:sync
    pub fn from_sys_rng() -> Self {
        Self {
            inner: RustOpaqueNom::new(RootSeedRs::from_rng(&mut SysRng::new())),
        }
    }

    /// Hex-encode the root seed secret. Should only be used for debugging.
    ///
    /// flutter_rust_bridge:sync
    pub fn expose_secret_hex(&self) -> String {
        hex::encode(self.inner.expose_secret().as_slice())
    }

    /// Return the 24-word BIP-39 seed phrase for this root seed.
    ///
    /// flutter_rust_bridge:sync
    pub fn seed_phrase(&self) -> Vec<String> {
        let mnemonic = self.inner.to_mnemonic();
        assert_eq!(mnemonic.word_count(), 24);
        mnemonic.words().map(|w| w.to_owned()).collect::<Vec<_>>()
    }

    /// flutter_rust_bridge:sync
    pub fn from_mnemonic(mnemonic: Vec<String>) -> anyhow::Result<Self> {
        let words = mnemonic.join(" ");
        let mnemonic = bip39::Mnemonic::parse_in_normalized(
            bip39::Language::English,
            words.as_str(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to parse mnemonic: {e}"))?;

        let root_seed_rs = RootSeedRs::try_from(mnemonic)?;
        Ok(Self {
            inner: RustOpaqueNom::new(root_seed_rs),
        })
    }
}

impl From<RootSeedRs> for RootSeed {
    fn from(inner: RootSeedRs) -> Self {
        Self {
            inner: RustOpaqueNom::new(inner),
        }
    }
}

pub struct GDriveSignupCredentials {
    /// The server auth code passed to the node enclave during provisioning.
    pub server_auth_code: String,
    /// The user's backup password, used to encrypt their [`RootSeed`] backup
    /// on Google Drive.
    pub password: String,
}

impl From<GDriveSignupCredentials> for GDriveSignupCredentialsRs {
    fn from(creds: GDriveSignupCredentials) -> Self {
        Self {
            server_auth_code: creds.server_auth_code,
            password: creds.password,
        }
    }
}

/// Some assorted user/node info. This is kinda hacked together currently just
/// to support account deletion requests.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct AppUserInfo {
    pub user_pk: String,
    pub node_pk: String,
    pub node_pk_proof: String,
}

// See `crate::app::AppUserInfo::to_ffi` for the conversion. Can't figure out
// why frb keeps RustAutoOpaque'ing this type if I impl the conversion here.

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
    Offer,
}

impl From<PaymentKindRs> for PaymentKind {
    fn from(value: PaymentKindRs) -> Self {
        match value {
            PaymentKindRs::Onchain => Self::Onchain,
            PaymentKindRs::Invoice => Self::Invoice,
            PaymentKindRs::Spontaneous => Self::Spontaneous,
            PaymentKindRs::Offer => Self::Offer,
        }
    }
}

/// See [`lexe_api::types::payments::PaymentCreatedIndex`].
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct PaymentCreatedIndex(pub String);

impl From<PaymentCreatedIndexRs> for PaymentCreatedIndex {
    fn from(value: PaymentCreatedIndexRs) -> Self {
        Self(value.to_string())
    }
}

impl TryFrom<PaymentCreatedIndex> for PaymentCreatedIndexRs {
    type Error = anyhow::Error;
    fn try_from(value: PaymentCreatedIndex) -> Result<Self, Self::Error> {
        PaymentCreatedIndexRs::from_str(&value.0)
    }
}

/// Just the info we need to display an entry in the payments list UI.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct ShortPayment {
    pub index: PaymentCreatedIndex,

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
            index: PaymentCreatedIndex::from(*payment.index()),

            kind: PaymentKind::from(payment.kind),
            direction: PaymentDirection::from(payment.direction),

            amount_sat: payment.amount.map(|amt| amt.sats_u64()),

            status: PaymentStatus::from(payment.status),

            note: payment.note_or_description().map(String::from),

            created_at: payment.created_at().to_i64(),
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
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct Payment {
    pub index: PaymentCreatedIndex,

    pub kind: PaymentKind,
    pub direction: PaymentDirection,

    pub invoice: Option<Invoice>,

    pub offer_id: Option<String>,
    pub offer: Option<Offer>,

    pub txid: Option<String>,
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
            index: PaymentCreatedIndex::from(*payment.index()),

            kind: PaymentKind::from(payment.kind),
            direction: PaymentDirection::from(payment.direction),

            invoice: payment.invoice.as_deref().map(Invoice::from),

            offer_id: payment.offer_id.map(|id| id.to_string()),
            offer: payment.offer.as_deref().map(Offer::from),

            txid: payment.txid.map(|txid| txid.to_string()),
            replacement: payment.replacement.map(|txid| txid.to_string()),

            amount_sat: payment.amount.map(|amt| amt.sats_u64()),
            fees_sat: payment.fees.sats_u64(),

            status: PaymentStatus::from(payment.status),
            status_str: payment.status_str.clone(),

            note: payment.note_or_description().map(String::from),

            created_at: payment.created_at().to_i64(),
            finalized_at: payment.finalized_at.map(|t| t.to_i64()),
        }
    }
}

/// A potential scanned/pasted payment.
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(Invoice),
    Offer(Offer),
    LnurlPayRequest(LnurlPayRequest),
}

impl From<payment_uri::PaymentMethod> for PaymentMethod {
    fn from(value: payment_uri::PaymentMethod) -> Self {
        match value {
            payment_uri::PaymentMethod::Onchain(x) =>
                Self::Onchain(Onchain::from(x)),
            payment_uri::PaymentMethod::Invoice(x) =>
                Self::Invoice(Invoice::from(x)),
            payment_uri::PaymentMethod::Offer(x) => Self::Offer(Offer::from(x)),
            payment_uri::PaymentMethod::LnurlPayRequest(x) =>
                Self::LnurlPayRequest(LnurlPayRequest::from(x)),
        }
    }
}

/// A potential onchain Bitcoin payment.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
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
///
/// flutter_rust_bridge:dart_metadata=("freezed")
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

            created_at: invoice.saturating_created_at().to_i64(),
            expires_at: invoice.saturating_expires_at().to_i64(),

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

/// A lightning offer with useful fields parsed out for the flutter frontend.
/// Mirrors the [`LxOffer`] type.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct Offer {
    pub string: String,

    pub description: Option<String>,

    pub expires_at: Option<i64>,
    pub amount_sats: Option<u64>,

    pub payee: Option<String>,
    pub payee_pubkey: Option<String>,
}

impl From<&LxOffer> for Offer {
    fn from(offer: &LxOffer) -> Self {
        Self {
            string: offer.to_string(),

            description: offer.description().map(String::from),

            expires_at: offer.expires_at().map(TimestampMs::to_i64),
            amount_sats: offer.amount().map(|amt| amt.sats_u64()),

            payee: offer.payee().map(String::from),
            payee_pubkey: offer.payee_node_pk().map(|pk| pk.to_string()),
        }
    }
}

impl From<LxOffer> for Offer {
    #[inline]
    fn from(value: LxOffer) -> Self {
        Self::from(&value)
    }
}

pub struct LnurlPayRequest {
    pub callback: String,
    pub min_sendable_msat: u64,
    pub max_sendable_msat: u64,
    pub metadata: LnurlPayRequestMetadata,
}

pub struct LnurlPayRequestMetadata {
    pub description: String,
    pub long_description: Option<String>,
    pub image_png_base64: Option<String>,
    pub image_jpeg_base64: Option<String>,
    pub identifier: Option<String>,
    pub email: Option<String>,
    pub description_hash: [u8; 32],
    pub raw: String,
}

impl From<payment_uri::LnurlPayRequest> for LnurlPayRequest {
    fn from(value: payment_uri::LnurlPayRequest) -> Self {
        Self {
            callback: value.callback,
            min_sendable_msat: value.min_sendable.msat(),
            max_sendable_msat: value.max_sendable.msat(),
            metadata: LnurlPayRequestMetadata::from(value.metadata),
        }
    }
}

impl From<payment_uri::LnurlPayRequestMetadata> for LnurlPayRequestMetadata {
    fn from(value: payment_uri::LnurlPayRequestMetadata) -> Self {
        Self {
            description: value.description,
            long_description: value.long_description,
            image_png_base64: value.image_png_base64,
            image_jpeg_base64: value.image_jpeg_base64,
            identifier: value.identifier,
            email: value.email,
            description_hash: value.description_hash,
            raw: value.raw,
        }
    }
}

impl From<LnurlPayRequest> for payment_uri::LnurlPayRequest {
    fn from(value: LnurlPayRequest) -> Self {
        payment_uri::LnurlPayRequest {
            callback: value.callback,
            min_sendable: AmountRs::from_msat(value.min_sendable_msat),
            max_sendable: AmountRs::from_msat(value.max_sendable_msat),
            metadata: payment_uri::LnurlPayRequestMetadata::from(
                value.metadata,
            ),
        }
    }
}

impl From<LnurlPayRequestMetadata> for payment_uri::LnurlPayRequestMetadata {
    fn from(value: LnurlPayRequestMetadata) -> Self {
        payment_uri::LnurlPayRequestMetadata {
            description: value.description,
            long_description: value.long_description,
            image_png_base64: value.image_png_base64,
            image_jpeg_base64: value.image_jpeg_base64,
            identifier: value.identifier,
            email: value.email,
            description_hash: value.description_hash,
            raw: value.raw,
        }
    }
}

/// A unique, client-generated id for payment types (onchain send,
/// ln spontaneous send) that need an extra id for idempotency.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct ClientPaymentId {
    pub id: [u8; 32],
}

impl ClientPaymentId {
    /// flutter_rust_bridge:sync
    pub fn gen_new() -> Self {
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

/// A unique, client-generated id for `open_channel`.
///
/// - Provides idempotency, to avoid opening duplicate channels on
///   `open_channel` retries.
/// - The `ChannelId` is only assigned when the channel finishes negotiation and
///   we build the channel funding txo.
///
/// flutter_rust_bridge:dart_metadata=("freezed")
pub struct UserChannelId {
    pub id: [u8; 16],
}

impl UserChannelId {
    /// flutter_rust_bridge:sync
    pub fn gen_new() -> Self {
        UserChannelId {
            id: LxUserChannelIdRs::from_rng(&mut SysRng::new()).0,
        }
    }
}

impl From<UserChannelId> for LxUserChannelIdRs {
    fn from(value: UserChannelId) -> LxUserChannelIdRs {
        LxUserChannelIdRs(value.id)
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

pub struct LxChannelDetails {
    pub channel_id: String,
    pub counterparty_node_id: String,
    pub channel_value_sats: u64,

    pub is_usable: bool,

    pub our_balance_sats: u64,
    pub outbound_capacity_sats: u64,
    pub next_outbound_htlc_limit_sats: u64,

    pub their_balance_sats: u64,
    pub inbound_capacity_sats: u64,
    //
    // TODO(phlip9): how to handle proportional fee
    // pub our_base_fee_sats: u64,
    // pub our_prop_fee_percent: String,
}

impl From<LxChannelDetailsRs> for LxChannelDetails {
    fn from(value: LxChannelDetailsRs) -> Self {
        Self {
            channel_id: value.channel_id.to_string(),
            counterparty_node_id: value.counterparty_node_id.to_string(),
            channel_value_sats: value.channel_value.sats_u64(),
            is_usable: value.is_usable,
            our_balance_sats: value.our_balance.sats_u64(),
            outbound_capacity_sats: value.outbound_capacity.sats_u64(),
            next_outbound_htlc_limit_sats: value
                .next_outbound_htlc_limit
                .sats_u64(),
            their_balance_sats: value.their_balance.sats_u64(),
            inbound_capacity_sats: value.inbound_capacity.sats_u64(),
            // our_base_fee_sats: value.our_base_fee.sats_u64(),
            // our_prop_fee: value.our_prop_fee.satu,
        }
    }
}

#[derive(Clone)]
pub enum Scope {
    All,
    NodeConnect,
}

impl From<Scope> for ScopeRs {
    fn from(value: Scope) -> Self {
        match value {
            Scope::All => Self::All,
            Scope::NodeConnect => Self::NodeConnect,
        }
    }
}

impl From<ScopeRs> for Scope {
    fn from(value: ScopeRs) -> Self {
        match value {
            ScopeRs::All => Self::All,
            ScopeRs::NodeConnect => Self::NodeConnect,
        }
    }
}

pub struct RevocableClient {
    pub pubkey: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub scope: Scope,
}

impl From<RevocableClientRs> for RevocableClient {
    fn from(value: RevocableClientRs) -> Self {
        Self {
            pubkey: value.pubkey.to_string(),
            created_at: value.created_at.to_i64(),
            label: value.label,
            scope: Scope::from(value.scope),
        }
    }
}

pub enum GDriveStatus {
    Ok,
    Error(String),
    Disabled,
}

impl From<GDriveStatusRs> for GDriveStatus {
    fn from(value: GDriveStatusRs) -> Self {
        match value {
            GDriveStatusRs::Ok => Self::Ok,
            GDriveStatusRs::Error(err) => Self::Error(err),
            GDriveStatusRs::Disabled => Self::Disabled,
        }
    }
}

pub struct BackupInfo {
    pub gdrive_status: GDriveStatus,
}

impl From<BackupInfoRs> for BackupInfo {
    fn from(value: BackupInfoRs) -> Self {
        Self {
            gdrive_status: GDriveStatus::from(value.gdrive_status),
        }
    }
}
