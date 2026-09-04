use std::{path::PathBuf, str::FromStr, time::Duration};

use anyhow::Context;
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
    /// Path to the encrypted channel manager backup (`channel_manager` in the
    /// user's Google Drive). Not required if the recovery dir already
    /// contains one from a previous run.
    pub channel_manager_path: Option<PathBuf>,
    /// How channels should be closed.
    pub close_mode: CloseMode,
    /// Esplora API URLs for chain access. Tried in a random order until one
    /// works, so a single unreachable provider can't strand the exit.
    pub esplora_urls: Vec<String>,
    /// Paths to the encrypted channel monitor backups (`channel_monitors
    /// <name>` files in the user's Google Drive).
    pub monitor_paths: Vec<PathBuf>,
    pub network: Network,
    /// If set, receives a [`RecoveryProgress`] snapshot whenever a poll
    /// observes a change, e.g. to render a progress display. Holds `None`
    /// until the first poll completes.
    pub progress_tx: Option<watch::Sender<Option<RecoveryProgress>>>,
    /// The recovery working dir. Imported backups and all recovery node state
    /// persist here (encrypted under the root seed, like the Drive backup).
    pub recovery_dir: PathBuf,
    pub root_seed: RootSeed,
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
    /// How many channels were cooperatively closed by this run. An earlier
    /// interrupted run may have closed more.
    pub coop_closed: usize,
    /// How many channels were force closed by this run. An earlier
    /// interrupted run may have closed more.
    pub force_closed: usize,
    /// The number of channel monitors tracked, i.e. how many channels' funds
    /// this recovery covers, no matter which run closed them.
    pub num_monitors: usize,
    /// The final confirmed on-chain balance, after all channel funds were
    /// swept.
    pub onchain_balance: Amount,
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
/// [`RecoveryArgs::progress_tx`] whenever a poll observes a change,
/// e.g. to render a progress display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryProgress {
    /// The deposit requirement, set while the recovery is stalled awaiting
    /// confirmed funds for fee-bumps (see [`AnchorFundsNeeded`]).
    /// Renewed only when `needed_deposit` outgrows the last
    /// `suggested_deposit`, or the confirmed balance changes while still
    /// short, so a UI can re-prompt on every change.
    pub awaiting_anchor_funds: Option<AnchorFundsNeeded>,
    /// The chain height as of this poll.
    pub best_height: u32,
    /// The total balance still held in channel monitors, i.e. channel funds
    /// which have not yet settled into the BDK wallet.
    pub claimable_balance: Amount,
    /// The BDK wallet's confirmed balance.
    pub confirmed_balance: Amount,
    /// Channels which are still open (or still negotiating a close).
    pub num_channels: usize,
    /// Events (e.g. `SpendableOutputs` sweeps) persisted but not yet
    /// handled. Funds in this state are no longer reported by any monitor
    /// and have not yet reached the wallet.
    pub num_unhandled_events: usize,
    /// Channel monitors with unswept/unconfirmed balances.
    pub num_unresolved_monitors: usize,
    /// The BDK wallet's pending (unconfirmed) balance.
    pub pending_balance: Amount,
    /// The height by which every timelocked monitor balance (e.g. a force
    /// close's `to_local` waiting out its contest delay) will have unlocked,
    /// if any balance is timelocked.
    pub unlock_height: Option<u32>,
}

impl RecoveryProgress {
    /// Whether every channel satoshi has settled into the BDK wallet.
    pub fn is_settled(&self) -> bool {
        self.num_channels == 0
            && self.num_unresolved_monitors == 0
            && self.num_unhandled_events == 0
            && self.pending_balance == Amount::ZERO
    }
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
