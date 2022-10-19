use std::sync::Arc;

use anyhow::Context;
use argh::FromArgs;
use common::cli::NodeCommand;
use common::constants::DEFAULT_CHANNEL_SIZE;
use common::rng::SysRng;
use common::shutdown::ShutdownChannel;
use tokio::sync::mpsc;

use crate::api::NodeApiClient;
use crate::provision;
use crate::run::UserNode;

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
        // TODO(max): Actually use the tx once we have a pub/sub system allowing
        // nodes to subscribe to chain updates from a sync enclave
        let (_poll_tip_tx, poll_tip_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = ShutdownChannel::new();

        match self.cmd {
            NodeCommand::Run(args) => rt
                .block_on(async {
                    let mut node =
                        UserNode::init(&mut rng, args, poll_tip_rx, shutdown)
                            .await
                            .context("Error during init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                })
                .context("Error running node"),
            NodeCommand::Provision(args) => {
                let api = Arc::new(NodeApiClient::new(
                    args.backend_url.clone(),
                    args.runner_url.clone(),
                ));
                rt.block_on(provision::provision_node(&mut rng, args, api))
                    .context("Error while provisioning")
            }
        }
    }
}
