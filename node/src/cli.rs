use std::sync::Arc;

use anyhow::Context;
use argh::FromArgs;
use common::{cli::node::NodeCommand, rng::SysRng};

use crate::{
    api::client::{BackendClient, RunnerClient},
    provision,
    run::UserNode,
};

/// A wrapper around [`NodeCommand`] that serves as [`argh::TopLevelCommand`].
#[derive(Debug, PartialEq, Eq, FromArgs)]
pub struct NodeArgs {
    #[argh(subcommand)]
    cmd: NodeCommand,
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

        match self.cmd {
            NodeCommand::Run(args) => rt
                .block_on(async {
                    let mut node = UserNode::init(&mut rng, args)
                        .await
                        .context("Error during init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                })
                .context("Error running node"),
            NodeCommand::Provision(args) => {
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
