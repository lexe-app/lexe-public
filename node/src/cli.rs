use std::sync::Arc;

use anyhow::Context;
use argh::FromArgs;
use common::{cli::node::NodeCommand, rng::SysRng, shutdown::ShutdownChannel};

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
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("Failed to build Tokio runtime")?;
        let mut rng = SysRng::new();
        let shutdown = ShutdownChannel::new();

        match self.cmd {
            NodeCommand::Run(args) => rt
                .block_on(async {
                    let mut node = UserNode::init(&mut rng, args, shutdown)
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
