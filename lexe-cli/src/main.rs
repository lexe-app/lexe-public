use anyhow::Context;
use clap::Parser;
use lexe_cli::LexeArgs;

fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("Failed to build Tokio runtime")?;

    let args = LexeArgs::parse();
    rt.block_on(lexe_cli::run(args))
}
