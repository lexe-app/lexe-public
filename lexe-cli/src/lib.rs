//! `lexe-cli` wraps the Lexe Rust SDK (`lexe` crate) and exposes its methods
//! via a command-line interface.

use std::{borrow::Cow, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use lexe::{
    bip39::Mnemonic,
    config::{Network, WalletEnvConfig},
    types::{
        auth::{
            ClientCredentials, Credentials, CredentialsRef, RootSeed, UserPk,
        },
        bitcoin::{Amount, Invoice, Offer},
        command::{
            CreateInvoiceRequest, CreateOfferRequest, GetPaymentRequest,
            PayInvoiceRequest, PayOfferRequest, PaymentSyncSummary,
            UpdatePaymentNoteRequest,
        },
        payment::{Order, PaymentCreatedIndex, PaymentFilter, PaymentStatus},
    },
    wallet::LexeWallet,
};
use lexe_common::or_env::OrEnvExt;
use serde::Serialize;
use tracing::{info, warn};

// --- Top-level args and command enum --- //

/// Clap's default help template, but with `$ ` prefixed to the usage line.
const HELP_TEMPLATE: &str = "\
{before-help}{about-with-newline}
{usage-heading} $ {usage}

{all-args}{after-help}";

#[derive(Parser)]
#[command(
    name = "lexe",
    about = "Lexe CLI - create and control 24/7 online Lightning wallets \
    from the command line.",
    long_about = "\
Lexe CLI - Create and control Lightning wallets from the command line.
   Lexe wallets are self-custodial and always online to receive payments.

Create a new mainnet wallet:
  $ lexe init
  The seedphrase is saved to ~/.lexe/seedphrase.txt and auto-loaded on
  subsequent runs. Be sure to back up your seedphrase to a safe place.

Control a wallet created from the Lexe mobile app:
  Export credentials from the app (Menu > SDK clients), then set via:
    • LEXE_CLIENT_CREDENTIALS     Set in environment or .env file
    • --client-credentials        Pass directly in CLI
    • --client-credentials-path   Path to file with client credentials

Verify your setup:
  $ lexe node-info

Precedence: CLI args > env vars > .env
  `.env` is loaded from the current or any parent directory.",
    help_template = HELP_TEMPLATE,
)]
pub struct LexeArgs {
    #[command(subcommand)]
    command: LexeCommand,

    /// The bitcoin network to use. [default: mainnet]
    //
    // TODO(max): Discrepancy between `LEXE_NETWORK` and our internal `NETWORK`
    #[arg(long, env = "LEXE_NETWORK", value_enum)]
    network: Option<ClapNetwork>,

    /// The client credentials string exported from the Lexe app.
    /// [env: LEXE_CLIENT_CREDENTIALS]
    #[arg(long)]
    client_credentials: Option<String>,

    /// Path to a file containing the client credentials.
    /// [env: LEXE_CLIENT_CREDENTIALS_PATH]
    #[arg(long)]
    client_credentials_path: Option<PathBuf>,

    /// Root seed as a 64-character hex string.
    /// [env: LEXE_ROOT_SEED]
    #[arg(long)]
    root_seed: Option<String>,

    /// Path to a file containing the root seed (hex or mnemonic).
    /// [env: LEXE_ROOT_SEED_PATH]
    #[arg(long)]
    root_seed_path: Option<PathBuf>,

    /// Data directory for persisted state. [default: ~/.lexe]
    /// [env: LEXE_DATA_DIR]
    #[arg(long)]
    lexe_data_dir: Option<PathBuf>,

    /// Log level: error, warn, info, debug, trace. [default: info]
    /// [env: RUST_LOG]
    #[arg(long)]
    rust_log: Option<String>,

    /// Use wallet without local payments persistence
    #[arg(long)]
    without_db: bool,
}

/// Network enum for clap's ValueEnum derive.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ClapNetwork {
    Mainnet,
    Testnet3,
    Regtest,
}

impl From<ClapNetwork> for Network {
    fn from(n: ClapNetwork) -> Self {
        match n {
            ClapNetwork::Mainnet => Network::Mainnet,
            ClapNetwork::Testnet3 => Network::Testnet3,
            ClapNetwork::Regtest => Network::Regtest,
        }
    }
}

#[derive(Subcommand)]
pub enum LexeCommand {
    Init(InitArgs),
    Signup(SignupArgs),
    Provision(ProvisionArgs),
    NodeInfo(NodeInfoArgs),
    CreateInvoice(CreateInvoiceArgs),
    PayInvoice(PayInvoiceArgs),
    CreateOffer(CreateOfferArgs),
    PayOffer(PayOfferArgs),
    GetPayment(GetPaymentArgs),
    WaitForPayment(WaitForPaymentArgs),
    UpdatePaymentNote(UpdatePaymentNoteArgs),
    SyncPayments(SyncPaymentsArgs),
    ListPayments(ListPaymentsArgs),
    ClearPayments(ClearPaymentsArgs),
}

// --- LexeArgs impl --- //

impl LexeArgs {
    /// Populate unset credential args from env vars.
    /// Should only be called if no CLI credentials were provided.
    fn credentials_or_env_mut(&mut self) -> anyhow::Result<()> {
        self.client_credentials
            .or_env_mut("LEXE_CLIENT_CREDENTIALS")?;
        self.client_credentials_path
            .or_env_mut("LEXE_CLIENT_CREDENTIALS_PATH")?;
        self.root_seed.or_env_mut("LEXE_ROOT_SEED")?;
        self.root_seed_path.or_env_mut("LEXE_ROOT_SEED_PATH")?;
        Ok(())
    }

    /// Populate unset non-credential args from env vars.
    fn other_or_env_mut(&mut self) -> anyhow::Result<()> {
        self.lexe_data_dir.or_env_mut("LEXE_DATA_DIR")?;
        // Network is handled via ClapNetwork enum, not direct env parsing here
        self.rust_log.or_env_mut("RUST_LOG")?;
        Ok(())
    }
}

/// Run the CLI with the given args.
pub async fn run(mut lexe_args: LexeArgs) -> anyhow::Result<()> {
    // If credentials were provided via CLI, use those exclusively.
    // Otherwise, fall back to env vars for credentials.
    let credentials_from_cli = lexe_args.client_credentials.is_some()
        || lexe_args.client_credentials_path.is_some()
        || lexe_args.root_seed.is_some()
        || lexe_args.root_seed_path.is_some();
    if !credentials_from_cli {
        lexe_args.credentials_or_env_mut()?;
    }
    lexe_args.other_or_env_mut()?;

    // Init logger here since `RUST_LOG` may be set in `other_or_env_mut`
    lexe::init_logger(lexe_args.rust_log.as_deref().unwrap_or("info"));

    let network = lexe_args
        .network
        .map(Network::from)
        .unwrap_or(Network::Mainnet);
    let env_config = match network {
        Network::Mainnet => WalletEnvConfig::mainnet(),
        Network::Testnet3 => WalletEnvConfig::testnet3(),
        Network::Testnet4 | Network::Signet =>
            return Err(anyhow!("{network} is not supported")),
        Network::Regtest => {
            let gateway_url = std::env::var("DEV_GATEWAY_URL").ok();
            ensure!(
                gateway_url.is_some(),
                "regtest requires DEV_GATEWAY_URL to be set"
            );
            WalletEnvConfig::regtest(false, gateway_url)
        }
    };

    // Show basic parameters and data dir location in logs.
    let lexe_data_dir = lexe_args
        .lexe_data_dir
        .clone()
        .map_or_else(lexe::default_lexe_data_dir, Ok)?;
    info!(%network, ?lexe_data_dir, "Starting Lexe CLI");

    // Init is special: it generates/loads seed and handles its own persistence.
    if let LexeCommand::Init(init_args) = lexe_args.command {
        return init_args
            .run(env_config, lexe_args.lexe_data_dir, lexe_args.without_db)
            .await;
    }

    // All other commands need an authenticated wallet.
    let credentials = helpers::resolve_credentials(
        &env_config,
        &lexe_args.lexe_data_dir,
        credentials_from_cli,
        lexe_args.client_credentials,
        lexe_args.client_credentials_path,
        lexe_args.root_seed,
        lexe_args.root_seed_path,
    )?;

    let wallet = if lexe_args.without_db {
        LexeWallet::without_db(env_config, credentials.as_ref())
            .context("Failed to create wallet (without-db)")?
    } else {
        LexeWallet::load_or_fresh(
            env_config,
            credentials.as_ref(),
            lexe_args.lexe_data_dir,
        )
        .context("Failed to initialize wallet")?
    };

    // TODO(max): Once we have delegated provisioning, we should provision
    // frequently. It doesn't have to be on *every* invocation, but this helps
    // keep users' nodes up to date. Perhaps once a day, and maybe we can
    // persist a `last_provisioned: (semver::Version, TimestampMs)` somewhere in
    // the `LEXE_DATA_DIR`? Be sure to consider the trust assumptions though.
    //
    // Provision the node on every invocation
    // wallet
    //     .provision(credentials.as_ref())
    //     .await
    //     .context("Provision failed")?;

    match lexe_args.command {
        LexeCommand::Init(_) => unreachable!("Handled above"),
        LexeCommand::Signup(a) => a.run(&wallet, &credentials).await,
        LexeCommand::Provision(a) => a.run(&wallet, &credentials).await,
        LexeCommand::NodeInfo(a) => a.run(&wallet).await,
        LexeCommand::CreateInvoice(a) => a.run(&wallet).await,
        LexeCommand::PayInvoice(a) => a.run(&wallet).await,
        LexeCommand::CreateOffer(a) => a.run(&wallet).await,
        LexeCommand::PayOffer(a) => a.run(&wallet).await,
        LexeCommand::GetPayment(a) => a.run(&wallet).await,
        LexeCommand::WaitForPayment(a) => a.run(&wallet).await,
        LexeCommand::UpdatePaymentNote(a) => a.run(&wallet).await,
        LexeCommand::SyncPayments(a) => a.run(&wallet).await,
        LexeCommand::ListPayments(a) => a.run(&wallet),
        LexeCommand::ClearPayments(a) => a.run(&wallet),
    }
}

// --- `init` --- //

#[derive(Parser)]
#[command(
    about = "Create a new Lexe wallet",
    long_about = "Creates a new Lexe wallet.\n\
        \n\
        Generates a fresh seedphrase, persists it to the Lexe data dir, \n\
        registers a wallet with Lexe, and provisions a new user node.\n\
        \n\
        Idempotent: safe to call multiple times.",
    help_template = HELP_TEMPLATE,
)]
pub struct InitArgs {}

impl InitArgs {
    async fn run(
        self,
        env_config: WalletEnvConfig,
        lexe_data_dir: Option<PathBuf>,
        without_db: bool,
    ) -> anyhow::Result<()> {
        let data_dir = lexe_data_dir
            .clone()
            .map_or_else(lexe::default_lexe_data_dir, Ok)?;
        let seed_path = env_config.seedphrase_path(&data_dir);

        // Load existing seedphrase or generate a fresh one.
        let root_seed = match RootSeed::read_from_path(&seed_path)? {
            Some(seed) => seed,
            None => {
                let seed = RootSeed::generate();
                seed.write_to_path(&seed_path)
                    .context("Failed to write seedphrase")?;
                seed
            }
        };

        let credentials = CredentialsRef::from(&root_seed);

        // Build wallet and call signup (includes initial provisioning).
        let wallet = if without_db {
            LexeWallet::without_db(env_config, credentials)
                .context("Failed to create wallet (without-db)")?
        } else {
            LexeWallet::load_or_fresh(env_config, credentials, lexe_data_dir)
                .context("Failed to initialize wallet")?
        };

        wallet
            .signup(&root_seed, None)
            .await
            .context("Signup failed")?;

        println!(
            "Wallet initialized. Seedphrase file: {}",
            seed_path.display()
        );

        Ok(())
    }
}

// --- `signup` --- //

#[derive(Parser)]
#[command(
    about = "Register with Lexe and perform initial provisioning",
    long_about = "Register with Lexe and perform initial provisioning. \n\
        Requires the root seed.\n\
        \n\
        This command exists mostly to support specialized flows. \n\
        `lexe init` is usually what you want instead.\n\
        \n\
        Idempotent: safe to call even if already signed up.",
    help_template = HELP_TEMPLATE,
)]
pub struct SignupArgs {
    /// Partner public key (hex) to earn a share of this wallet's fees
    #[arg(long)]
    partner_pk: Option<String>,
}

impl SignupArgs {
    async fn run(
        self,
        wallet: &LexeWallet,
        credentials: &Credentials,
    ) -> anyhow::Result<()> {
        let Credentials::RootSeed(root_seed) = credentials else {
            return Err(anyhow!(
                "signup requires a root seed. \
                 Use --root-seed / $LEXE_ROOT_SEED \
                 or --root-seed-path / $LEXE_ROOT_SEED_PATH."
            ));
        };

        let partner_pk = self
            .partner_pk
            .map(|s| UserPk::from_str(&s))
            .transpose()
            .context("Invalid partner public key")?;

        wallet
            .signup(root_seed, partner_pk)
            .await
            .context("Signup failed")?;

        println!("Signup complete.");
        Ok(())
    }
}

// --- `provision` --- //

#[derive(Parser)]
#[command(
    about = "Provision wallet to latest enclave releases",
    long_about = "Ensure the wallet is provisioned to all recent trusted releases.\n\
        \n\
        Should be called every time the wallet is loaded to ensure the node\n\
        is running the most up-to-date enclave software.\n\
        \n\
        Idempotent: safe to call multiple times.",
    help_template = HELP_TEMPLATE,
)]
pub struct ProvisionArgs {}

impl ProvisionArgs {
    async fn run(
        self,
        wallet: &LexeWallet,
        credentials: &Credentials,
    ) -> anyhow::Result<()> {
        wallet
            .provision(credentials.as_ref())
            .await
            .context("Provision failed")?;

        println!("Provision complete.");
        Ok(())
    }
}

// --- `node-info` --- //

#[derive(Parser)]
#[command(about = "Get information about this Lexe node", help_template = HELP_TEMPLATE)]
pub struct NodeInfoArgs {}

impl NodeInfoArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let info = wallet
            .node_info()
            .await
            .context("Failed to get node info")?;
        helpers::print_json_pretty(&info)
    }
}

// --- `create-invoice` --- //

#[derive(Parser)]
#[command(
    about = "Create a BOLT 11 invoice to receive a Lightning payment",
    help_template = HELP_TEMPLATE,
)]
pub struct CreateInvoiceArgs {
    #[arg(
        long,
        help = "Amount in satoshis.\n\
        Omit for amountless invoice."
    )]
    amount_sats: Option<Amount>,

    #[arg(
        long,
        help = "Description to encode in the invoice.\n\
        Visible to sender when scanned."
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Invoice expiration in seconds.\n\
        [default: 86400 = 1 day]"
    )]
    expiration_secs: Option<u32>,
}

impl CreateInvoiceArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CreateInvoiceRequest {
            expiration_secs: self.expiration_secs,
            amount: self.amount_sats,
            description: self.description,
        };
        let resp = wallet
            .create_invoice(req)
            .await
            .context("Failed to create invoice")?;

        // Sync payments to persist the new invoice locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_json_pretty(&resp)
    }
}

// --- `pay-invoice` --- //

#[derive(Parser)]
#[command(about = "Pay a BOLT 11 invoice over Lightning", help_template = HELP_TEMPLATE)]
pub struct PayInvoiceArgs {
    /// The BOLT 11 invoice to pay
    invoice: String,

    #[arg(
        long,
        help = "Amount to pay if invoice has no amount.\n\
        Required for amountless invoices."
    )]
    fallback_amount_sats: Option<Amount>,

    /// Personal note (not visible to receiver, max 200 chars)
    #[arg(long)]
    note: Option<String>,
}

impl PayInvoiceArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let invoice =
            Invoice::from_str(&self.invoice).context("Invalid invoice")?;

        let req = PayInvoiceRequest {
            invoice,
            fallback_amount: self.fallback_amount_sats,
            note: self.note,
        };
        let resp = wallet.pay_invoice(req).await.inspect(|_| {
            // Provide some successful confirmation for CLI users;
            // stdout only prints the `PayInvoiceResponse` with no status.
            info!("Invoice paid!");
        })?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_json_pretty(&resp)
    }
}

// --- `create-offer` --- //

#[derive(Parser)]
#[command(
    about = "Create a BOLT 12 offer to receive Lightning payments",
    long_about = "Create a BOLT 12 offer to receive Lightning payments.\n\
        \n\
        Unlike invoices, offers are reusable: multiple payments can be\n\
        made to it, including from multiple payers.",
    help_template = HELP_TEMPLATE,
)]
pub struct CreateOfferArgs {
    #[arg(
        long,
        help = "Description shown to payers when they scan the offer.\n\
            Maximum length: 512 UTF-8 bytes."
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Minimum payment amount in satoshis.\n\
            If not set, the payer can send any amount."
    )]
    min_amount_sats: Option<Amount>,

    /// Offer expiration in seconds from now
    #[arg(long)]
    expiration_secs: Option<u32>,
}

impl CreateOfferArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CreateOfferRequest {
            description: self.description,
            min_amount: self.min_amount_sats,
            expiration_secs: self.expiration_secs,
        };
        let resp = wallet.create_offer(req).await?;

        helpers::print_json_pretty(&resp)
    }
}

// --- `pay-offer` --- //

#[derive(Parser)]
#[command(about = "Pay a BOLT 12 offer over Lightning", help_template = HELP_TEMPLATE)]
pub struct PayOfferArgs {
    /// The BOLT 12 offer to pay
    offer: String,

    #[arg(
        long,
        help = "The amount to pay in satoshis.\n\
            Must satisfy the offer's minimum amount if set."
    )]
    amount_sats: Amount,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to receiver.\n\
            Maximum length: 512 UTF-8 bytes."
    )]
    note: Option<String>,

    /// Note sent to the receiver with the payment.
    ///
    /// Maximum length: 512 UTF-8 bytes.
    #[arg(long)]
    payer_note: Option<String>,
}

impl PayOfferArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let offer = Offer::from_str(&self.offer).context("Invalid offer")?;

        let req = PayOfferRequest {
            offer,
            amount: self.amount_sats,
            note: self.note,
            payer_note: self.payer_note,
        };
        let resp = wallet.pay_offer(req).await.inspect(|_| {
            info!("Offer paid!");
        })?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_json_pretty(&resp)
    }
}

// --- `get-payment` --- //

#[derive(Parser)]
#[command(
    about = "Get a payment by its created index",
    long_about = "Get a payment by its created index.\n\
        \n\
        Fetches the payment directly from the user node (not from local storage).",
    help_template = HELP_TEMPLATE,
)]
pub struct GetPaymentArgs {
    /// The payment's created index
    index: PaymentCreatedIndex,
}

impl GetPaymentArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let resp = wallet
            .get_payment(GetPaymentRequest { index: self.index })
            .await
            .context("Failed to get payment")?;
        helpers::print_json_pretty(&resp)
    }
}

// --- `wait-for-payment` --- //

#[derive(Parser)]
#[command(
    about = "Wait for a payment to reach a terminal state",
    long_about = "Wait for a payment to reach a terminal state (completed or failed).\n\
        \n\
        Polls the node with exponential backoff until the payment finalizes\n\
        or the timeout is reached. Defaults to 600 seconds (10 minutes).\n\
        \n\
        If already finalized, we still fetch the payment\n\
        to ensure we have the latest metadata.",
    help_template = HELP_TEMPLATE,
)]
pub struct WaitForPaymentArgs {
    /// The payment index to wait on
    index: PaymentCreatedIndex,

    /// Timeout in seconds. [default: 600, max: 86400]
    #[arg(long)]
    timeout_secs: Option<u64>,
}

impl WaitForPaymentArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let timeout = self.timeout_secs.map(Duration::from_secs);
        info!("Waiting for payment...");
        let payment = wallet
            .wait_for_payment(self.index, timeout)
            .await
            .context("Failed waiting for payment")?;
        match payment.status {
            PaymentStatus::Pending =>
                unreachable!("wait_for_payment should only return finalized"),
            PaymentStatus::Completed => info!("Payment complete!"),
            PaymentStatus::Failed => warn!("Payment failed."),
        }
        helpers::print_json_pretty(&payment)
    }
}

// --- `update-payment-note` --- //

#[derive(Parser)]
#[command(
    about = "Update the personal note on an existing payment",
    long_about = "Update the personal note on an existing payment.\n\
        \n\
        The note is stored on the user node and is not visible to the counterparty.",
    help_template = HELP_TEMPLATE,
)]
pub struct UpdatePaymentNoteArgs {
    /// The payment index
    index: PaymentCreatedIndex,

    /// The new note (omit to clear, max 200 chars)
    #[arg(long)]
    note: Option<String>,
}

impl UpdatePaymentNoteArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = UpdatePaymentNoteRequest {
            index: self.index,
            note: self.note,
        };
        wallet
            .update_payment_note(req)
            .await
            .context("Failed to update payment note")?;
        println!("Payment note updated");
        Ok(())
    }
}

// --- `sync-payments` --- //

#[derive(Parser)]
#[command(
    about = "Sync payments from the node to the local payments cache",
    long_about = "Sync payments from the user node to the local payments cache.",
    help_template = HELP_TEMPLATE,
)]
pub struct SyncPaymentsArgs {}

impl SyncPaymentsArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let summary = wallet
            .sync_payments()
            .await
            .context("Payment sync failed")?;
        let PaymentSyncSummary {
            num_new,
            num_updated,
        } = summary;
        println!(
            "Local payments cache synced to user node data: \
             {num_new} new, {num_updated} updated",
        );
        Ok(())
    }
}

// --- `list-payments` --- //

#[derive(Parser)]
#[command(
    about = "List payments from local storage",
    long_about = "List payments from local storage with cursor-based pagination.\n\
        \n\
        Defaults to descending order (newest first) with a limit of 100.\n\
        Use `sync-payments` to fetch the latest data from the node first.",
    help_template = HELP_TEMPLATE,
)]
pub struct ListPaymentsArgs {
    /// Filter: all, pending, completed, failed, finalized. [default: all]
    #[arg(long, default_value = "all")]
    filter: String,

    /// Sort order: asc or desc. [default: desc]
    #[arg(long)]
    order: Option<String>,

    /// Max number of payments to return. [default: 100]
    #[arg(long)]
    limit: Option<usize>,

    /// Start after this payment index (for pagination)
    #[arg(long)]
    after: Option<PaymentCreatedIndex>,
}

impl ListPaymentsArgs {
    fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let filter = match self.filter.as_str() {
            "all" => PaymentFilter::All,
            "pending" => PaymentFilter::Pending,
            "completed" => PaymentFilter::Completed,
            "failed" => PaymentFilter::Failed,
            "finalized" => PaymentFilter::Finalized,
            other =>
                return Err(anyhow!(
                    "Invalid filter: '{other}'. \
                     Valid: 'all', 'pending', 'completed', 'failed', 'finalized'"
                )),
        };

        let order = match self.order.as_deref() {
            None | Some("desc") => Some(Order::Desc),
            Some("asc") => Some(Order::Asc),
            Some(other) =>
                return Err(anyhow!(
                    "Invalid order: '{other}'. Valid: 'asc', 'desc'"
                )),
        };

        let resp = wallet
            .list_payments(&filter, order, self.limit, self.after.as_ref())
            .context("Failed to list payments")?;
        helpers::print_json_pretty(&resp)
    }
}

// --- `clear-payments` --- //

#[derive(Parser)]
#[command(
    about = "Clear all locally cached payment data for this wallet",
    long_about = "Clear all locally cached payment data for this wallet.\n\
        \n\
        Remote data on the node is not affected.\n\
        Use `sync-payments` to re-populate after clearing.",
    help_template = HELP_TEMPLATE,
)]
pub struct ClearPaymentsArgs {}

impl ClearPaymentsArgs {
    fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        wallet
            .clear_payments()
            .context("Failed to clear payments")?;
        println!("Cleared local payments cache.");
        Ok(())
    }
}

/// Credential resolution and output formatting.
mod helpers {
    use super::*;

    /// Print a value as pretty JSON to stdout.
    pub fn print_json_pretty(value: &impl Serialize) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(value)
            .context("Failed to serialize to JSON")?;
        println!("{json}");
        Ok(())
    }

    /// Resolve credentials from the provided args or the seedphrase file.
    ///
    /// At most one credential source may be specified. If none is provided,
    /// falls back to the seedphrase file in the data directory.
    pub fn resolve_credentials(
        env_config: &WalletEnvConfig,
        lexe_data_dir: &Option<PathBuf>,
        credentials_from_cli: bool,
        client_credentials: Option<String>,
        client_credentials_path: Option<PathBuf>,
        root_seed: Option<String>,
        root_seed_path: Option<PathBuf>,
    ) -> anyhow::Result<Credentials> {
        // Count how many credential sources were provided.
        let num_sources = [
            client_credentials.is_some(),
            client_credentials_path.is_some(),
            root_seed.is_some(),
            root_seed_path.is_some(),
        ]
        .into_iter()
        .filter(|&b| b)
        .count();

        ensure!(
            num_sources <= 1,
            "Multiple credential sources specified. Provide only one of:\n\
             \t--client-credentials / $LEXE_CLIENT_CREDENTIALS\n\
             \t--client-credentials-path / $LEXE_CLIENT_CREDENTIALS_PATH\n\
             \t--root-seed / $LEXE_ROOT_SEED\n\
             \t--root-seed-path / $LEXE_ROOT_SEED_PATH"
        );

        // Use the provided credential source, or fall back to seedphrase file.
        let direct_from_cli_or_env = if credentials_from_cli {
            Cow::Borrowed("CLI")
        } else {
            Cow::Borrowed("env")
        };
        let (credentials, source) = if let Some(s) = client_credentials {
            let cc = ClientCredentials::from_string(&s)
                .context("Invalid client credentials")?;
            (Credentials::from(cc), direct_from_cli_or_env)
        } else if let Some(path) = &client_credentials_path {
            let contents =
                std::fs::read_to_string(path).with_context(|| {
                    format!("Failed to read {}", path.display())
                })?;
            let cc = ClientCredentials::from_string(contents.trim())
                .context("Failed to parse client credentials file")?;
            let source = Cow::Owned(path.display().to_string());
            (Credentials::from(cc), source)
        } else if let Some(s) = root_seed {
            let seed = RootSeed::from_str(&s).context("Invalid root seed")?;
            (Credentials::from(seed), direct_from_cli_or_env)
        } else if let Some(path) = &root_seed_path {
            let seed = read_root_seed_file(path)?;
            let source = Cow::Owned(path.display().to_string());
            (Credentials::from(seed), source)
        } else {
            let data_dir = lexe_data_dir
                .clone()
                .map_or_else(lexe::default_lexe_data_dir, Ok)?;
            let seed_path = env_config.wallet_env.seedphrase_path(&data_dir);
            let seed =
                    RootSeed::read_from_path(&seed_path)?.ok_or_else(|| {
                        anyhow!(
                            "No credentials found. Either:\n  \
                         - Run `lexe init` first, or\n  \
                         - Provide --root-seed / $LEXE_ROOT_SEED, or\n  \
                         - Provide --client-credentials / $LEXE_CLIENT_CREDENTIALS"
                        )
                    })?;
            let source = Cow::Owned(seed_path.display().to_string());
            (Credentials::from(seed), source)
        };

        // Log the source of the credentials used, to help users debug if
        // multiple sources were provided. (I (Max) ran into this myself).
        let cred_type = match &credentials {
            Credentials::RootSeed(_) => "root seed",
            Credentials::ClientCredentials(_) => "client credentials",
        };
        info!("Using {cred_type} from {source}.");

        // TODO(max): Maybe log the user_pk too, to help differentiate between
        // wallets? It's a bit long though, petname might be better.
        // let user_pk = credentials.as_ref().user_pk().context(
        //     "Client credentials are out of date. \
        //     Please create a new one from within the Lexe wallet app.",
        // )?;
        // info!("Lexe pubkey (user_pk): {user_pk}");

        Ok(credentials)
    }

    /// Read a root seed from a file containing either hex or mnemonic.
    fn read_root_seed_file(path: &PathBuf) -> anyhow::Result<RootSeed> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let contents = contents.trim();

        // Try hex first (64 hex chars = 32 bytes).
        if contents.len() == 64
            && contents.chars().all(|c| c.is_ascii_hexdigit())
        {
            return RootSeed::from_hex(contents)
                .context("Failed to parse root seed hex");
        }

        // Fall back to mnemonic.
        let mnemonic = Mnemonic::from_str(contents)
            .map_err(|e| anyhow!("Invalid mnemonic: {e}"))?;
        RootSeed::from_mnemonic(mnemonic).context("Failed to parse mnemonic")
    }
}
