//! Managed Lightning Network node that runs in a secure enclave.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// once_cell replacement in std (called LazyLock)
#![feature(lazy_cell)]
// Easy side-effects in Result / Option chains
#![feature(result_option_inspect)]

pub mod alias;
pub mod channel_manager;
pub mod cli;
pub mod peer_manager;
pub mod persister;
pub mod run;

mod api;
mod event_handler;
mod inactivity_timer;
mod provision;
mod server;
