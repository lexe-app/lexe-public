use anyhow::Context;
use sdk_sidecar::{cli::SidecarArgs, run::Sidecar};

fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    logger::init();
    let args = SidecarArgs::from_env().context("Invalid args")?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build Tokio runtime")?;

    let sidecar = Sidecar::new(args)?;
    rt.block_on(sidecar.run())
}
