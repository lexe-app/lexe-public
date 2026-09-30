//! SDK sidecar CLI

use std::{net::SocketAddr, path::PathBuf, sync::LazyLock};

use clap::{ArgAction, Parser};
use lexe::{provision, types::auth::ClientCredentials};
use lexe_common::{ln::network::Network, or_env::OrEnvExt as _};

/// Version string; `lexe-sidecar --version` prints
/// `lexe-sidecar v0.4.23 (node-v0.10.4, released: 2026-09-11)`.
static VERSION: LazyLock<String> = LazyLock::new(|| {
    let sidecar_version = env!("CARGO_PKG_VERSION");
    let (node_version, node_release) = provision::latest_trusted_node_release();
    let node_date = node_release.release_date;
    format!("v{sidecar_version} (node-v{node_version}, released: {node_date})")
});

/// Lexe sidecar SDK CLI args
#[derive(Default, Parser)]
#[command(
    name = "lexe-sidecar",
    version = VERSION.as_str(),
    disable_version_flag = true,
    about = "Lexe SDK sidecar service",
    long_about = r#"
Lexe SDK sidecar service

The sidecar runs a local webserver that exposes a simple HTTP API for
controlling your Lexe node.

Conventions:
* CLI args take priority over envs.
* Env vars are automatically loaded from the first `.env` file in the
  current directory or parent directories.

Exporting client credentials:
* Open the app's left sidebar > "SDK clients" > "Create new client".
* To get started, we suggest placing your client credentials in a `.env` file:
```
# .env
LEXE_CLIENT_CREDENTIALS=<client_credentials>
```

Example:
```
$ lexe-sidecar
INFO (sdk): lexe_api::server: Url for (server): http://127.0.0.1:5393

$ curl http://127.0.0.1:5393/v2/health
{"status":"ok"}
```"#
)]
pub struct SidecarArgs {
    /// Client credentials exported from the Lexe app.
    /// [env: LEXE_CLIENT_CREDENTIALS]
    #[arg(long)]
    pub client_credentials: Option<ClientCredentials>,

    /// Path to a file containing client credentials exported from the Lexe
    /// app.
    /// [env: LEXE_CLIENT_CREDENTIALS_PATH]
    #[arg(long)]
    pub client_credentials_path: Option<PathBuf>,

    /// Root seed as a 64-character hex string.
    /// [env: LEXE_ROOT_SEED]
    #[arg(long)]
    pub root_seed: Option<String>,

    /// Path to a file containing the root seed (hex or mnemonic).
    /// [env: LEXE_ROOT_SEED_PATH]
    #[arg(long)]
    pub root_seed_path: Option<PathBuf>,

    /// The `<ip-address>:<port>` to listen on. [default: 127.0.0.1:5393]
    /// [env: LISTEN_ADDR]
    #[arg(long)]
    pub listen_addr: Option<SocketAddr>,

    /// The URL that clients use to connect to the sidecar; used to construct
    /// the callback in `/analyze`. [default: http://<listen_addr>]
    /// [env: LEXE_SIDECAR_URL]
    #[arg(long)]
    pub sidecar_url: Option<String>,

    /// The Bitcoin network to use: mainnet, testnet3, regtest.
    /// [default: mainnet] [env: LEXE_NETWORK]
    #[arg(long, hide = true)] // hide option until we support staging
    pub network: Option<Network>,

    /// Webhook URL for payment notifications. When a payment is finalized
    /// (completed or failed), the sidecar POSTs a JSON payload to this URL.
    /// [env: LEXE_WEBHOOK_URL]
    #[arg(long)]
    pub webhook_url: Option<String>,

    /// Shared secret for signing webhook payloads using the "Standard
    /// Webhooks" HMAC-SHA256 scheme. Recommended if the webhook receiver is
    /// publicly reachable. Typically a `whsec_`-prefixed base64 string.
    /// [env: LEXE_WEBHOOK_SECRET]
    #[arg(long)]
    pub webhook_secret: Option<String>,

    /// Data directory for local persistence. Also used by the webhook sender
    /// to store tracked payments. [default: $HOME/.lexe]
    /// [env: LEXE_DATA_DIR]
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Print version
    #[arg(short = 'v', short_alias = 'V', long, action = ArgAction::Version)]
    pub version: (),
}

impl SidecarArgs {
    /// Reads [`SidecarArgs`] from CLI args passed to the current program.
    /// NOTE: Exits the program with an error if the CLI args failed to parse.
    pub fn from_cli() -> Self {
        Self::parse()
    }

    /// Populates any unset args from env, if available.
    /// Does not overwrite any fields which are already set.
    pub fn or_env_mut(&mut self) -> anyhow::Result<()> {
        self.other_or_env_mut()?;
        self.credentials_or_env_mut()?;
        Ok(())
    }

    /// Populates any unset non-credentials args from env, if available.
    /// Does not overwrite any fields which are already set.
    pub fn other_or_env_mut(&mut self) -> anyhow::Result<()> {
        self.listen_addr.or_env_mut("LISTEN_ADDR")?;
        self.sidecar_url.or_env_mut("LEXE_SIDECAR_URL")?;
        self.network.or_env_mut("LEXE_NETWORK")?;
        self.webhook_url.or_env_mut("LEXE_WEBHOOK_URL")?;
        self.webhook_secret.or_env_mut("LEXE_WEBHOOK_SECRET")?;
        self.data_dir.or_env_mut("LEXE_DATA_DIR")?;
        Ok(())
    }

    /// Populates any unset credentials args from env, if available.
    /// Does not overwrite any fields which are already set.
    pub fn credentials_or_env_mut(&mut self) -> anyhow::Result<()> {
        self.client_credentials
            .or_env_mut("LEXE_CLIENT_CREDENTIALS")?;
        self.client_credentials_path
            .or_env_mut("LEXE_CLIENT_CREDENTIALS_PATH")?;
        self.root_seed.or_env_mut("LEXE_ROOT_SEED")?;
        self.root_seed_path.or_env_mut("LEXE_ROOT_SEED_PATH")?;
        Ok(())
    }
}
