mod api;
mod attest;
mod bitcoind_client;
mod convert;
mod event_handler;
mod hex_utils;
mod init;
mod logger;
mod persister;
mod repl;
mod structs;
mod types;

#[tokio::main]
pub async fn main() {
    match init::start_ldk().await {
        Ok(()) => {}
        Err(e) => println!("Error: {:#}", e),
    }
}
