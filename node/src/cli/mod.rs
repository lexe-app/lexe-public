use std::sync::Arc;

use anyhow::{ensure, Context};
use argh::FromArgs;
use common::cli::NodeCommand;
use common::enclave;
use common::rng::SysRng;

use crate::api::LexeApiClient;
use crate::init::LexeNode;
use crate::provision::provision;

/// the Lexe node CLI
#[derive(Debug, PartialEq, Eq, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    cmd: NodeCommand,
}

impl Args {
    pub fn run(self) -> anyhow::Result<()> {
        match self.cmd {
            NodeCommand::Run(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime");
                let mut rng = SysRng::new();
                rt.block_on(async {
                    let node = LexeNode::init(&mut rng, args).await?;
                    node.run().await
                })
                .context("Error running node")
            }
            NodeCommand::Provision(args) => {
                let machine_id = enclave::machine_id();
                ensure!(
                    args.machine_id == machine_id,
                    "cli machine id '{}' != derived machine id '{}'",
                    args.machine_id,
                    machine_id,
                );
                let measurement = enclave::measurement();
                let mut rng = SysRng::new();
                let api = Arc::new(LexeApiClient::new(
                    args.backend_url.clone(),
                    args.runner_url.clone(),
                ));

                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to init tokio runtime");
                rt.block_on(provision(args, measurement, api, &mut rng))
                    .context("error while provisioning")
            }
        }
    }
}
