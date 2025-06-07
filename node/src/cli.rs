use std::env;

use anyhow::{bail, Context};
use common::{
    cli::{
        node::{MegaArgs, ProvisionArgs, RunArgs},
        EnclaveArgs,
    },
    enclave,
    rng::SysRng,
};
use lexe_tokio::notify_once::NotifyOnce;

use crate::{
    mega, provision::ProvisionInstance, run::UserNode, DEV_VERSION,
    SEMVER_VERSION,
};

/// Commands accepted by the user node.
pub enum NodeCommand {
    /// Runs a mega node which can provision users or load user nodes.
    Mega(MegaArgs),
    /// Runs an individual user node directly.
    /// Avoids the need to specify provision-specific args.
    Run(RunArgs),
    // TODO(max): Remove, redundant with `Mega`
    Provision(ProvisionArgs),
}

impl NodeCommand {
    /// Try to parse a [`NodeCommand`] from CLI args.
    /// Returns [`None`] if we simply printed version or help.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let mut args = env::args();
        let _bin_path = args.next().context("No executable??")?;

        match (args.next().as_deref(), args.next()) {
            // If --version or --help was given, just print and exit.
            (Some("version"), _) | (Some("--version"), _) => {
                let dev_version_str = DEV_VERSION.unwrap_or("None");
                let measurement = enclave::measurement();
                println!(
                    "node-v{SEMVER_VERSION} (Dev version: v{dev_version_str})"
                );
                println!("Measurement: {measurement}");
                Ok(None)
            }
            (Some("help"), _) | (Some("--help"), _) => {
                print_help();
                Ok(None)
            }
            (Some("mega"), Some(args_str)) => {
                let args = MegaArgs::from_json_str(&args_str)
                    .context("Invalid MegaArgs JSON string")?;
                Ok(Some(NodeCommand::Mega(args)))
            }
            (Some("run"), Some(args_str)) => {
                let args = RunArgs::from_json_str(&args_str)
                    .context("Invalid RunArgs JSON string")?;
                Ok(Some(NodeCommand::Run(args)))
            }
            (Some("provision"), Some(args_str)) => {
                let args = ProvisionArgs::from_json_str(&args_str)
                    .context("Invalid ProvisionArgs JSON string")?;
                Ok(Some(NodeCommand::Provision(args)))
            }
            _ => bail!("Invalid CLI options"),
        }
    }

    /// Gets the value for `RUST_LOG` passed from args.
    pub fn rust_log(&self) -> Option<&str> {
        match self {
            Self::Run(args) => args.rust_log.as_deref(),
            Self::Provision(args) => args.rust_log.as_deref(),
            Self::Mega(args) => args.rust_log.as_deref(),
        }
    }

    /// Gets the value for `RUST_BACKTRACE` passed from args.
    pub fn rust_backtrace(&self) -> Option<&str> {
        match self {
            Self::Run(args) => args.rust_backtrace.as_deref(),
            Self::Provision(args) => args.rust_backtrace.as_deref(),
            Self::Mega(args) => args.rust_backtrace.as_deref(),
        }
    }

    /// Run this [`NodeCommand`].
    pub fn run(self) -> anyhow::Result<()> {
        // We have 2 total threads configured in our `Cargo.toml`.
        //
        // - One thread is reserved for the main program thread
        // - One thread is reserved for async_usercalls (see Cargo.toml)
        // - The remaining threads are available for worker threads or threads
        //   created via `spawn_blocking`.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            // NOTE: This should match `stack-size` in Cargo.toml.
            .thread_stack_size(0x80_0000)
            .build()
            .context("Failed to build Tokio runtime")?;
        let mut rng = SysRng::new();

        match self {
            Self::Mega(args) => rt
                .block_on(mega::run(&mut rng, args))
                .context("Mega instance error"),
            Self::Run(args) => rt
                .block_on(async {
                    let mut node = UserNode::init(&mut rng, args)
                        .await
                        .context("Error during run init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                })
                .context("Error running node"),
            // TODO(max): Remove the `provision` command, as it is redundant
            // with the functionality provided by the mega node. We can keep the
            // `run` command around, however.
            Self::Provision(args) => rt
                .block_on(async {
                    let shutdown = NotifyOnce::new();
                    let provision =
                        ProvisionInstance::init(&mut rng, args, shutdown)
                            .await?;
                    provision.run().await
                })
                .context("Provision instance error"),
        }
    }
}

/// Print out CLI help.
pub fn print_help() {
    println!(
        "CLI format: <bin_path> <help|version|mega|run> \
         [<JSON-string-serialized `MegaArgs` or `RunArgs`>]"
    );
}
