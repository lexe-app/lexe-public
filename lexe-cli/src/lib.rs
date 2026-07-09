//! `lexe-cli` wraps the Lexe Rust SDK (`lexe` crate) and exposes its methods
//! via a command-line interface.

use std::{borrow::Cow, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use lexe::{
    config::{Network, WalletEnvConfig},
    types::{
        auth::{
            ClientCredentials, Credentials, CredentialsRef, RootSeed, UserPk,
        },
        bitcoin::{
            Amount, ChannelId, ClaimMethod, Invoice, Offer, PaymentMethod,
            UserChannelId,
        },
        command::{
            AnalyzeRequest, AnalyzeResponse, CashAppBuyRequest, ChannelDetails,
            ClientInfo, CloseChannelRequest, CreateClientRequest,
            CreateInvoiceRequest, CreateOfferRequest, GetPaymentRequest,
            GetUpdatedPaymentsRequest, OpenChannelRequest, PayInvoiceRequest,
            PayLnurlRequest, PayOfferRequest, PayRequest, PaymentSyncSummary,
            RevokeClientRequest, UpdateClientRequest,
            UpdatePersonalNoteRequest, WithdrawLnurlRequest,
        },
        payment::{
            Order, Payment, PaymentCreatedIndex, PaymentFilter, PaymentStatus,
            PaymentUpdatedIndex,
        },
        util::{Ppm, TimestampMs},
    },
    util::ed25519,
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
    PayLnurl(PayLnurlArgs),
    WithdrawLnurl(WithdrawLnurlArgs),
    BuyWithCashApp(BuyWithCashAppArgs),
    SyncPayments(SyncPaymentsArgs),
    ListPayments(ListPaymentsArgs),
    ClearPayments(ClearPaymentsArgs),
    WaitForPayment(WaitForPaymentArgs),
    GetPayment(GetPaymentArgs),
    GetUpdatedPayments(GetUpdatedPaymentsArgs),
    UpdatePersonalNote(UpdatePersonalNoteArgs),
    ListClients(ListClientsArgs),
    CreateClient(CreateClientArgs),
    UpdateClient(UpdateClientArgs),
    RevokeClient(RevokeClientArgs),
    ListChannels(ListChannelsArgs),
    OpenChannel(OpenChannelArgs),
    CloseChannel(CloseChannelArgs),
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
        LexeCommand::PayLnurl(a) => a.run(&wallet).await,
        LexeCommand::WithdrawLnurl(a) => a.run(&wallet).await,
        LexeCommand::BuyWithCashApp(a) => a.run(&wallet).await,
        LexeCommand::SyncPayments(a) => a.run(&wallet).await,
        LexeCommand::ListPayments(a) => a.run(&wallet),
        LexeCommand::ClearPayments(a) => a.run(&wallet),
        LexeCommand::WaitForPayment(a) => a.run(&wallet).await,
        LexeCommand::GetPayment(a) => a.run(&wallet).await,
        LexeCommand::GetUpdatedPayments(a) => a.run(&wallet).await,
        LexeCommand::UpdatePersonalNote(a) => a.run(&wallet).await,
        LexeCommand::ListClients(a) => a.run(&wallet).await,
        LexeCommand::CreateClient(a) => a.run(&wallet).await,
        LexeCommand::UpdateClient(a) => a.run(&wallet).await,
        LexeCommand::RevokeClient(a) => a.run(&wallet).await,
        LexeCommand::ListChannels(a) => a.run(&wallet).await,
        LexeCommand::OpenChannel(a) => a.run(&wallet).await,
        LexeCommand::CloseChannel(a) => a.run(&wallet).await,
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
constituent payment and claim methods (if any). Returned information includes
the type of method used (invoice, offer, onchain, lnurl-pay, lnurl-withdraw)
and the amount constraints requested by the counterparty.

Analysis results include either a `payable` or `claimable` field.

Payables (outbound) can be paid using
  $ lexe pay <payable>

Claimables (inbound, currently only LNURL-withdraw) can be claimed using
  $ lexe withdraw-lnurl <claimable>"#,
    help_template = HELP_TEMPLATE,
)]
pub struct AnalyzeArgs {
    /// Display output as JSON
    #[arg(long)]
    json: bool,

    /// Don't render the QR code
    #[arg(long)]
    no_qr: bool,

    /// Only show payables (outbound payment methods).
    #[arg(long, conflicts_with = "claimables_only")]
    payables_only: bool,

    /// Only show claimables (inbound payment methods).
    #[arg(long)]
    claimables_only: bool,

    /// The Bitcoin or Lightning payment string to analyze.
    payment_string: String,
}

impl AnalyzeArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = AnalyzeRequest {
            payment_string: self.payment_string,
        };
        let resp = wallet.analyze(req).await?;

        if self.json {
            // JSON response
            Self::print_json(self.payables_only, self.claimables_only, resp)
        } else {
            // Human-readable response
            Self::print_human_readable(
                self.no_qr,
                self.payables_only,
                self.claimables_only,
                resp,
            )
        }
    }

    fn print_json(
        payables_only: bool,
        claimables_only: bool,
        resp: AnalyzeResponse,
    ) -> anyhow::Result<()> {
        let mut json_resp = serde_json::Map::new();
        if !claimables_only {
            let mut json_payables = vec![];
            for p in resp.payables {
                let kind = p.method.kind();
                let command = Self::validate_command("pay", &p.payable, "")?;
                let method_string = match p.method {
                    PaymentMethod::Onchain { address, .. } =>
                        address.to_string(),
                    PaymentMethod::Invoice { invoice, .. } =>
                        invoice.to_string(),
                    PaymentMethod::Offer { offer, .. } => offer.to_string(),
                    PaymentMethod::LnurlPay { lnurl, .. } => lnurl,
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
            json_resp.insert(
                "payables".to_string(),
                serde_json::Value::Array(json_payables),
            );
        }

        if !payables_only {
            let mut json_claimables = vec![];
            for c in resp.claimables {
                let kind = c.method.kind();
                // TODO(nicole): don't forget to change when `claim` comes thru
                let command =
                    Self::validate_command("withdraw-lnurl", &c.claimable, "")?;
                let method_string = match c.method {
                    ClaimMethod::LnurlWithdraw { lnurl, .. } => lnurl,
                };
                let json_claimable = serde_json::json!({
                    "command": command,
                    kind: method_string,
                    "kind": kind,
                    "description": c.description,
                    "min_amount": c.min_amount,
                    "max_amount": c.max_amount,
                });
                json_claimables.push(json_claimable);
            }
            json_resp.insert(
                "claimables".to_string(),
                serde_json::Value::Array(json_claimables),
            );
        }

        helpers::print_json_pretty(&json_resp)
    }

    fn print_human_readable(
        no_qr: bool,
        payables_only: bool,
        claimables_only: bool,
        resp: AnalyzeResponse,
    ) -> anyhow::Result<()> {
        /// Indent size to use in output formatting.
        const TAB: &str = "    "; // 4
        /// Text width to use for wrapping.
        const TEXT_WIDTH: usize = 80;

        /// A struct used for printing payable and claimable analyze results
        struct MethodEntry {
            /// Lowercase "claim" or "pay", for example
            verb: &'static str,
            method_name: &'static str,
            kind: &'static str,
            /// The string encoding of the payment method
            method_string: String,
            description: Option<String>,
            amount: Option<Amount>,
            min_amount: Option<Amount>,
            max_amount: Option<Amount>,
            expires_at: Option<TimestampMs>,
            command_arg: String,
            lexe_command: &'static str,
        }

        // Output list formatting options
        let list_bullet = format!("{TAB}- ");
        let subsequent_indent = format!("{TAB}{TAB}");
        let list_style_options = textwrap::Options::new(TEXT_WIDTH)
            .initial_indent(&list_bullet)
            .subsequent_indent(&subsequent_indent);

        // `MethodEntry` print function
        let print_entry = |entry: &MethodEntry, recommended, print_qr| {
            let MethodEntry {
                verb,
                method_name,
                kind,
                method_string,
                description,
                amount,
                min_amount,
                max_amount,
                expires_at,
                command_arg,
                lexe_command,
            } = entry;
            let details_list = [
                description.as_ref().map(|d| format!("description: {d}")),
                amount.map(|a| format!("amount: {a} sats")),
                min_amount.map(|a| format!("minimum amount: {a} sats")),
                max_amount.map(|a| format!("maximum amount: {a} sats")),
                expires_at.and_then(|t| {
                    Some(format!(
                        "expiration date: {}",
                        helpers::timestamp_to_datetime(t).to_rfc2822()
                    ))
                }),
            ];
            let amount_hint = if amount.is_none() {
                " --amount-sats <amount_sats>"
            } else {
                ""
            };
            let mut command = "$ ".to_string();
            command.push_str(&Self::validate_command(
                lexe_command,
                command_arg,
                amount_hint,
            )?);

            // <QR>
            if print_qr
                && !no_qr
                && let Ok(qr) =
                    lexe_qr::encode_unicode(method_string.bytes().collect())
            {
                println!("\n{qr}\n");
            }

            // [ Offer ] (recommended)
            if recommended {
                println!("\n[ {method_name} ] (recommended)");
            } else {
                println!("\n[ {method_name} ]");
            }

            // - offer: lno1...
            // Don't wrap this to keep it copy/paste-able
            println!("{list_bullet}{kind}: {method_string}");

            // - description: ... (wrapped)
            // - min_amount: 123 sats
            for line in details_list.into_iter().flatten() {
                let styled_line = wrap(&line, &list_style_options).join("\n");
                println!("{styled_line}");
            }

            // To pay this, run:
            // $ lexe pay lno1abracadabra (--amount-sats <amount_sats>)
            println!("\n{TAB}To {verb} this, run:");
            // Don't wrap this to keep it copy/paste-able
            println!("{TAB}{command}");

            anyhow::Ok(())
        };

        // Check if there is only one entry across payables and claimables
        enum OnlyOne {
            Payable,
            Claimable,
            Neither,
        }
        let show_qr = if resp.payables.len() == 1
            && (payables_only || resp.claimables.is_empty())
        {
            OnlyOne::Payable
        } else if resp.claimables.len() == 1
            && (claimables_only || resp.payables.is_empty())
        {
            OnlyOne::Claimable
        } else {
            OnlyOne::Neither
        };

        if !claimables_only {
            // Process payables
            let mut payable_details = Vec::with_capacity(resp.payables.len());
            for details in resp.payables.into_iter() {
                let kind = details.method.kind();
                let (method_name, method_string) = match details.method {
                    PaymentMethod::Onchain { address, .. } =>
                        ("On-chain address", address.to_string()),
                    PaymentMethod::Invoice { invoice, .. } =>
                        ("BOLT11 invoice", invoice.to_string()),
                    PaymentMethod::Offer { offer, .. } =>
                        ("BOLT12 offer", offer.to_string()),
                    PaymentMethod::LnurlPay { lnurl, .. } =>
                        ("LNURL-pay", lnurl),
                };
                let entry = MethodEntry {
                    verb: "pay",
                    method_name,
                    kind,
                    method_string,
                    description: details.description,
                    amount: details.amount,
                    min_amount: details.min_amount,
                    max_amount: details.max_amount,
                    expires_at: details.expires_at,
                    command_arg: details.payable,
                    lexe_command: "pay",
                };
                payable_details.push(entry);
            }

            // Print payables
            match payable_details.len() {
                0 => println!("No payables found."),
                1 => println!("Found 1 payable:"),
                n => println!("Found {n} payables:"),
            }
            let mut details_iter = payable_details.into_iter();
            if let Some(first) = details_iter.next() {
                let only_entry = matches!(show_qr, OnlyOne::Payable);
                // If it's the only one: don't print "recommended"; do print QR
                print_entry(&first, !only_entry, only_entry)?;
            }
            for entry in details_iter {
                print_entry(&entry, false, false)?;
            }
        }

        if !payables_only && !claimables_only {
            println!("\n");
        }

        if !payables_only {
            // Process claimables
            let mut claimable_details =
                Vec::with_capacity(resp.claimables.len());
            for details in resp.claimables.into_iter() {
                let kind = details.method.kind();
                let (method_name, method_string) = match details.method {
                    ClaimMethod::LnurlWithdraw { lnurl, .. } =>
                        ("LNURL-withdraw", lnurl),
                };
                let entry = MethodEntry {
                    verb: "withdraw",
                    method_name,
                    kind,
                    method_string,
                    description: details.description,
                    amount: None,
                    min_amount: details.min_amount,
                    max_amount: details.max_amount,
                    expires_at: None,
                    command_arg: details.claimable,
                    lexe_command: "withdraw-lnurl",
                };
                claimable_details.push(entry);
            }

            // Print claimables
            match claimable_details.len() {
                0 => println!("No claimables found."),
                1 => println!("Found 1 claimable:"),
                n => println!("Found {n} claimables:"),
            }
            let mut details_iter = claimable_details.into_iter();
            if let Some(first) = details_iter.next() {
                let only_entry = matches!(show_qr, OnlyOne::Claimable);
                // If it's the only one: don't print "recommended"; do print QR
                print_entry(&first, !only_entry, only_entry)?;
            }
            for entry in details_iter {
                print_entry(&entry, false, false)?;
            }
        }

        Ok(())
    }

    fn validate_command(
        command: &str,
        arg: &str,
        append: &str,
    ) -> anyhow::Result<String> {
        if arg.contains('\'') {
            Err(anyhow!(
                // Shouldn't happen, but disallow to guard against
                // command injection
                "Found unexpected single quote (') in payment string.
                 Please report this to Lexe. Payment string: {arg}"
            ))
        } else {
            Ok(format!("lexe {command} '{arg}'{append}"))
        }
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
        If the payable doesn't support messages, this message will be ignored.\n\
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
        let payment = wallet.pay(req).await?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_payment(&payment)
    }
}

// --- `create-invoice` --- //

#[derive(Parser)]
#[command(
    about = "Create a BOLT 11 invoice to receive a Lightning payment",
    help_template = HELP_TEMPLATE,
)]
pub struct CreateInvoiceArgs {
    #[arg(long, help = "Amount in satoshis. Omit for amountless invoice.")]
    amount_sats: Option<Amount>,

    #[arg(
        long,
        help = "Description to encode in invoice. Visible to sender when scanned."
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to sender.\n\
        Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,

    #[arg(
        long,
        help = "Invoice expiration in seconds. [default: 86400 = 1 day]"
    )]
    expiration_secs: Option<u32>,

    #[arg(
        long,
        help = "Partner user_pk for partner-set fees. Required for\n\
        partner_prop_fee and partner_base_fee to take effect."
    )]
    partner_pk: Option<UserPk>,

    #[arg(
        long,
        help = "Partner proportional fee in ppm. Required if partner_pk is set.\n\
        Min: 5000 (0.5%),\n\
        Max: 500000 (50%)"
    )]
    partner_prop_fee: Option<Ppm>,

    #[arg(
        long,
        help = "Partner base fee in satoshis. Requires amount_sats to also be set."
    )]
    partner_base_fee: Option<Amount>,

    /// Don't render the QR code
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
            personal_note: self.personal_note,
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
            println!("\nScan this QR code to pay the invoice:");
            // Encode the QR as a `lightning:` URI, matching how the Lexe app
            // encodes BOLT11 invoices (see `PaymentOffer.uri` in the app).
            helpers::encode_and_print_qr(&format!("lightning:{invoice}"))?;
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
        let payment = wallet
            .pay_invoice(req)
            .await
            .context("Failed to pay invoice")?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_payment(&payment)
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
            Maximum length: 200 chars / 512 UTF-8 bytes."
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
            println!("\nScan this QR code to pay the offer:");
            // Encode the QR as a `bitcoin:?lno=<offer>` URI, matching how the
            // Lexe app encodes BOLT12 offers (see `PaymentOffer.uri` in app).
            helpers::encode_and_print_qr(&format!("bitcoin:?lno={offer}"))?;
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
        let payment =
            wallet.pay_offer(req).await.context("Failed to pay offer")?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_payment(&payment)
    }
}

// --- `pay-lnurl` --- //

#[derive(Parser)]
#[command(
    about = "Pay to an LNURL-pay endpoint",
    long_about = r#"
Pay to an LNURL-pay endpoint.

Use `lexe analyze` to get information on amount constraints,
message length limits, and other details of the LNURL-pay endpoint.

Accepted LNURL encodings:
    Lightning Address          user@domain.com
    LUD-17 URI                 lnurlp://...
    bech32                     lnurl1...
    Lightning URI with bech32  lightning:lnurl1..."#,
    help_template = HELP_TEMPLATE,
)]
pub struct PayLnurlArgs {
    /// The LNURL-pay string to pay.
    lnurl: String,

    #[arg(
        long,
        help = "The amount to pay in satoshis.\n\
            Must satisfy the recipient's amount limits if set."
    )]
    amount_sats: Amount,

    #[arg(
        long,
        help = "Message sent to the receiver with the payment.\n\
            \n\
            Sent only if the recipient accepts LUD-12 comments, and\n\
            truncated to their specified length limit if necessary."
    )]
    message: Option<String>,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to receiver.\n\
            Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,
}

impl PayLnurlArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = PayLnurlRequest {
            lnurl: Some(self.lnurl),
            pay_request: None,
            amount: self.amount_sats,
            message: self.message,
            personal_note: self.personal_note,
        };
        let payment =
            wallet.pay_lnurl(req).await.context("Failed to pay LNURL")?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_payment(&payment)
    }
}

// --- `withdraw-lnurl` --- //

#[derive(Parser)]
#[command(
    about = "Withdraw from an LNURL-withdraw endpoint",
    long_about = r#"
Withdraw from an LNURL-withdraw endpoint.

Use `lexe analyze` to get information on amount constraints,
default description, and other details of the LNURL-withdraw endpoint.

Accepted LNURL encodings:
    LUD-17 URI                 lnurlw://...
    bech32                     lnurl1...
    Lightning URI with bech32  lightning:lnurl1..."#,
    help_template = HELP_TEMPLATE,
    // We must set this otherwise help text width exceeds 80 chars
    next_line_help = true,
)]
pub struct WithdrawLnurlArgs {
    /// The LNURL-withdraw string to withdraw from.
    lnurl: String,

    #[arg(
        long,
        help = "The amount to withdraw in satoshis.\n\
            Must satisfy the endpoint's amount limits.\n\
            Defaults to the maximum if not set."
    )]
    amount_sats: Option<Amount>,

    #[arg(
        long,
        help = "Description encoded into the withdrawal invoice,\n\
            visible to the LNURL endpoint.\n\
            Defaults to the endpoint's description if not set."
    )]
    description: Option<String>,

    #[arg(
        long,
        help = "Personal note stored locally, not visible to endpoint.\n\
            Maximum length: 200 chars / 512 UTF-8 bytes."
    )]
    personal_note: Option<String>,
}

impl WithdrawLnurlArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let amount = self.amount_sats;
        let req = WithdrawLnurlRequest {
            lnurl: Some(self.lnurl),
            withdraw_request: None,
            amount,
            description: self.description,
            personal_note: self.personal_note,
        };
        info!("Waiting for withdrawal...");
        let payment = wallet
            .withdraw_lnurl(req)
            .await
            .context("Failed to withdraw LNURL")?;

        // Sync payments to persist the new payment locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        helpers::print_payment(&payment)
    }
}

// --- `buy-with-cash-app` --- //

// Unlike the SDK docs which address a developer whose end user does the buying,
// the CLI user is themselves the buyer, so this speaks to them directly.
#[derive(Parser)]
#[command(
    about = "Buy Bitcoin with Cash App",
    long_about = "Buy Bitcoin with Cash App.\n\
        \n\
        Given an amount of Bitcoin to buy, returns a Cash App URL. Open it to\n\
        complete the purchase in Cash App. Cash App buys are instant and land\n\
        directly into your Lexe wallet.\n\
        \n\
        For the smoothest experience, open this URL on a device where you have\n\
        Cash App already set up.",
    help_template = HELP_TEMPLATE,
)]
pub struct BuyWithCashAppArgs {
    /// Amount to buy in satoshis. Minimum: 5000.
    amount_sats: Amount,

    /// Don't render the QR code
    #[arg(long)]
    no_qr: bool,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl BuyWithCashAppArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CashAppBuyRequest {
            amount: self.amount_sats,
        };
        let resp = wallet
            .buy_with_cash_app(req)
            .await
            .context("Failed to start Cash App buy")?;

        // Sync payments to persist the pending buy locally.
        if wallet.persistence_enabled() {
            wallet
                .sync_payments()
                .await
                .context("Payment sync failed")?;
        }

        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        println!("\nOpen this Cash App URL to complete the buy:\n");
        // Don't wrap this to keep it copy/paste-able
        println!("{}", resp.redirect_url);
        println!(
            "\nPayment index (can be used with `lexe wait-for-payment` or \
             `lexe get-payment`):\n"
        );
        println!("{}", resp.index);

        if !self.no_qr {
            // The URL likely needs to be opened on a phone with Cash App, so a
            // QR code is easier than copying the URL across devices.
            println!("\nScan this QR code to complete the buy:");
            helpers::encode_and_print_qr(&resp.redirect_url)?;
        }

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
        // the payments display. We show `last_synced_at` (when we last
        // contacted the node) rather than the newest payment's `updated_at`,
        // since a recent sync may have simply turned up no new payments.
        let last_synced_at =
            wallet.payments_db().and_then(|db| db.last_synced_at());
        match last_synced_at {
            Some(synced_at) => {
                let relative = helpers::timestamp_ms_relative(synced_at);
                info!(
                    "Hint: Payments were last synced {relative}. \
                     Run $ lexe sync-payments to see updates newer than this."
                );
            }
            None => info!(
                "Note: Run $ lexe sync-payments first to populate local \
                 payments storage."
            ),
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

// --- `list-clients` --- //

#[derive(Parser)]
#[command(
    about = "List the clients authorized to control this node",
    long_about = "List the clients authorized to control this node.\n\
        \n\
        Returns each client's public key, creation time, expiration (if any),\n\
        and label (if any). Revoked and expired clients are not included.",
    help_template = HELP_TEMPLATE,
)]
pub struct ListClientsArgs {
    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl ListClientsArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let resp = wallet
            .list_clients()
            .await
            .context("Failed to list clients")?;

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        // Display oldest first for stable, deterministic output.
        let mut clients = resp.clients.into_values().collect::<Vec<_>>();
        clients.sort_by_key(|client| client.created_at);

        // Print each entry, then the count summary last. A trailing blank line
        // after each entry separates it from the next and from the summary.
        println!();
        for client in &clients {
            helpers::print_client_info(client)?;
            println!();
        }
        match clients.len() {
            0 => println!("No clients found."),
            1 => println!("Found 1 client."),
            n => println!("Found {n} clients."),
        }

        Ok(())
    }
}

// --- `create-client` --- //

#[derive(Parser)]
#[command(
    about = "Create a new client authorized to control this node",
    long_about = "Create a new client and its associated client credentials.\n\
        \n\
        The returned credentials grant control of this node without exposing\n\
        the root seed, and can be revoked at any time with `lexe \
        revoke-client`.\n\
        \n\
        An expiration must be chosen explicitly: pass --expiration-days and/or\n\
        --expiration-secs to set one, or --never-expires to opt out.\n\
        \n\
        WARNING: Anyone with these credentials can control this node's funds.\n\
        Store them somewhere safe.",
        // TODO(nicole): edit the above warning when credential scopes are added
    help_template = HELP_TEMPLATE,
)]
pub struct CreateClientArgs {
    #[arg(
        long,
        help = "Label for the client. Maximum length: 64 UTF-8 bytes."
    )]
    label: Option<String>,

    /// Create a credential that never expires. Use carefully!
    #[arg(long, conflicts_with_all = ["expiration_days", "expiration_secs"])]
    never_expires: bool,

    #[arg(
        long,
        help = "Days until the client expires, counting from now.\n\
        Adds to --expiration-secs."
    )]
    expiration_days: Option<u32>,

    #[arg(
        long,
        help = "Seconds until the client expires, counting from now.\n\
        Adds to --expiration-days."
    )]
    expiration_secs: Option<u32>,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl CreateClientArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let expires_at = if self.never_expires {
            None
        } else {
            let expiration = helpers::expiration_from_now(
                self.expiration_days,
                self.expiration_secs,
            );
            match expiration {
                None =>
                // self.never_expires was false -- force an opt-in
                    return Err(anyhow!(
                        "Choose an expiration: pass --expiration-days and/or \
                         --expiration-secs, or --never-expires to opt out."
                    )),
                Some(e) => Some(e),
            }
        };
        let req = CreateClientRequest {
            expires_at,
            label: self.label.clone(),
        };
        let resp = wallet
            .create_client(req)
            .await
            .context("Failed to create client")?;

        let credentials = resp.client_credentials.export_string();

        // Hand-assembled rather than serializing `resp` so we can also echo
        // `expires_at` and `label`, which aren't on `CreateClientResponse`.
        if self.json {
            let json = serde_json::json!({
                "client_pk": resp.client_pk,
                "client_credentials": credentials,
                "created_at": resp.created_at,
                "expires_at": expires_at,
                "label": self.label,
            });
            return helpers::print_json_pretty(&json);
        }

        let client_info = ClientInfo {
            client_pk: resp.client_pk,
            created_at: resp.created_at,
            expires_at,
            label: self.label,
        };

        // Human-readable response
        println!("\nClient credentials:");
        // Don't wrap this to keep it copy/paste-able
        println!("{credentials}");

        // TODO(nicole): edit when credential scopes are added
        println!(
            "\nWARNING: Anyone with these credentials can control this node's \
             funds. Store them somewhere safe.\n"
        );

        helpers::print_client_info(&client_info)
    }
}

// --- `update-client` --- //

#[derive(Parser)]
#[command(
    about = "Update a client's label or expiration",
    long_about = "Update the label or expiration of an existing client.\n\
        \n\
        Only the provided fields are changed; omitted fields are left as-is.",
    help_template = HELP_TEMPLATE,
)]
pub struct UpdateClientArgs {
    /// The public key of the client to update.
    client_pk: ed25519::PublicKey,

    #[arg(
        long,
        conflicts_with = "clear_label",
        help = "Set the client's label. Maximum length: 64 UTF-8 bytes."
    )]
    label: Option<String>,

    /// Clear the client's label.
    #[arg(long)]
    clear_label: bool,

    /// Clear the client's expiration, so it never expires. Use carefully!
    #[arg(long, conflicts_with_all = ["expiration_days", "expiration_secs"])]
    clear_expiration: bool,

    #[arg(
        long,
        help = "Set the client's expiration in days from now.\n\
        Adds to --expiration-secs."
    )]
    expiration_days: Option<u32>,

    #[arg(
        long,
        help = "Set the client's expiration in seconds from now.\n\
        Adds to --expiration-days."
    )]
    expiration_secs: Option<u32>,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl UpdateClientArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let expires_at = helpers::expiration_from_now(
            self.expiration_days,
            self.expiration_secs,
        );
        let req = UpdateClientRequest::new(
            self.client_pk,
            self.label,
            self.clear_label,
            expires_at,
            self.clear_expiration,
        )?;
        let resp = wallet
            .update_client(req)
            .await
            .context("Failed to update client")?;

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        helpers::print_client_info(&resp.client)
    }
}

// --- `revoke-client` --- //

#[derive(Parser)]
#[command(
    about = "Permanently revoke a client's access to your Lexe wallet",
    long_about = "Permanently revoke a client's access to your Lexe wallet.\n\
        \n\
        This cannot be undone.",
    help_template = HELP_TEMPLATE,
)]
pub struct RevokeClientArgs {
    /// The public key of the client to revoke.
    client_pk: ed25519::PublicKey,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl RevokeClientArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = RevokeClientRequest {
            client_pk: self.client_pk,
        };
        let resp = wallet
            .revoke_client(req)
            .await
            .context("Failed to revoke client")?;

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        println!("Client revoked.");
        println!();
        helpers::print_client_info(&resp.client)
    }
}

// --- `list-channels` --- //

#[derive(Parser)]
#[command(
    about = "List this node's Lightning channels",
    long_about = "List this node's Lightning channels.\n\
        \n\
        All of this node's Lightning channels are connected to the Lexe LSP.",
    help_template = HELP_TEMPLATE,
)]
pub struct ListChannelsArgs {
    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl ListChannelsArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let resp = wallet
            .list_channels()
            .await
            .context("Failed to list channels")?;

        // JSON response
        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        for channel in &resp.channels {
            Self::print_channel_details(channel);
            println!();
        }
        match resp.channels.len() {
            0 => println!("No channels found."),
            1 => println!("Found 1 channel."),
            n => println!("Found {n} channels."),
        }

        Ok(())
    }

    /// Print a [`ChannelDetails`] as a human-readable block.
    // Channel [<channel_id>]
    //     - user channel id: <user_channel_id>
    //     - funding transaction output txid: <txid> index: <index>
    //
    //     - channel value:      1000000 sats
    //     - our balance:         750000 sats
    //     ...
    //
    // UNUSABLE: Channel [<unusable_channel_id>]
    //     ...
    fn print_channel_details(channel: &ChannelDetails) {
        // Static label-column widths, sized to each group's longest label.
        const AMOUNTS_LABEL_WIDTH: usize = "punishment reserve:".len();

        let usable_prefix = if channel.is_usable { "" } else { "UNUSABLE: " };
        let channel_id = &channel.channel_id;
        println!("{usable_prefix}Channel [{channel_id}]");
        println!("    - user channel id: {}", channel.user_channel_id);

        match &channel.funding_txo {
            Some(txo) => {
                println!("    - funding transaction output");
                println!("        txid: {}", txo.txid);
                println!("        index: {}", txo.index);
            }
            None =>
                println!("    - funding transaction output: Not yet confirmed."),
        }

        println!();
        let value_width = Self::print_sat_group(
            &[
                ("channel value:", channel.channel_value),
                ("our balance:", channel.our_balance),
                ("their balance:", channel.their_balance),
                ("punishment reserve:", channel.punishment_reserve),
            ],
            AMOUNTS_LABEL_WIDTH,
            None,
        );

        println!();
        Self::print_sat_group(
            &[
                ("outbound_capacity:", channel.outbound_capacity),
                ("inbound_capacity:", channel.inbound_capacity),
            ],
            AMOUNTS_LABEL_WIDTH,
            Some(value_width),
        );
    }

    /// Print a group of `(label, amount)` rows as `- <label> <sats> sats`,
    ///
    /// `label_width` left-pads the label column (pass the group's longest label
    /// width); the whole-sat values are right-aligned to `value_width`.
    ///
    /// Returns the value width used.
    fn print_sat_group(
        rows: &[(&str, Amount)],
        label_width: usize,
        value_width_override: Option<usize>,
    ) -> usize {
        let values = rows
            .iter()
            .map(|(_, amt)| amt.sats_u64().to_string())
            .collect::<Vec<_>>();
        let value_width = value_width_override.unwrap_or_else(|| {
            values.iter().map(String::len).max().unwrap_or(0)
        });

        for ((label, _), value) in rows.iter().zip(&values) {
            println!("    - {label:<label_width$} {value:>value_width$} sats");
        }
        value_width
    }
}

// --- `open-channel` --- //

#[derive(Parser)]
#[command(
    about = "Open a Lightning channel from this node to Lexe's LSP",
    help_template = HELP_TEMPLATE,
)]
pub struct OpenChannelArgs {
    #[arg(long, help = "The value of the channel to open, in satoshis.")]
    value_sats: Amount,

    #[arg(
        long,
        help = "An optional client-generated channel id for idempotency,\n\
            serialized as a 32-character hex string (16 bytes).\n\
            Retrying with the same id won't open a duplicate channel.\n\
            A random id is generated if not provided."
    )]
    user_channel_id: Option<UserChannelId>,

    /// Display output as JSON
    #[arg(long)]
    json: bool,
}

impl OpenChannelArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = OpenChannelRequest {
            value: self.value_sats,
            user_channel_id: self.user_channel_id,
        };
        let resp = wallet.open_channel(req).await?;

        if self.json {
            return helpers::print_json_pretty(&resp);
        }

        let channel_id = resp.channel_id;
        let user_channel_id = resp.user_channel_id;
        println!("Opened channel: {channel_id}");
        println!("User channel id: {user_channel_id}");
        anyhow::Ok(())
    }
}

// --- `close-channel` --- //

#[derive(Parser)]
#[command(
    about = "Close a Lightning channel between this node and Lexe's LSP",
    help_template = HELP_TEMPLATE,
)]
pub struct CloseChannelArgs {
    #[arg(help = "The channel id to close, \n\
        serialized as a 64-character hex string (32 bytes).")]
    channel_id: ChannelId,
}

impl CloseChannelArgs {
    async fn run(self, wallet: &LexeWallet) -> anyhow::Result<()> {
        let req = CloseChannelRequest {
            channel_id: self.channel_id,
        };
        wallet.close_channel(req).await?;
        println!("Channel closed.");
        anyhow::Ok(())
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
            println!(
                "\nAlternatively, scan this QR code to import your Lexe seed \
                 into another wallet:"
            );
            helpers::encode_and_print_qr(&mnemonic.to_string())?;
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

    /// Encode `data` as a QR code and print it with two newlines above and
    /// below, something like a vertical-only "quiet zone".
    ///
    /// We don't add extra horizontal quiet zones because Lexe QRs have been
    /// designed to have 80-character widths if the data inside fits, so that
    /// Lexe QRs show up nicely in most terminals. In practice the vertical
    /// quiet zone alone should be good enough.
    ///
    /// See the [`lexe_qr`] crate for details.
    pub fn encode_and_print_qr(data: &str) -> anyhow::Result<()> {
        let qr = lexe_qr::encode_unicode(data.to_owned().into_bytes())
            .context("Failed to encode QR code")?;
        println!("\n\n{qr}\n");
        Ok(())
    }

    /// Print a payment as pretty JSON, log its terminal status, and return an
    /// `Err` (so the CLI exits non-zero) if the payment failed.
    ///
    /// This design allows us to expose the payment details while still
    /// signalling that something went wrong.
    pub fn print_payment(payment: &Payment) -> anyhow::Result<()> {
        // Log the status (to stderr) before printing the payment (to stdout).
        let result = match payment.status {
            PaymentStatus::Completed => {
                info!("Payment complete!");
                Ok(())
            }
            PaymentStatus::Pending => {
                // A `Pending` status is treated as success:
                // Lightning `pay*` commands wait for a terminal state, so the
                // only way to get `Pending` here is an outbound on-chain send
                // (which takes ~1 hour to confirm).
                info!("Payment initiated; still pending confirmation.");
                Ok(())
            }
            // The returned `Err` is the failure signal; no `warn!` needed.
            PaymentStatus::Failed => Err(anyhow!("Payment failed")),
        };
        print_json_pretty(payment)?;
        result
    }

    /// Print a [`ClientInfo`] as a human-readable block.
    ///
    /// The entry title uses the client's label if present, falling back to its
    /// public key. `expires_at` is always shown.
    pub fn print_client_info(client: &ClientInfo) -> anyhow::Result<()> {
        let title = match &client.label {
            Some(label) => label.clone(),
            None => client.client_pk.to_string(),
        };
        println!("Client [{title}]");
        // `- client_pk: <hex>` is always 81 chars wide (a 64-char hex pubkey),
        // so we linebreak
        println!("    - client_pk:");
        println!("        {}", client.client_pk);

        let created_at =
            helpers::timestamp_to_datetime(client.created_at).to_rfc2822();
        println!("    - created_at: {created_at}");

        match client.expires_at {
            Some(expires_at) => {
                let expires_at =
                    helpers::timestamp_to_datetime(expires_at).to_rfc2822();
                println!("    - expires_at: {expires_at}");
            }
            None => println!("    - expires_at: Never expires!"),
        }

        if let Some(label) = &client.label {
            println!("    - label: {label}");
        }

        Ok(())
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

        // Saturating so a slightly-future timestamp (due to clock drift) reads
        // as "just now" rather than underflowing.
        let secs = timestamp.saturating_elapsed().as_secs();

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

    /// Compute an absolute expiration [`TimestampMs`] from `--expiration-days`
    /// and `--expiration-secs`, which combine additively. Returns [`None`] if
    /// neither is set.
    // Can repurpose this generically (not expiration-flavored) when needed
    pub fn expiration_from_now(
        days: Option<u32>,
        secs: Option<u32>,
    ) -> Option<TimestampMs> {
        if days.is_none() && secs.is_none() {
            return None;
        }

        const SECS_PER_DAY: u64 = 24 * 60 * 60;
        let days = u64::from(days.unwrap_or(0));
        let secs = u64::from(secs.unwrap_or(0));
        let total_secs = days.saturating_mul(SECS_PER_DAY).saturating_add(secs);

        let expires_at =
            TimestampMs::now().saturating_add(Duration::from_secs(total_secs));
        Some(expires_at)
    }

    /// Get this unix timestamp as a [`FixedOffset`] [`chrono::DateTime`].
    ///
    /// [`FixedOffset`]: chrono::FixedOffset
    pub fn timestamp_to_datetime(
        timestamp: TimestampMs,
    ) -> chrono::DateTime<chrono::Local> {
        chrono::DateTime::<chrono::Local>::from(timestamp.to_system_time())
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
            let seed =
                RootSeed::read_from_path_as_seedphrase_or_hex(path.as_path())?;
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

#[cfg(test)]
mod test {
    use lexe_common::test_utils::arbitrary;
    use lexe_payment_uri_core::{Lnurl, LnurlScheme};
    use proptest::{
        arbitrary::{any, any_with},
        prop_assert, prop_oneof, proptest,
        strategy::Strategy,
    };

    use super::*;

    /// `analyze` output goes through [`AnalyzeArgs::validate_command`], which
    /// rejects `payable`s/`claimable`s with single quotes.
    /// We should never hit that case, but this test helps check
    #[test]
    fn analyze_results_have_no_single_quote() {
        proptest!(|(
            s in prop_oneof![
                arbitrary::any_mainnet_addr().prop_map(|addr| addr.to_string()),
                any::<Invoice>().prop_map(|invoice| invoice.to_string()),
                any::<Offer>().prop_map(|offer| offer.to_string()),
                any_with::<Lnurl>(Some(LnurlScheme::Pay)).prop_map(|lnurl| lnurl.to_string()),
                any_with::<Lnurl>(Some(LnurlScheme::Withdraw)).prop_map(|lnurl| lnurl.to_string()),
            ]
        )| {
            prop_assert!(!s.contains('\''));
        });
    }
}
