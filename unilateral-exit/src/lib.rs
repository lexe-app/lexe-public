//! # Unilateral exit
//!
//! Recovers all funds from a Lexe node without requiring the assistance or
//! cooperation of the Lexe company (or even for the Lexe company to exist).
//!
//! Given the user's root seed and the encrypted channel state backed up to
//! their Google Drive (channel manager + channel monitors), this crate runs a
//! minimal local Lightning node which:
//!
//! 1. Attempts to cooperatively close all channels via Lexe's LSP, falling back
//!    to unilateral (force) close if the LSP is unreachable.
//! 2. Sweeps all channel outputs into the BDK wallet derived from the same root
//!    seed, contesting any observed channel breaches along the way.
//! 3. Waits until all funds are confirmed on-chain.
//!
//! Force closes need an on-chain UTXO to fee-bump the commitment tx, so if
//! the BDK wallet is empty, the run pauses and publishes an
//! [`AnchorFundsNeeded`] deposit requirement via
//! [`RecoveryArgs::progress_tx`], then proceeds automatically once the
//! deposit confirms.
//!
//! Progress persists to a local recovery dir (same encryption-at-rest as the
//! Google Drive backup), so an interrupted run can simply be re-run.
//!
//! [`AnchorFundsNeeded`]: types::AnchorFundsNeeded
//! [`RecoveryArgs::progress_tx`]: types::RecoveryArgs::progress_tx

pub use crate::node::{RecoveryNode, RunContext, SyncContext};

/// Publicly exposed types: The inputs and outputs of a recovery run.
pub mod types;

/// Concrete type aliases.
mod alias;
/// Imports downloaded backup files into the recovery dir.
mod backups;
/// Coin selection for fee-bump (anchor CPFP) txs.
mod coin_selector;
/// Event handling.
mod event_handler;
/// The recovery node.
mod node;
/// The recovery node's peer manager.
mod peer_manager;
/// Local-directory persistence.
mod persister;
/// The recovery loop.
mod recover;
