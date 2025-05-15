use anyhow::Context;
use sdk_sidecar::{cli::SidecarArgs, run::Sidecar};

fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    logger::init();

    let mut args = SidecarArgs::from_cli();
    args.or_env_mut()?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build Tokio runtime")?;

    let sidecar = Sidecar::init(args)?;
    let spawn_ctrlc_handler = true;
    rt.block_on(sidecar.run(spawn_ctrlc_handler))
}
