//! SDK sidecar CLI

use std::net::SocketAddr;

use common::{
    env::DeployEnv, ln::network::LxNetwork, or_env::OrEnvExt as _,
    root_seed::RootSeed,
};

/// Lexe SDK sidecar
#[derive(argh::FromArgs)]
pub struct SidecarArgs {
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

    /// the <ip-address:port> to listen on.
    ///
    /// Default: `[::1]:5393`.
    /// Env: `LISTEN_ADDR`.
    #[argh(option)]
    pub listen_addr: Option<SocketAddr>,

    /// the current Lexe deployment environment.
    /// one of: ["dev", "staging", "prod"].
    ///
    /// Default: "prod".
    /// Env: `DEPLOY_ENVIRONMENT`.
    #[argh(option)]
    pub deploy_env: Option<DeployEnv>,

    /// the Bitcoin network run against.
    /// one of: ["mainnet", "testnet3", "testnet4", "regtest"].
    ///
    /// Default: "mainnet".
    /// Env: `NETWORK`.
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
