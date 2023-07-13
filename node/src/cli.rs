use std::sync::Arc;

use anyhow::Context;
use argh::FromArgs;
use common::{
    cli::node::NodeCommand, constants::SMALLER_CHANNEL_SIZE, rng::SysRng,
    shutdown::ShutdownChannel,
};
use lexe_ln::test_event;
use tokio::sync::broadcast;

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
        let (resync_tx, _) = broadcast::channel(SMALLER_CHANNEL_SIZE);
        let bdk_resync_rx = resync_tx.subscribe();
        let ldk_resync_rx = resync_tx.subscribe();
        let (test_event_tx, _test_event_rx) =
            test_event::test_event_channel("(node)");
        let shutdown = ShutdownChannel::new();

        match self.cmd {
            NodeCommand::Run(args) => rt
                .block_on(async {
                    let mut node = UserNode::init(
                        &mut rng,
                        args,
                        resync_tx,
                        bdk_resync_rx,
                        ldk_resync_rx,
                        test_event_tx,
                        shutdown,
                    )
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
