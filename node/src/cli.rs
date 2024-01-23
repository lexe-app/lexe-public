use std::sync::Arc;

use anyhow::Context;
use argh::FromArgs;
use common::{
    cli::node::{ProvisionArgs, RunArgs},
    rng::SysRng,
};

use crate::{
    api::client::{BackendClient, RunnerClient},
    provision,
    run::UserNode,
};

/// A wrapper around [`NodeCommand`] that serves as [`argh::TopLevelCommand`].
#[derive(Debug, Eq, PartialEq, FromArgs)]
pub struct NodeArgs {
    /// show the current version, then exit.
    #[argh(switch)]
    pub version: bool,

    // Has to be Option otherwise --version doesn't work
    #[argh(subcommand)]
    command: Option<NodeCommand>,
}

/// Commands accepted by the user node.
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand)]
#[allow(clippy::large_enum_variant)] // It will be Run most of the time
pub enum NodeCommand {
    Run(RunArgsWrapper),
    Provision(ProvisionArgsWrapper),
}

/// A `FromArgs` impl which takes [`RunArgs`] as a positional argument.
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand, name = "run")]
pub struct RunArgsWrapper {
    /// the JSON string-serialized [`RunArgs`].
    #[argh(positional)]
    pub args: RunArgs,
}

/// A `FromArgs` impl which takes [`ProvisionArgs`] as a positional argument.
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgsWrapper {
    /// the JSON string-serialized [`ProvisionArgs`].
    #[argh(positional)]
    pub args: ProvisionArgs,
}

impl NodeArgs {
    pub fn run(self) -> anyhow::Result<()> {
        // We have 2 total threads configured in our `Cargo.toml`.
        //
        // - One thread is reserved for the main program thread
        // - One thread is reserved for async_usercalls (see Cargo.toml)
        //
        // NOTE: This leaves no room for additional threads spawned with
        // [`tokio::task::spawn_blocking`] or [`std::thread::spawn`] - calling
        // these functions will cause the program to crash.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("Failed to build Tokio runtime")?;
        let mut rng = SysRng::new();

        let command = self
            .command
            .context("Missing subcommand: try 'help', 'run', or 'provision'")?;

        match command {
            NodeCommand::Run(RunArgsWrapper { args }) => rt
                .block_on(async {
                    let mut node = UserNode::init(&mut rng, args)
                        .await
                        .context("Error during init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                })
                .context("Error running node"),
            NodeCommand::Provision(ProvisionArgsWrapper { args }) => {
                let runner_api =
                    Arc::new(RunnerClient::new(args.runner_url.clone()));
                let backend_api =
                    Arc::new(BackendClient::new(args.backend_url.clone()));
                rt.block_on(provision::provision_node(
                    &mut rng,
                    args,
                    runner_api,
                    backend_api,
                ))
                .context("Error while provisioning")
            }
        }
    }
}
