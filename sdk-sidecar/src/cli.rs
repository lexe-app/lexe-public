//! SDK sidecar CLI

use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use app_rs::client::ClientCredentials;
use common::{
    env::DeployEnv, ln::network::LxNetwork, or_env::OrEnvExt as _,
    root_seed::RootSeed,
};

/// Lexe sidecar SDK CLI args
#[derive(argh::FromArgs)]
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

    /// lexe user root seed.
    /// (env=`ROOT_SEED`)
    // TODO(phlip9): take a pass at CLI error messages after we unhide
    #[argh(option, hidden_help)] // hide option for now
    pub root_seed: Option<RootSeed>,

    /// path to Lexe user root seed.
    /// (env=`ROOT_SEED_PATH`)
    // TODO(phlip9): take a pass at CLI error messages after we unhide
    #[argh(option, hidden_help)] // hide option for now
    pub root_seed_path: Option<PathBuf>,

    /// `<ip-address>:<port>` to listen on.
    /// (default=`127.0.0.1:5393`, env=`LISTEN_ADDR`)
    #[argh(option)]
    pub listen_addr: Option<SocketAddr>,

    /// the target deploy environment. one of: `prod`, `staging`, `dev`.
    /// (default=`prod`, env=`DEPLOY_ENVIRONMENT`)
    #[argh(option, hidden_help)] // hide option until we support staging
    pub deploy_env: Option<DeployEnv>,

    /// the Bitcoin network to use. one of `mainnet`, `testnet3`, `regtest`.
    /// (default=`mainnet`, env=`NETWORK`)
    #[argh(option, hidden_help)] // hide option until we support staging
    pub network: Option<LxNetwork>,
}

impl SidecarArgs {
    pub fn from_env() -> anyhow::Result<Self> {
        let mut args = argh::from_env::<Self>();

        // Fill from env vars if they're set
        args.client_credentials
            .or_env_mut("LEXE_CLIENT_CREDENTIALS")?;
        args.client_credentials_path
            .or_env_mut("LEXE_CLIENT_CREDENTIALS_PATH")?;
        args.root_seed.or_env_mut("ROOT_SEED")?;
        args.root_seed_path.or_env_mut("ROOT_SEED_PATH")?;
        args.listen_addr.or_env_mut("LISTEN_ADDR")?;
        args.deploy_env.or_env_mut("DEPLOY_ENVIRONMENT")?;
        args.network.or_env_mut("NETWORK")?;

        Ok(args)
    }

    /// If any of the `--*-path` options are set, load the corresponding values
    /// from those file paths into the args struct.
    pub(crate) fn load(&mut self) -> anyhow::Result<()> {
        self.load_client_credentials()?;
        self.load_root_seed()?;
        Ok(())
    }

    pub(crate) fn load_client_credentials(&mut self) -> anyhow::Result<()> {
        match (self.client_credentials.is_some(), self.client_credentials_path.take()) {
            (true, None) | (false, None) => Ok(()),
            (true, Some(_)) => Err(
                anyhow!(
                    "Only one of `--client-credentials`/`$LEXE_CLIENT_CREDENTIALS` \
                     or `--client-credentials-path`/`$LEXE_CLIENT_CREDENTIALS_PATH` \
                     must be specified"),
            ),
            (false, Some(client_credentials_path)) => {
                let s = fs_ext::read_to_string(&client_credentials_path)?;
                let client_credentials =
                    ClientCredentials::from_str(s.trim())?;
                self.client_credentials = Some(client_credentials);
                Ok(())
            }
        }
    }

    pub(crate) fn load_root_seed(&mut self) -> anyhow::Result<()> {
        match (
            self.client_credentials.is_some(),
            self.client_credentials_path.take(),
        ) {
            (true, None) | (false, None) => Ok(()),
            (true, Some(_)) => Err(anyhow!(
                "Only one of `--root-seed`/`$ROOT_SEED` or \
                    `--root-seed-path`/`$ROOT_SEED_PATH` must be specified"
            )),
            (false, Some(root_seed_path)) => {
                let s = fs_ext::read_to_string(&root_seed_path)?;
                let root_seed = RootSeed::from_str(s.trim())?;
                self.root_seed = Some(root_seed);
                Ok(())
            }
        }
    }
}

pub(crate) mod fs_ext {
    use std::{fs, path::Path};

    use anyhow::Context;

    pub(crate) fn read_to_string(path: &Path) -> anyhow::Result<String> {
        fs::read_to_string(path).with_context(|| {
            format!("Failed to read file `{}`", path.display())
        })
    }
}
