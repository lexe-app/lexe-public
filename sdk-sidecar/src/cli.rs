//! SDK sidecar CLI

use std::{net::SocketAddr, path::PathBuf, str::FromStr};

use anyhow::anyhow;
use app_rs::client::ClientCredentials;
use common::{
    env::DeployEnv, ln::network::LxNetwork, or_env::OrEnvExt as _,
    root_seed::RootSeed,
};

/// Lexe SDK sidecar
// NOTE: Any changes or doc updates here should be duplicated to `.env.example`
// in the Sidecar SDK repo, which is a lot more discoverable for end users.
#[derive(argh::FromArgs)]
pub struct SidecarArgs {
    /// required: The client credentials string exported from the Lexe app.
    /// Open the app's left sidebar > "SDK clients" > "Create new client".
    /// Env: `LEXE_CLIENT_CREDENTIALS`.
    #[argh(option)]
    pub client_credentials: Option<ClientCredentials>,

    /// required: A path to a file containing the client credentials string
    /// exported from the Lexe app.
    /// Open the app's left sidebar > "SDK clients" > "Create new client".
    /// Env: `LEXE_CLIENT_CREDENTIALS_PATH`.
    #[argh(option)]
    pub client_credentials_path: Option<PathBuf>,

    /// lexe user root seed.
    /// Env: `ROOT_SEED`.
    #[argh(option, hidden_help)] // hide option for now
    pub root_seed: Option<RootSeed>,

    /// path to Lexe user root seed.
    /// Env: `ROOT_SEED_PATH`.
    #[argh(option, hidden_help)] // hide option for now
    pub root_seed_path: Option<PathBuf>,

    /// optional: The `<ip_address>:<port>` to listen on.
    ///
    /// Default: `127.0.0.1:5393`.
    /// Env: `LISTEN_ADDR`.
    #[argh(option)]
    pub listen_addr: Option<SocketAddr>,

    /// optional: the current Lexe deployment environment.
    ///
    /// Options: ["prod"]
    /// Default: "prod".
    /// Env: `DEPLOY_ENVIRONMENT`.
    // TODO(max): The user has no concept of "deploy environment". In this
    // context we should derive the intended deploy env from the network.
    // This arg should be removed.
    //
    // expose the `NETWORK`: "mainnet", "testnet3", "testnet4".
    #[argh(option, hidden_help)] // hide option until we support staging
    pub deploy_env: Option<DeployEnv>,

    /// optional: the Bitcoin network to use.
    /// Currently, only "mainnet" is supported.
    ///
    /// Options: ["mainnet"].
    /// Default: "mainnet".
    /// Env: `NETWORK`.
    // NOTE: `.env.example` currently says we only support mainnet because our
    // SDK users can only use it on mainnet. However, Lexe devs can also run
    // this on regtest. Update `.env.example` if more networks are supported.
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
                anyhow!("Exactly one of `--client-credentials` or `--client-credentials-path` must be specified"),
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
        match (self.client_credentials.is_some(), self.client_credentials_path.take()) {
            (true, None) | (false, None) => Ok(()),
            (true, Some(_)) => Err(
                anyhow!("Exactly one of `--root-seed` or `--root-seed-path` must be specified"),
            ),
            (false, Some(root_seed_path)) => {
                let s = fs_ext::read_to_string(&root_seed_path)?;
                let root_seed = RootSeed::from_str(s.trim())?;
                self.root_seed= Some(root_seed);
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
