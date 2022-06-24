mod api;
mod attest;
mod bitcoind_client;
mod cli;
mod convert;
mod event_handler;
mod hex_utils;
mod init;
mod logger;
mod persister;
mod repl;
mod types;

pub fn main() -> anyhow::Result<()> {
    // TODO(phlip9): init tracing

    let args = argh::from_env::<cli::Args>();
    args.run()
}
