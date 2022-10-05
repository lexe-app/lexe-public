//! Managed Lightning Network node that runs in a secure enclave.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]

pub mod channel_manager;
pub mod cli;
pub mod run;

mod alias;
mod api;
mod command;
mod event_handler;
mod inactivity_timer;
mod peer_manager;
mod persister;
mod provision;
mod repl;
