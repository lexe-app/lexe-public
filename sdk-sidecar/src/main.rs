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

    rt.block_on(async move {
        let sidecar = Sidecar::init(args)?;
        let spawn_ctrlc_handler = true;
        sidecar.run(spawn_ctrlc_handler).await
    })
}
