//! Managed Lightning Network node that runs in a secure enclave.

#![feature(slice_as_chunks)]

pub mod cli;
pub mod logger;

mod api;
mod attest;
mod bitcoind_client;
mod cert;
mod command_server;
mod convert;
mod ed25519;
mod event_handler;
mod hex;
mod inactivity_timer;
mod init;
mod peer;
mod persister;
mod provision;
mod repl;
mod types;
