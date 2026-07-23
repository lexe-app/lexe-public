use anyhow::Context;
use sdk_sidecar::{cli::SidecarArgs, run::Sidecar};

fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    lexe::init_logger("info");

    let mut args = SidecarArgs::from_cli();
    args.other_or_env_mut()?;

    // Check for CLI credentials args
    let from_cli = args.client_credentials.is_some()
        || args.client_credentials_path.is_some()
        || args.root_seed.is_some()
        || args.root_seed_path.is_some();
    // If no CLI credentials args, populate from env
    if !from_cli {
        args.credentials_or_env_mut()?;
    }

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build Tokio runtime")?;

    rt.block_on(async move {
        let sidecar = Sidecar::init(args)?;

        // Install a Ctrl+C handler which triggers a graceful shutdown.
        lexe_tokio::task::spawn_ctrlc_shutdown(&sidecar.shutdown_channel())
            .detach();

        sidecar.run().await
    })
}
