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

// TODO(max): Remove the allow once the recovery node uses these.
#[allow(dead_code)]
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
