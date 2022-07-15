use anyhow::Context;
use argh::FromArgs;
use common::rng::SysRng;

use crate::init;
use crate::provision::{provision, LexeRunner};
use crate::types::{BitcoindRpcInfo, Network, NodeAlias, Port, UserId};

pub const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_RUNNER_URL: &str = "http://127.0.0.1:5050";

/// the Lexe node CLI
#[derive(Debug, PartialEq, Eq, FromArgs)]
pub struct Args {
    #[argh(subcommand)]
    cmd: Command,
}

#[derive(Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand)]
pub enum Command {
    Start(StartCommand),
    Provision(ProvisionCommand),
}

/// Start the Lexe node
#[derive(Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "start")]
pub struct StartCommand {
    /// bitcoind rpc info, in the format <username>:<password>@<host>:<port>
    #[argh(positional)]
    pub bitcoind_rpc: BitcoindRpcInfo,

    /// the Lexe user id used in queries to the persistence API
    #[argh(option)]
    pub user_id: UserId,

    /// the port on which to accept Lightning P2P connections.
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub peer_port: Option<Port>,

    /// this node's Lightning Network alias
    #[argh(option, default = "NodeAlias::default()")]
    pub announced_node_name: NodeAlias,

    /// testnet or mainnet. Defaults to testnet.
    #[argh(option, default = "Network::default()")]
    pub network: Network,

    /// the port warp uses to accept commands and TLS connections.
    #[argh(option)]
    /// Defaults to a port assigned by the OS
    pub warp_port: Option<Port>,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes. Defaults to false.
    #[argh(switch, short = 's')]
    pub shutdown_after_sync_if_no_activity: bool,

    /// how long the node will stay online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever the node
    /// receives some activity. Defaults to 3600 seconds (1 hour)
    #[argh(option, short = 'i', default = "3600")]
    pub inactivity_timer_sec: u64,

    /// whether to start the REPL, for debugging purposes. Only takes effect if
    /// the node is run outside of SGX.
    #[argh(switch)]
    pub repl: bool,

    /// protocol://host:port of the node backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.into()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.into()")]
    pub runner_url: String,
}

/// Provision a new Lexe node for a user
#[derive(Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionCommand {
    /// the Lexe user id to provision the node for
    #[argh(option)]
    pub user_id: UserId,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option)]
    pub node_dns_name: String,

    /// the port to accept a TLS connection from the client for the
    /// provisioning process.
    #[argh(option)]
    pub port: Port,

    /// protocol://host:port of the node backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.into()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.into()")]
    pub runner_url: String,
}

// -- impl Args -- //

impl Args {
    pub fn run(self) -> anyhow::Result<()> {
        match self.cmd {
            Command::Start(args) => {
                // TODO(phlip9): set runtime max_blocking_threads and
                // worker_threads to a reasonable value, then match that value
                // in the Cargo.toml SGX metadata.
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime");
                let mut rng = SysRng::new();
                rt.block_on(init::start_ldk(&mut rng, args))
                    .context("Error running node")
            }
            Command::Provision(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to init tokio runtime");
                let mut rng = SysRng::new();
                let runner = LexeRunner::new(
                    args.backend_url.clone(),
                    args.runner_url.clone(),
                );
                rt.block_on(provision(args, &mut rng, runner))
                    .context("error while provisioning")
            }
        }
    }
}
