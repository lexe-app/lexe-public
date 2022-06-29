//! Managed Lightning Network node that runs in a secure enclave.

#![feature(slice_as_chunks)]

pub mod cli;

mod api;
mod attest;
mod bitcoind_client;
mod convert;
mod event_handler;
mod hex;
mod init;
mod logger;
mod persister;
mod provision;
mod repl;
mod types;
