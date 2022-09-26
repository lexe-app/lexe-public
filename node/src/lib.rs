//! Managed Lightning Network node that runs in a secure enclave.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

pub mod cli;

mod api;
mod channel_manager;
mod command;
mod event_handler;
mod inactivity_timer;
mod peer_manager;
mod persister;
mod provision;
mod repl;
mod run;
mod sync;
mod types;
