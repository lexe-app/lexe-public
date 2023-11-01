//! Managed Lightning Network node that runs in a secure enclave.

// Enforce disallowed methods clippy lint
#![deny(clippy::disallowed_methods)]
// once_cell replacement in std (called LazyLock)
#![feature(lazy_cell)]
// Easy side-effects in Result / Option chains
#![feature(result_option_inspect)]

pub mod cli;

mod alias;
mod api;
mod channel_manager;
mod event_handler;
mod inactivity_timer;
mod peer_manager;
mod persister;
mod provision;
mod run;
mod server;
