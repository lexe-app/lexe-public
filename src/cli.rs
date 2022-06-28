use argh::FromArgs;

use crate::init;
use crate::provision::provision;
use crate::types::{BitcoindRpcInfo, Network, NodeAlias, Port, UserId};

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

    /// the port warp uses to accept TLS connections from the owner
    #[argh(option, default = "1999")]
    pub warp_port: Port,
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
    pub auth_token: String,

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
