use argh::FromArgs;

use crate::init;
use crate::provision::provision;
use crate::types::{
    AuthToken, BitcoindRpcInfo, Network, NodeAlias, Port, UserId,
};

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

    /// the port on which to accept Lightning P2P connections
    #[argh(option, default = "9735")]
    pub peer_port: Port,

    /// this node's Lightning Network alias
    #[argh(option, default = "NodeAlias::default()")]
    pub announced_node_name: NodeAlias,

    /// testnet or mainnet. Defaults to testnet.
    #[argh(option, default = "Network::default()")]
    pub network: Network,

    /// the Lexe user id used in queries to the persistence API
    #[argh(option)]
    pub user_id: UserId,

    /// the port warp uses to accept commands and TLS connections
    #[argh(option, default = "1999")]
    pub warp_port: Port,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes. Defaults to false.
    #[argh(switch, short = 's')]
    pub shutdown_after_sync_if_no_activity: bool,

    /// how long the node will stay online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever the node
    /// receives some activity. Defaults to 3600 seconds (1 hour)
    #[argh(option, default = "3600")]
    pub inactivity_timer_sec: u64,

    /// whether to start the REPL, for debugging purposes. Only takes effect if
    /// the node is run outside of SGX.
    #[argh(switch)]
    pub repl: bool,
}

/// Provision a new Lexe node for a user
#[derive(Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionCommand {
    /// the Lexe user id to provision the node for
    #[argh(option)]
    pub user_id: UserId,

    /// IDK yet. need to authenticate client connections pre-provision somehow
    #[argh(option)]
    pub auth_token: AuthToken,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option)]
    pub node_dns_name: String,

    /// the port to accept a TLS connection from the client for the
    /// provisioning process.
    #[argh(option)]
    pub port: Port,
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
                rt.block_on(init::start_ldk(args))
            }
            Command::Provision(args) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to init tokio runtime");
                rt.block_on(provision(args))
            }
        }
    }
}
