//! SDK sidecar CLI

use std::{net::SocketAddr, path::PathBuf};

use lexe::types::auth::{ClientCredentials, RootSeed};
use lexe_common::{ln::network::Network, or_env::OrEnvExt as _};

/// Lexe sidecar SDK CLI args
#[derive(Default, argh::FromArgs)]
#[argh(description = r#"
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

$ curl http://127.0.0.1:5393/v1/health
{{"status":"ok"}}
```
"#)]
// NOTE: Any changes or doc updates here should be duplicated to `.env.example`
// in the Sidecar SDK repo, which is a lot more discoverable for end users.
pub struct SidecarArgs {
    /// client credentials exported from the Lexe app.
    /// (env=`LEXE_CLIENT_CREDENTIALS`)
    #[argh(option)]
    pub client_credentials: Option<ClientCredentials>,

    /// path to file containing client credentials exported from the Lexe app.
    /// (env=`LEXE_CLIENT_CREDENTIALS_PATH`)
    #[argh(option)]
    pub client_credentials_path: Option<PathBuf>,

    /// lexe user root seed, as a 64-character hex string.
    /// (env=`LEXE_ROOT_SEED`)
    #[argh(option)]
    pub root_seed: Option<RootSeed>,

    /// path to a file containing the root seed (hex or mnemonic).
    /// (env=`LEXE_ROOT_SEED_PATH`)
    #[argh(option)]
    pub root_seed_path: Option<PathBuf>,

    /// the `<ip-address>:<port>` to listen on.
    /// (default=`127.0.0.1:5393`, env=`LISTEN_ADDR`)
    #[argh(option)]
    pub listen_addr: Option<SocketAddr>,

    /// the URL that clients use to connect to the sidecar;
    /// used to construct the callback in `/analyze`
    /// (default=http://<listen_addr>, env=`LEXE_SIDECAR_URL`)
    #[argh(option)]
    pub sidecar_url: Option<String>,

    /// the Bitcoin network to use. one of `mainnet`, `testnet3`, `regtest`.
    /// (default=`mainnet`, env=`LEXE_NETWORK`)
    #[argh(option, hidden_help)] // hide option until we support staging
    pub network: Option<Network>,

    /// webhook URL for payment notifications. when a payment is finalized
    /// (completed or failed), the sidecar will POST a JSON payload to this
    /// URL. (env=`LEXE_WEBHOOK_URL`)
    #[argh(option)]
    pub webhook_url: Option<String>,

    /// data directory for persisted state.
    /// (default=`$HOME/.lexe`, env=`LEXE_DATA_DIR`)
    #[argh(option)]
    pub data_dir: Option<PathBuf>,
}

impl SidecarArgs {
    /// Reads [`SidecarArgs`] from CLI args passed to the current program.
    /// NOTE: Exits the program with an error if the CLI args failed to parse.
    pub fn from_cli() -> Self {
        argh::from_env::<Self>()
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
