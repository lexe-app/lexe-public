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
//! Progress persists to a local recovery dir (same encryption-at-rest as the
//! Google Drive backup), so an interrupted run can simply be re-run.

/// Publicly exposed types: The inputs and outputs of a recovery run.
pub mod types;

/// Imports downloaded backup files into the recovery dir.
// TODO(max): Remove the allow once the recovery node uses this.
#[allow(dead_code)]
mod backups;
/// Local-directory persistence.
// TODO(max): Remove the allow once the recovery node uses this.
#[allow(dead_code)]
mod persister;
