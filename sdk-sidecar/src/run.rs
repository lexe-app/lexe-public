use std::{
    borrow::Cow,
    env,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, anyhow, ensure};
use lexe::{
    config::WalletEnvConfig,
    types::auth::{ClientCredentials, Credentials, RootSeed},
    wallet::LexeWallet,
};
use lexe_api::server::{LayerConfig, build_server_url};
use lexe_common::{env::DeployEnv, ln::network::Network};
use lexe_tokio::{
    notify_once::NotifyOnce,
    task::{self, LxTask},
};
use quick_cache::unsync;
use standardwebhooks::Webhook as WebhookSigner;
use tracing::{info, info_span, instrument, warn};

use crate::{cli::SidecarArgs, server, webhook::WebhookSender};

/// `127.0.0.1:5393` We use IPv4 because it's more approachable to newbie devs.
/// The docs note that IPv6 is still supported.
const DEFAULT_LISTEN_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5393));
const WALLET_CACHE_CAPACITY: usize = 64;

pub struct Sidecar {
    deploy_env: DeployEnv,
    network: Network,
    sidecar_url: String,
    static_tasks: Vec<LxTask<()>>,
    shutdown: NotifyOnce,
}

impl Sidecar {
    /// Initialize the [`Sidecar`]
    #[instrument(skip_all, name = "(sidecar)")]
    pub fn init(args: SidecarArgs) -> anyhow::Result<Self> {
        let mut static_tasks = Vec::with_capacity(1);

        let network = args.network.unwrap_or(Network::Mainnet);
        let use_sgx = matches!(env::var("SGX").as_deref(), Ok("true"));
        let dev_gateway_url = env::var("DEV_GATEWAY_URL").ok().map(Cow::Owned);
        let wallet_env_config = match network {
            Network::Mainnet => WalletEnvConfig::mainnet(),
            Network::Regtest =>
                WalletEnvConfig::regtest(use_sgx, dev_gateway_url.clone()),
            Network::Testnet3 => WalletEnvConfig::testnet3(),
            Network::Signet | Network::Testnet4 =>
                return Err(anyhow!("{network} network is not supported.")),
        };

        // Get the data dir from args with fallback to the Lexe default
        let data_dir =
            args.data_dir.map_or_else(lexe::default_lexe_data_dir, Ok)?;

        let maybe_credentials = resolve_credentials(
            &wallet_env_config,
            data_dir.as_path(),
            args.client_credentials,
            args.client_credentials_path,
            args.root_seed,
            args.root_seed_path,
        )?;

        // Create the default wallet if default credentials were provided.
        let default = match maybe_credentials {
            Some(credentials) => {
                let wallet = LexeWallet::without_db(
                    wallet_env_config.clone(),
                    credentials.as_ref(),
                )
                .context("Failed to create wallet")?;

                Some((Arc::new(wallet), Arc::new(credentials)))
            }
            None => None,
        };

        // Parse webhook URL if provided
        let webhook_url = args
            .webhook_url
            .map(|s| reqwest::Url::from_str(&s))
            .transpose()
            .context("Invalid webhook URL")?;

        // Parse the optional shared secret used to sign outbound webhooks
        // using the "Standard Webhooks" HMAC-SHA256 scheme.
        let webhook_signer = match (&webhook_url, args.webhook_secret) {
            (Some(_), Some(secret)) =>
                WebhookSigner::new(&secret).map(Some).context(
                    "Invalid LEXE_WEBHOOK_SECRET: expected a base64-encoded \
                     string, optionally prefixed with `whsec_`. Generate one \
                     with: `openssl rand -base64 24 | sed 's/^/whsec_/'`",
                )?,
            (Some(_), None) => {
                info!(
                    "LEXE_WEBHOOK_SECRET is not set; outbound webhooks will \
                     be unsigned. Configure a secret if your webhook \
                     receiver is publicly reachable."
                );
                None
            }
            (None, Some(_)) => {
                warn!(
                    "LEXE_WEBHOOK_SECRET was set but LEXE_WEBHOOK_URL was \
                     not; ignoring the secret."
                );
                None
            }
            (None, None) => None,
        };

        // Create WebhookSender if webhook URL is configured
        let shutdown = NotifyOnce::new();
        let wallet_cache =
            Arc::new(Mutex::new(unsync::Cache::new(WALLET_CACHE_CAPACITY)));
        let webhook_tx = match webhook_url {
            Some(url) => {
                let sidecar_dir = data_dir.join("sidecar");
                let (sender, tx) = WebhookSender::new(
                    default.as_ref().map(|(w, _)| w.clone()),
                    shutdown.clone(),
                    sidecar_dir,
                    url,
                    webhook_signer,
                    wallet_cache.clone(),
                    wallet_env_config.clone(),
                );
                static_tasks.push(sender.spawn());
                Some(tx)
            }
            None => None,
        };

        // Spawn HTTP server
        let listen_addr = args.listen_addr.unwrap_or(DEFAULT_LISTEN_ADDR);
        let maybe_tls_and_dns = None;
        let maybe_dns = maybe_tls_and_dns.as_ref().map(|(_, dns)| *dns);
        let sidecar_url = args
            .sidecar_url
            .unwrap_or_else(|| build_server_url(listen_addr, maybe_dns));
        let deploy_env = wallet_env_config.wallet_env.deploy_env;
        let router_state = Arc::new(server::RouterState {
            sidecar_url,
            default,
            wallet_cache,
            wallet_env_config,
            webhook_tx,
        });
        const SERVER_SPAN_NAME: &str = "(server)";
        let (server_task, sidecar_url) = lexe_api::server::spawn_server_task(
            listen_addr,
            server::router(router_state),
            LayerConfig::default(),
            maybe_tls_and_dns,
            SERVER_SPAN_NAME.into(),
            info_span!(SERVER_SPAN_NAME),
            shutdown.clone(),
        )
        .context("Failed to spawn server task")?;
        static_tasks.push(server_task);

        Ok(Self {
            deploy_env,
            network,
            sidecar_url,
            static_tasks,
            shutdown,
        })
    }

    /// Get the url of the [`Sidecar`] webserver, e.g. "http://127.0.0.1:5393".
    pub fn url(&self) -> String {
        self.sidecar_url.clone()
    }

    pub fn deploy_env(&self) -> DeployEnv {
        self.deploy_env
    }

    pub fn network(&self) -> Network {
        self.network
    }

    /// Get a clone of the shutdown channel which can be used to shut down the
    /// [`Sidecar`]. Simply call [`NotifyOnce::send`] on the returned channel.
    pub fn shutdown_channel(&self) -> NotifyOnce {
        self.shutdown.clone()
    }

    /// Runs the [`Sidecar`] until a shutdown signal is received.
    ///
    /// - Set `spawn_ctrlc_handler` to `true` if you'd like the sidecar to
    ///   listen for a Ctrl+C signal to initiate a shutdown.
    ///
    /// Generally, you want to `.await` on this function until it's complete,
    /// but it's also OK to spawn this function call into a task.
    #[instrument(skip_all, name = "(sidecar)")]
    pub async fn run(self, spawn_ctrlc_handler: bool) -> anyhow::Result<()> {
        // Shutdown on CTRL+C
        if spawn_ctrlc_handler {
            LxTask::spawn("ctrlc-handler", {
                let shutdown = self.shutdown.clone();
                async move {
                    use tokio::signal::ctrl_c;

                    info!("Ctrl+C handler ready, press Ctrl+C to shut down.");
                    ctrl_c().await.expect("Error receiving first CTRL+C");

                    info!(
                        "CTRL+C received, starting graceful shutdown. \
                         Hit CTRL+C again to quit immediately."
                    );
                    shutdown.send();
                    ctrl_c().await.expect("Error receiving second CTRL+C");
                    std::process::exit(1);
                }
            })
            .detach();
        }

        // Wait for graceful shutdown (with time limit)
        const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(10);
        let (_eph_tasks_tx, eph_tasks_rx) = tokio::sync::mpsc::channel(1);
        task::try_join_tasks_and_shutdown(
            self.static_tasks,
            eph_tasks_rx,
            self.shutdown,
            SHUTDOWN_TIME_LIMIT,
        )
        .await
        .context("Error awaiting tasks")?;

        Ok(())
    }
}

/// Resolve credentials from the provided args or the seedphrase file.
///
/// At most one credential source may be specified. If none is provided,
/// falls back to the seedphrase file in the data directory. Returns `None`
/// if no credentials are configured.
//
// NOTE: Keep in sync with `resolve_credentials` in
//       `public/lexe-cli/src/lib.rs`.
fn resolve_credentials(
    env_config: &WalletEnvConfig,
    data_dir: &Path,
    client_credentials: Option<ClientCredentials>,
    client_credentials_path: Option<PathBuf>,
    root_seed: Option<RootSeed>,
    root_seed_path: Option<PathBuf>,
) -> anyhow::Result<Option<Credentials>> {
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

    // Get the provided source
    let source_if_direct = Cow::Borrowed("args");
    let (maybe_creds, source) = if let Some(cc) = client_credentials {
        (Some(Credentials::from(cc)), source_if_direct)
    } else if let Some(path) = &client_credentials_path {
        let contents = std::fs::read_to_string(path).with_context(|| {
            format!("Failed to read '{}' as path", path.display())
        })?;
        let cc = ClientCredentials::from_string(contents.trim()).context(
            format!(
                "Failed to parse client credentials from '{}'",
                path.display()
            ),
        )?;
        let source = Cow::Owned(path.display().to_string());
        (Some(Credentials::from(cc)), source)
    } else if let Some(seed) = root_seed {
        (Some(Credentials::from(seed)), source_if_direct)
    } else if let Some(path) = &root_seed_path {
        let seed = RootSeed::read_from_path_either(path.as_path())?;
        let source = Cow::Owned(path.display().to_string());
        (Some(Credentials::from(seed)), source)
    } else {
        // Try to get credentials from a locally persisted wallet
        let seed_path = env_config.wallet_env.seedphrase_path(data_dir);
        let root_seed = RootSeed::read_from_path(&seed_path)?;
        let source = Cow::Owned(seed_path.display().to_string());
        (root_seed.map(Credentials::from), source)
    };

    // Log the source of credentials
    let creds_kind =
        maybe_creds.as_ref().map(|credentials| match credentials {
            Credentials::ClientCredentials(_) => "client credentials",
            Credentials::RootSeed(_) => "root seed",
        });
    if let Some(kind) = creds_kind {
        info!("Using {kind} from {source}.");
    } else {
        // TODO(nicole): mention locally persisted wallet via lexe_data_dir
        //               once sidecar has a way to init that?
        info!(
            "No client credentials configured. \
             Credentials must be set per-request via the Authorization \
             header. Alternatively, one of the following flags can be set:
             \t--client-credentials / $LEXE_CLIENT_CREDENTIALS\n\
             \t--client-credentials-path / $LEXE_CLIENT_CREDENTIALS_PATH\n\
             \t--root-seed / $LEXE_ROOT_SEED\n\
             \t--root-seed-path / $LEXE_ROOT_SEED_PATH",
        );
    }

    Ok(maybe_creds)
}
