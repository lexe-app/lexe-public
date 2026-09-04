use std::{path::PathBuf, str::FromStr, time::Duration};

use anyhow::Context;
use bitcoin::address::NetworkUnchecked;
use lexe_common::{
    api::user::NodePk,
    constants,
    env::DeployEnv,
    ln::{addr::LxSocketAddress, amount::Amount, network::Network},
    root_seed::RootSeed,
};
use lightning::util::ser::Hostname;
use tokio::sync::watch;

// --- Inputs --- //

/// Arguments for a recovery run.
pub struct RecoveryArgs {
    /// The encrypted channel manager backup from the user's Google Drive. Not
    /// required if the recovery dir has one from a previous run.
    pub channel_manager_path: Option<PathBuf>,
    pub close_mode: CloseMode,
    /// Esplora API URLs, tried in a random order until one works.
    pub esplora_urls: Vec<String>,
    /// The encrypted channel monitor backups from the user's Google Drive.
    pub monitor_paths: Vec<PathBuf>,
    pub network: Network,
    /// If set, receives a [`RecoveryProgress`] snapshot whenever a poll
    /// observes a change. Holds `None` until the first poll completes.
    pub progress_tx: Option<watch::Sender<Option<RecoveryProgress>>>,
    /// Where the imported backups and all recovery node state persist,
    /// encrypted under the root seed.
    pub recovery_dir: PathBuf,
    pub root_seed: RootSeed,
    /// If set, once all channel funds have confirmed in the BDK wallet, drain
    /// the wallet to this external address. Otherwise, funds remain in the
    /// root seed's BDK wallet.
    pub sweep_address: Option<bitcoin::Address<NetworkUnchecked>>,
    /// How often to sync the chain and re-evaluate recovery progress.
    pub sync_interval: Duration,
}

/// How the recovery node should close channels.
#[derive(Clone, Debug)]
pub enum CloseMode {
    /// Coop close through the LSP, erroring out rather than force closing.
    /// Avoids the force close timelock, but can't finish without the LSP.
    /// Not a true unilateral exit flow.
    CoopOnly(LexeLsp),
    /// Coop close through the LSP, then force close whatever remains.
    CoopThenForce(LexeLsp),
    /// Force close every channel without contacting the LSP. Always works,
    /// but every channel then waits out its timelock.
    ForceOnly,
}

impl CloseMode {
    /// The LSP to coop close through, if this mode uses one.
    pub(crate) fn lsp(&self) -> Option<&LexeLsp> {
        match self {
            Self::CoopOnly(lsp) | Self::CoopThenForce(lsp) => Some(lsp),
            Self::ForceOnly => None,
        }
    }

    pub(crate) fn can_force_close(&self) -> bool {
        match self {
            Self::CoopThenForce(_) | Self::ForceOnly => true,
            Self::CoopOnly(_) => false,
        }
    }
}

/// Lexe's LSP, the counterparty to all of the user's channels.
#[derive(Clone, Debug)]
pub struct LexeLsp {
    pub node_pk: NodePk,
    pub addrs: Vec<LxSocketAddress>,
}

impl LexeLsp {
    /// The LSP deployed in `deploy_env`. Dev has no fixed LSP, so `dev_lsp`
    /// supplies it.
    pub fn for_env(
        deploy_env: DeployEnv,
        dev_lsp: Option<LexeLsp>,
    ) -> anyhow::Result<Self> {
        let (node_pk, hostname) = match deploy_env {
            DeployEnv::Prod =>
                (constants::PROD_LSP_NODE_ID, constants::PROD_LSP_DNS),
            DeployEnv::Staging =>
                (constants::STAGING_LSP_NODE_ID, constants::STAGING_LSP_DNS),
            DeployEnv::Dev =>
                return dev_lsp.context("No Lexe LSP is deployed in dev"),
        };

        let node_pk = NodePk::from_str(node_pk).expect("Checked in tests");
        // This binary may run long after a redeploy changes the LSP's IPs, so
        // dial by DNS instead.
        let hostname =
            Hostname::try_from(hostname.to_owned()).expect("Checked in tests");
        let addrs = vec![LxSocketAddress::TcpDns {
            hostname,
            port: 9735,
        }];
        Ok(Self { node_pk, addrs })
    }
}

// --- Outputs --- //

/// The outcome of a completed recovery run.
#[derive(Debug)]
pub struct RecoveryReport {
    /// Channels coop closed by this run, not counting earlier runs.
    pub coop_closed: usize,
    /// Channels force closed by this run, not counting earlier runs.
    pub force_closed: usize,
    /// Channels whose funds this recovery covers, across all runs.
    pub num_monitors: usize,
    /// The BDK wallet's final confirmed balance, after any external sweep.
    pub onchain_balance: Amount,
    /// The external sweep, if one was requested and funds were available.
    pub sweep_info: Option<SweepInfo>,
}

/// A completed sweep of all recovered funds to an external address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SweepInfo {
    pub address: bitcoin::Address,
    pub amount: Amount,
    pub txid: bitcoin::Txid,
}

/// Force closing an anchor channel needs confirmed on-chain funds to fee-bump
/// its commitment tx. If the BDK wallet can't provide them, the recovery
/// waits with this deposit requirement, before closing or mid-close, until a
/// sufficient deposit to `deposit_address` confirms.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnchorFundsNeeded {
    /// The BDK wallet's confirmed balance, which `needed_deposit` already
    /// credits.
    pub confirmed_balance: Amount,
    pub deposit_address: bitcoin::Address,
    /// The estimated minimum deposit, i.e. what all stalled fee-bumps
    /// together need beyond `confirmed_balance`.
    pub needed_deposit: Amount,
    /// The deposit which keeps the fee-bumps funded as LDK escalates their
    /// feerate, up to its cap of 5x the fee estimate. One deposit of this
    /// size suffices unless the estimate itself rises.
    pub suggested_deposit: Amount,
}

/// A snapshot of the recovery's remaining work, published to
/// [`RecoveryArgs::progress_tx`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryProgress {
    /// The outstanding deposit requirement, while the recovery is stalled.
    /// Renewed only when `needed_deposit` outgrows the last
    /// `suggested_deposit`, or the confirmed balance changes while still
    /// short, so a UI can re-prompt on every change.
    pub awaiting_anchor_funds: Option<AnchorFundsNeeded>,
    /// The chain height as of this poll.
    pub best_height: u32,
    /// The total balance still claimable from channel monitors.
    pub claimable_balance: Amount,
    /// The BDK wallet's confirmed balance.
    pub confirmed_balance: Amount,
    /// Channels still open or negotiating a close.
    pub num_channels: usize,
    /// Persisted but unhandled events, e.g. `SpendableOutputs` sweeps. Funds
    /// in this state are reported by no monitor and have not yet reached the
    /// wallet.
    pub num_unhandled_events: usize,
    /// Channel monitors which still report claimable balances.
    pub num_unresolved_monitors: usize,
    /// The BDK wallet's unconfirmed balance.
    pub pending_balance: Amount,
    /// Set once the external sweep is broadcast.
    pub sweep_progress: Option<SweepProgress>,
    /// The height by which every timelocked monitor balance, e.g. a force
    /// close's `to_local` output, will have unlocked.
    pub unlock_height: Option<u32>,
}

impl RecoveryProgress {
    /// Whether all channel funds have settled into the BDK wallet. Any
    /// requested external sweep runs afterward, tracked by [`Self::is_swept`].
    pub fn is_settled(&self) -> bool {
        self.num_channels == 0
            && self.num_unresolved_monitors == 0
            && self.num_unhandled_events == 0
            && self.pending_balance == Amount::ZERO
    }

    /// Whether the external sweep has reached its required confirmations.
    pub fn is_swept(&self) -> bool {
        match &self.sweep_progress {
            Some(sweep) => sweep.confs >= sweep.confs_required,
            None => false,
        }
    }
}

/// A broadcast external sweep's confirmation progress.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SweepProgress {
    /// The sweep tx's current number of confirmations.
    pub confs: u32,
    /// The confirmations after which the sweep is considered final.
    pub confs_required: u32,
    pub info: SweepInfo,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn lexe_lsp_for_env_parses() {
        for deploy_env in [DeployEnv::Prod, DeployEnv::Staging] {
            let lsp = LexeLsp::for_env(deploy_env, None).unwrap();
            assert_eq!(lsp.addrs.len(), 1);
        }
        assert!(LexeLsp::for_env(DeployEnv::Dev, None).is_err());
    }
}
