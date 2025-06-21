use std::env;

use anyhow::{bail, Context};
use common::{
    cli::{
        node::{MegaArgs, RunArgs},
        EnclaveArgs,
    },
    enclave,
    rng::SysRng,
};
use lexe_tokio::notify_once::NotifyOnce;

use crate::{
    context::{MegaContext, UserContext},
    mega,
    run::UserNode,
    DEV_VERSION, SEMVER_VERSION,
};

/// Commands accepted by the user node.
pub enum NodeCommand {
    /// Runs a mega node which can provision users or load user nodes.
    Mega(MegaArgs),
    /// Runs an individual user node directly.
    /// Avoids the need to specify provision-specific args.
    Run(RunArgs),
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
            _ => bail!("Invalid CLI options"),
        }
    }

    /// Gets the value for `RUST_LOG` passed from args.
    pub fn rust_log(&self) -> Option<&str> {
        match self {
            Self::Run(args) => args.rust_log.as_deref(),
            Self::Mega(args) => args.rust_log.as_deref(),
        }
    }

    /// Gets the value for `RUST_BACKTRACE` passed from args.
    pub fn rust_backtrace(&self) -> Option<&str> {
        match self {
            Self::Run(args) => args.rust_backtrace.as_deref(),
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
            // TODO(max): Remove this command entirely.
            Self::Run(args) => rt
                .block_on(async {
                    let user_shutdown = NotifyOnce::new();
                    let user_ctxt = UserContext {
                        lease_id: None, // Run mode doesn't have lease renewal
                        user_shutdown: user_shutdown.clone(),
                        ..Default::default()
                    };
                    let allow_mock = true;
                    let (mega_ctxt, static_tasks) = MegaContext::init(
                        &mut rng,
                        allow_mock,
                        args.backend_url.clone(),
                        args.lsp.clone(),
                        args.runner_url.clone(),
                        args.untrusted_deploy_env,
                        args.untrusted_esplora_urls.clone(),
                        args.untrusted_network,
                        user_shutdown,
                    )
                    .await
                    .context("Error initializing mega context")?;
                    let mut node = UserNode::init(
                        &mut rng,
                        args,
                        mega_ctxt,
                        user_ctxt,
                        static_tasks,
                    )
                    .await
                    .context("Error during run init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                })
                .context("Error running node"),
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
