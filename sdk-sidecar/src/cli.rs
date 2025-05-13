//! SDK sidecar CLI

use std::net::SocketAddr;

use common::{
    env::DeployEnv, ln::network::LxNetwork, or_env::OrEnvExt as _,
    root_seed::RootSeed,
};

/// Lexe SDK sidecar
// NOTE: Any changes or doc updates here should be duplicated to `.env.example`
// in the Sidecar SDK repo, which is a lot more discoverable for end users.
#[derive(argh::FromArgs)]
pub struct SidecarArgs {
    // TODO(max): Should use this instead
    // /// required: The client credentials string exported from the Lexe app.
    // /// Open the app's left sidebar > "SDK clients" > "Create new client"
    // /// Env: `LEXE_CLIENT_CREDENTIALS`.
    // #[argh(option)]
    // pub client_credentials: Option<String>,

    // TODO(max): Revisit this arg after partner signup API
    /// your Lexe user root seed.
    ///
    /// Required: true.
    /// Env: `ROOT_SEED`.
    //
    // phase 1: prototype just uses root seed
    // phase 2: package user_key_pair and root seed derived mTLS certs together
    // phase 3: fine-grained delegated auth packaged together
    #[argh(option)]
    pub root_seed: Option<RootSeed>,

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
    #[argh(option)]
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
    #[argh(option)]
    pub network: Option<LxNetwork>,
}

impl SidecarArgs {
    pub fn from_env() -> anyhow::Result<Self> {
        let mut args = argh::from_env::<Self>();

        // Fill from env vars if they're set
        args.root_seed.or_env_mut("ROOT_SEED")?;
        args.listen_addr.or_env_mut("LISTEN_ADDR")?;
        args.deploy_env.or_env_mut("DEPLOY_ENVIRONMENT")?;
        args.network.or_env_mut("NETWORK")?;

        Ok(args)
    }
}
