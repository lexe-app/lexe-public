//! `lexe-cli` wraps the Lexe Rust SDK (`lexe` crate) and exposes its methods
//! via a command-line interface.

use std::{borrow::Cow, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow, ensure};
use chrono::DateTime;
use clap::{Parser, Subcommand, ValueEnum};
use lexe::{
    config::{Network, WalletEnvConfig},
    types::{
        auth::{
            ClientCredentials, Credentials, CredentialsRef, RootSeed, UserPk,
        },
        bitcoin::{Amount, Invoice, Offer, PaymentMethod},
        command::{
            AnalyzeRequest, CreateInvoiceRequest, CreateOfferRequest,
            GetPaymentRequest, GetUpdatedPaymentsRequest, PayInvoiceRequest,
            PayOfferRequest, PayRequest, PayResponse, PayableDetails,
            PaymentSyncSummary, UpdatePersonalNoteRequest,
        },
        payment::{
            Order, PaymentCreatedIndex, PaymentFilter, PaymentStatus,
            PaymentUpdatedIndex,
        },
        util::Ppm,
    },
    wallet::LexeWallet,
};
use lexe_common::or_env::OrEnvExt;
use serde::Serialize;
use textwrap::wrap;
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

Verify your setup, view balance:
  $ lexe node-info

Receive:
  $ lexe create-invoice
  $ lexe wait-for-payment <index> # 0000001778115215123-ln_e1f8e...

Send:
  $ lexe pay-invoice <invoice> # lnbc10u1p5...

List payments:
  $ lexe sync-payments # Sync payments to local storage
  $ lexe list-payments # List payments in local storage

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
    Analyze(AnalyzeArgs),
    Pay(PayArgs),
    CreateInvoice(CreateInvoiceArgs),
    PayInvoice(PayInvoiceArgs),
    CreateOffer(CreateOfferArgs),
    PayOffer(PayOfferArgs),
    GetPayment(GetPaymentArgs),
    GetUpdatedPayments(GetUpdatedPaymentsArgs),
    WaitForPayment(WaitForPaymentArgs),
    UpdatePersonalNote(UpdatePersonalNoteArgs),
    SyncPayments(SyncPaymentsArgs),
    ListPayments(ListPaymentsArgs),
    ClearPayments(ClearPaymentsArgs),
    Export(ExportArgs),
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
        LexeCommand::Analyze(a) => a.run(&wallet).await,
        LexeCommand::Pay(a) => a.run(&wallet).await,
        LexeCommand::CreateInvoice(a) => a.run(&wallet).await,
        LexeCommand::PayInvoice(a) => a.run(&wallet).await,
        LexeCommand::CreateOffer(a) => a.run(&wallet).await,
        LexeCommand::PayOffer(a) => a.run(&wallet).await,
        LexeCommand::GetPayment(a) => a.run(&wallet).await,
        LexeCommand::GetUpdatedPayments(a) => a.run(&wallet).await,
        LexeCommand::WaitForPayment(a) => a.run(&wallet).await,
        LexeCommand::UpdatePersonalNote(a) => a.run(&wallet).await,
        LexeCommand::SyncPayments(a) => a.run(&wallet).await,
        LexeCommand::ListPayments(a) => a.run(&wallet),
        LexeCommand::ClearPayments(a) => a.run(&wallet),
        LexeCommand::Export(a) => a.run(&credentials),
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

// --- `analyze` --- //

#[derive(Parser)]
#[command(
    about = "Get information about a Bitcoin or Lightning payment string",
    long_about = r#"
Get information about a Bitcoin or Lightning payment string and its
constituent payment methods (if any). Returned information includes the
type of payment method used (invoice, offer, onchain, lnurl) and the amount
constraints requested by the receiver.

Also provides the specific payment string <payable> that can be used
to pay via the associated payment method using
  $ lexe pay <payable>
  
Payment methods are returned in order of most to least recommended."#,
    help_template = HELP_TEMPLATE,
)]
pub struct AnalyzeArgs {
    /// Display output as JSON
    #[arg(long)]
    json: bool,

    /// The Bitcoin or Lightning payment string to analyze.
    payable: String,
}

impl AnalyzeArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        /// Indent size to use in output formatting.
        const TAB: &str = "    "; // 4
        /// Text width to use for wrapping.
        const TEXT_WIDTH: usize = 80;

        let req = AnalyzeRequest {
            payable: self.payable,
        };
        let resp = wallet.analyze(req).await?;

        // JSON response
        if self.json {
            let mut json_payables = vec![];
            for p in resp.payables {
                let kind = p.method.kind();
                let command = if p.payable.contains('\'') {
                    format!(
                        "ERR: payable contained a single quote (\'); \
                         failed to generate command string for {}",
                        p.payable
                    )
                } else {
                    format!("lexe pay \'{}\'", p.payable)
                };
                let method_string = match p.method {
                    PaymentMethod::Onchain { address, .. } =>
                        address.to_string(),
                    PaymentMethod::Invoice { invoice, .. } =>
                        invoice.to_string(),
                    PaymentMethod::Offer { offer, .. } => offer.to_string(),
                    PaymentMethod::LnurlPay { lnurl, .. } => lnurl.to_string(),
                };

                let json_payable = serde_json::json!({
                    "command": command,
                    kind: method_string,
                    "kind": kind,
                    "description": p.description,
                    "amount": p.amount,
                    "min_amount": p.min_amount,
                    "max_amount": p.max_amount,
                    "expires_at": p.expires_at
                });
                json_payables.push(json_payable);
            }
            let json_resp = serde_json::json!({ "payables": json_payables });

            return helpers::print_json_pretty(&json_resp);
        }

        // Human-readable response

        let plural = if resp.payables.len() > 1 {
            "s, from most to least recommended"
        } else {
            ""
        };
        println!("Found payable{plural}:");

        // Output list formatting options
        let list_bullet = format!("{TAB}- ");
        let subsequent_indent = format!("{TAB}{TAB}");
        let list_style_options = textwrap::Options::new(TEXT_WIDTH)
            .initial_indent(&list_bullet)
            .subsequent_indent(&subsequent_indent);

        let mut recommended_hint = " (recommended)";
        for payable_details in resp.payables.into_iter() {
            let PayableDetails {
                payable,
                description,
                method,
                amount,
                min_amount,
                max_amount,
                expires_at,
            } = payable_details;
            let method_name = match method {
                PaymentMethod::Onchain { .. } => "On-chain address",
                PaymentMethod::Invoice { .. } => "BOLT11 invoice",
                PaymentMethod::Offer { .. } => "BOLT12 offer",
                PaymentMethod::LnurlPay { .. } => "LNURL-pay",
            };
            let kind = method.kind();
            let method_string = match method {
                PaymentMethod::Onchain { address, .. } => address.to_string(),
                PaymentMethod::Invoice { invoice, .. } => invoice.to_string(),
                PaymentMethod::Offer { offer, .. } => offer.to_string(),
                PaymentMethod::LnurlPay { lnurl, .. } => lnurl,
            };

            let details_list: [Option<String>; 5] = [
                description.map(|d| format!("description: {d}")),
                amount.map(|a| format!("amount: {a} sats")),
                min_amount.map(|a| format!("minimum amount: {a} sats")),
                max_amount.map(|a| format!("maximum amount: {a} sats")),
                expires_at.and_then(|t| {
                    // If conversion fails, time was too large, so just omit
                    helpers::timestamp_ms_pretty(t)
                        .ok()
                        .map(|s| format!("expiration date: {s}"))
                }),
            ];
            let amount_hint = if amount.is_none() {
                "--amount-sats <amount_sats>"
            } else {
                ""
            };

            println!("\n[ {method_name} ]{recommended_hint}");
            // Don't wrap this to keep it copy-paste-able
            println!("{list_bullet}{kind}: {method_string}");
            for line in details_list.into_iter().flatten() {
                let styled_line = wrap(&line, &list_style_options).join("\n");
                println!("{styled_line}");
            }
            println!("\n{TAB}To pay this, run:");
            if payable.contains('\'') {
                println!(
                    "{TAB}ERR: payable contained a single quote (\'); \
                     failed to generate command string for {payable}"
                );
            } else {
                // Don't wrap this to keep it copy/paste-able
                println!("{TAB}$ lexe pay \'{payable}\' {amount_hint}");
            }

            recommended_hint = "";
        }

        anyhow::Ok(())
    }
}

// --- `pay` --- //

#[derive(Parser)]
#[command(
    about = "Pay a Bitcoin or Lightning payment string",
    long_about = r#"
Pay any string which encodes a Bitcoin or Lightning payment method.

If there exist multiple encoded payment methods, one best recommended
payment method will be chosen.

For finer control over how to pay, consider first using
  $ lexe analyze
to resolve the contents of the payable string, then invoking the specific
pay command for the payment method of choice:
  $ lexe pay-offer ...
  $ lexe pay-invoice ...
etc."#,
    help_template = HELP_TEMPLATE
)]
pub struct PayArgs {
    /// Display output as JSON
    #[arg(long)]
    json: bool,

    /// The string to be paid.
    pub payable: String,

    #[arg(
        long,
        help = "Amount in satoshis.\n\
        Optional for payable string with encoded amounts."
    )]
    pub amount_sats: Option<Amount>,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to receiver.\n\
        Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,

    #[arg(
        long,
        help = "Message sent to the receiver with the payment.\n\
        \n\
        Supported only if sending to BOLT 12 offers, HBAs pointing to offers,\n\
        LNURL recipients whose wallets accept LUD-12 comments, and Lightning\n\
        Addresses of wallets that accept LUD-12 comments.\n\
        \n\
        If the payable doesn't support payer notes, this note will be ignored.\n\
        \n\
        Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    message: Option<String>,
}

impl PayArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = PayRequest {
            payable: self.payable,
            amount: self.amount_sats,
            message: self.message,
            personal_note: self.personal_note,
        };
        let resp = wallet.pay(req).await?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        // JSON response
        if self.json {
            info!("Sent payment!");
            return helpers::print_json_pretty(&resp);
        }

        // Human-readable response
        let PayResponse {
            index,
            created_at: _,
        } = resp;
        println!("Sent payment!");
        // Don't wrap this to keep it copy/paste-able
        println!("index: {index}");

        anyhow::Ok(())
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
        help = "Description to encode in invoice.\n\
        Visible to sender when scanned."
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Invoice expiration in seconds.\n\
        [default: 86400 = 1 day]"
    )]
    expiration_secs: Option<u32>,

    #[arg(
        long,
        help = "Partner user_pk for\n\
        partner-set fees. Required for\n\
        partner_prop_fee and\n\
        partner_base_fee to take effect."
    )]
    partner_pk: Option<UserPk>,

    #[arg(
        long,
        help = "Partner proportional fee in ppm.\n\
        Required if partner_pk is set.\n\
        Min: 5000 (0.5%),\n\
        Max: 500000 (50%)"
    )]
    partner_prop_fee: Option<Ppm>,

    #[arg(
        long,
        help = "Partner base fee in satoshis.\n\
        Requires amount_sats\n\
        to also be set."
    )]
    partner_base_fee: Option<Amount>,

    /// Don't render the QR code
    //
    // Skips QR rendering, which may be useful for agents trying to limit
    // token usage.
    #[arg(long)]
    no_qr: bool,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl CreateInvoiceArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CreateInvoiceRequest {
            expiration_secs: self.expiration_secs,
            amount: self.amount_sats,
            description: self.description,
            partner_pk: self.partner_pk,
            partner_prop_fee: self.partner_prop_fee,
            partner_base_fee: self.partner_base_fee,
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

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        // Human-readable response: the invoice string, the payment index
        // (usable with `wait-for-payment` / `get-payment`), and a QR code.
        let invoice = resp.invoice;

        println!("\nInvoice:\n");
        // Don't wrap this to keep it copy/paste-able
        println!("{invoice}");
        println!(
            "\nPayment index (can be used with `lexe wait-for-payment` or \
             `lexe get-payment`):\n"
        );
        println!("{}", resp.index);

        if !self.no_qr {
            // Encode the QR as a `lightning:` URI, matching how the Lexe app
            // encodes BOLT11 invoices (see `PaymentOffer.uri` in the app).
            let qr = lexe_qr::encode_unicode(
                format!("lightning:{invoice}").into_bytes(),
            )
            .context("Failed to encode invoice as QR code")?;
            println!("\nScan this QR code to pay the invoice:\n");
            println!("{qr}\n");
        }

        anyhow::Ok(())
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

    #[arg(
        long,
        help = "Personal note stored locally, not visible to receiver.\n\
        Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,
}

impl PayInvoiceArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let invoice =
            Invoice::from_str(&self.invoice).context("Invalid invoice")?;

        let req = PayInvoiceRequest {
            invoice,
            fallback_amount: self.fallback_amount_sats,
            personal_note: self.personal_note,
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

    /// Don't render the QR code
    //
    // Skips QR rendering, which may be useful for agents trying to limit
    // token usage.
    #[arg(long)]
    no_qr: bool,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl CreateOfferArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CreateOfferRequest {
            description: self.description,
            min_amount: self.min_amount_sats,
            expiration_secs: self.expiration_secs,
        };
        let resp = wallet.create_offer(req).await?;

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        // Human-readable response: the offer string, the offer ID (which later
        // shows up in `Payment::offer_id` on payments to this offer), and a QR
        // code of the offer.
        let offer = resp.offer;

        println!("\nOffer:\n");
        // Don't wrap this to keep it copy/paste-able
        println!("{offer}");
        let offer_id = offer.id();
        println!("\nOffer ID: {offer_id}");
        println!(
            "\nPayments to this offer will have this ID in their `offer_id` \
             field."
        );

        if !self.no_qr {
            // Encode the QR as a `bitcoin:?lno=<offer>` URI, matching how the
            // Lexe app encodes BOLT12 offers (see `PaymentOffer.uri` in app).
            let qr = lexe_qr::encode_unicode(
                format!("bitcoin:?lno={offer}").into_bytes(),
            )
            .context("Failed to encode offer as QR code")?;
            println!("\nScan this QR code to pay the offer:\n");
            println!("{qr}\n");
        }

        anyhow::Ok(())
    }
}

// --- `pay-offer` --- //

#[derive(Parser)]
#[command(
    about = "Pay a BOLT 12 offer over Lightning",
    help_template = HELP_TEMPLATE,
    // We must set this otherwise help text width exceeds 80 chars
    next_line_help = true,
)]
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
        help = "Message sent to the receiver with the payment.\n\
            \n\
            Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    message: Option<String>,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to receiver.\n\
            Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,
}

impl PayOfferArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let offer = Offer::from_str(&self.offer).context("Invalid offer")?;

        let req = PayOfferRequest {
            offer,
            amount: self.amount_sats,
            message: self.message,
            personal_note: self.personal_note,
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

// --- `get-updated-payments` --- //

#[derive(Parser)]
#[command(
    about = "Get payments which were updated past a specified index",
    long_about = "Get payments which were updated past a specified index.\n\
        \n\
        Fetches updated payments directly from the user node \
        (not from local storage).",
    help_template = HELP_TEMPLATE,
)]
pub struct GetUpdatedPaymentsArgs {
    #[arg(
        long,
        help = "The cursor at which the results should start, exclusive.\n\
        If given, payments that were last updated earlier than or\n\
        equal to this will not be returned. If omitted, the least\n\
        recently updated payments will be returned first."
    )]
    start_index: Option<PaymentUpdatedIndex>,

    #[arg(
        long,
        help = "Maximum number of updated payments to return.\n\
        Maximum value: 100. Defaults to 50 if not set."
    )]
    limit: Option<u16>,
}

impl GetUpdatedPaymentsArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = GetUpdatedPaymentsRequest {
            start_index: self.start_index,
            limit: self.limit,
        };
        let resp = wallet
            .get_updated_payments(req)
            .await
            .context("Failed to get updated payments")?;
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

// --- `update-personal-note` --- //

#[derive(Parser)]
#[command(
    about = "Update the personal note on an existing payment",
    long_about = "Update the personal note on an existing payment.\n\
        \n\
        The note is stored on the user node and is not visible to the counterparty.",
    help_template = HELP_TEMPLATE,
)]
pub struct UpdatePersonalNoteArgs {
    /// The payment index
    index: PaymentCreatedIndex,

    #[arg(
        long,
        help = "The new personal note. Omit to clear.\n\
        Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,
}

impl UpdatePersonalNoteArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = UpdatePersonalNoteRequest {
            index: self.index,
            personal_note: self.personal_note,
        };
        wallet
            .update_personal_note(req)
            .await
            .context("Failed to update personal note")?;
        println!("Personal note updated");
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
        helpers::print_json_pretty(&resp)?;

        // Some users were confused that list-payments doesn't sync first.
        // Since list-payments output can be very long, print a hint *after*
        // the payments display.
        let latest_updated_index = wallet
            .payments_db()
            .and_then(|db| db.latest_updated_index());
        match latest_updated_index {
            Some(index) => {
                let relative = helpers::timestamp_ms_relative(index.updated_at);
                info!(
                    "Hint: Last payments update was {relative}. \
                     You may want to run $ lexe sync-payments first to see \
                     updates later than this."
                );
            }
            None =>
                info!("Note: You may need to run $ lexe sync-payments first."),
        }

        Ok(())
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

// --- `export` --- //

#[derive(Parser)]
#[command(
    about = "Export this wallet's seedphrase as 24 words",
    long_about = "Export this wallet's BIP39 seedphrase.\n\
        \n\
        Prints the 24-word mnemonic, suitable for importing the wallet into\n\
        the Lexe mobile app. Pass --qr to also print a QR code encoding the\n\
        same mnemonic.\n\
        \n\
        Requires the root seed (not just client credentials).\n\
        \n\
        WARNING: Anyone with the mnemonic or QR code can take full control of\n\
        this wallet's funds. Only display this in a private location and store\n\
        the backup somewhere safe.",
    help_template = HELP_TEMPLATE,
)]
pub struct ExportArgs {
    /// Also print a QR code encoding the seedphrase.
    #[arg(long)]
    qr: bool,
}

impl ExportArgs {
    fn run(self, credentials: &Credentials) -> anyhow::Result<()> {
        let Credentials::RootSeed(root_seed) = credentials else {
            return Err(anyhow!(
                "export requires a root seed. \
                 Use --root-seed / $LEXE_ROOT_SEED \
                 or --root-seed-path / $LEXE_ROOT_SEED_PATH."
            ));
        };

        let mnemonic = root_seed.to_mnemonic();

        println!("Seedphrase (24 words):\n");
        println!("```");
        println!("{mnemonic}");
        println!("```");

        println!(
            "\nWARNING: Anyone with this seedphrase can spend your wallet's \
             funds."
        );

        println!(
            "\nTo import this wallet into the Lexe mobile app, go to \
             Restore wallet > Restore from Seed Phrase and enter the words \
             above."
        );

        // We don't display the QR code by default because we don't yet have a
        // way to import a seedphrase into the app via scan.
        //
        // TODO(max): Implement "Restore from QR code" in the app, remove --qr
        if self.qr {
            let qr = lexe_qr::encode_unicode(mnemonic.to_string().into_bytes())
                .context("Failed to encode mnemonic as QR code")?;

            println!(
                "\nAlternatively, scan this QR code to import your Lexe seed \
                 into another wallet:\n"
            );
            println!("{qr}\n");
        }

        Ok(())
    }
}

/// Credential resolution and output formatting.
mod helpers {
    use lexe_common::time::TimestampMs;

    use super::*;

    /// Print a value as pretty JSON to stdout.
    pub fn print_json_pretty(value: &impl Serialize) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(value)
            .context("Failed to serialize to JSON")?;
        println!("{json}");
        Ok(())
    }

    /// Convert a [`TimestampMs`] to a formatted string
    /// Loses millisecond precision (only displays up to seconds)
    /// If conversion fails, timestamp was too big
    pub fn timestamp_ms_pretty(
        timestamp: TimestampMs,
    ) -> anyhow::Result<String> {
        i64::try_from(timestamp.to_millis())
            .ok()
            .and_then(DateTime::from_timestamp_millis)
            .map(|dt| dt.with_timezone(&chrono::Local))
            // 2026 December 01 22:49:32 UTC-08:30
            .map(|dt| dt.format("%Y %B %d %T UTC%:z").to_string())
            .context("Failed to convert timestamp to display string")
    }

    /// Format a past [`TimestampMs`] relative to now, e.g. "just now",
    /// "5 minutes ago", "3 hours ago", "143 weeks ago".
    ///
    /// Picks the largest whole unit that fits. Granularity tops out at weeks
    /// (no months/years), so old timestamps still read as "N weeks ago".
    pub fn timestamp_ms_relative(timestamp: TimestampMs) -> String {
        const MINUTE: u64 = 60;
        const HOUR: u64 = 60 * MINUTE;
        const DAY: u64 = 24 * HOUR;
        const WEEK: u64 = 7 * DAY;

        // `absolute_diff` so a slightly-future timestamp (clock drift) still
        // produces a sane "just now" rather than underflowing.
        let secs = TimestampMs::now().absolute_diff(timestamp).as_secs();

        let (count, unit) = match secs {
            0 => return "just now".to_owned(),
            s if s < MINUTE => (s, "second"),
            s if s < HOUR => (s / MINUTE, "minute"),
            s if s < DAY => (s / HOUR, "hour"),
            s if s < WEEK => (s / DAY, "day"),
            s => (s / WEEK, "week"),
        };

        let plural = if count == 1 { "" } else { "s" };
        format!("{count} {unit}{plural} ago")
    }

    /// Resolve credentials from the provided args or the seedphrase file.
    ///
    /// At most one credential source may be specified. If none is provided,
    /// falls back to the seedphrase file in the data directory.
    //
    // NOTE: Keep in sync with `resolve_credentials` in
    //       `public/sdk-sidecar/src/run.rs`.
    // NOTE: `credentials_from_cli` isn't included in the sidecar due to the
    //       sidecar's CLI and library being separate
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
            let seed = RootSeed::read_from_path_either(path.as_path())?;
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
}
