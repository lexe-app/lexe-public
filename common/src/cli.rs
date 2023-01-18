use std::fmt::{self, Display};
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use anyhow::ensure;
use argh::FromArgs;
use bitcoin::blockdata::constants;
use bitcoin::hash_types::BlockHash;
use lightning_invoice::Currency;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::arbitrary::any;
#[cfg(any(test, feature = "test-utils"))]
use proptest::arbitrary::Arbitrary;
#[cfg(any(test, feature = "test-utils"))]
use proptest::strategy::{BoxedStrategy, Just, Strategy};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::api::ports::Port;
use crate::api::UserPk;
use crate::constants::{
    DEFAULT_BACKEND_URL, DEFAULT_ESPLORA_URL, DEFAULT_RUNNER_URL,
    NODE_PROVISION_DNS, NODE_RUN_DNS,
};
use crate::ln::peer::ChannelPeer;

pub const MAINNET_NETWORK: Network = Network(bitcoin::Network::Bitcoin);
pub const TESTNET_NETWORK: Network = Network(bitcoin::Network::Testnet);
pub const REGTEST_NETWORK: Network = Network(bitcoin::Network::Regtest);
pub const SIGNET_NETWORK: Network = Network(bitcoin::Network::Signet);

/// Commands accepted by the user node.
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand)]
#[allow(clippy::large_enum_variant)] // It will be Run most of the time
pub enum NodeCommand {
    Run(RunArgs),
    Provision(ProvisionArgs),
}

impl NodeCommand {
    /// Shorthand to get the UserPk from NodeCommand
    pub fn user_pk(&self) -> UserPk {
        match self {
            Self::Run(args) => args.user_pk,
            Self::Provision(args) => args.user_pk,
        }
    }

    /// Shorthand for calling to_cmd() on either RunArgs or ProvisionArgs
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        match self {
            Self::Run(args) => args.to_cmd(bin_path),
            Self::Provision(args) => args.to_cmd(bin_path),
        }
    }

    /// Shorthand for calling `append_args(cmd)` on the inner variant.
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        match self {
            Self::Run(args) => args.append_args(cmd),
            Self::Provision(args) => args.append_args(cmd),
        }
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for NodeCommand {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        proptest::prop_oneof! {
            any::<RunArgs>().prop_map(Self::Run),
            any::<ProvisionArgs>().prop_map(Self::Provision),
        }
        .boxed()
    }
}

/// Run a user node
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "run")]
pub struct RunArgs {
    /// the Lexe user pk used in queries to the persistence API
    #[argh(option)]
    pub user_pk: UserPk,

    /// the port warp uses to accept requests from the owner.
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub owner_port: Option<Port>,

    /// the port warp uses to accept requests from the host (Lexe).
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub host_port: Option<Port>,

    /// the port on which to accept Lightning P2P connections.
    /// Defaults to a port assigned by the OS
    // TODO: We should remove this since all P2P connections are initiated by
    // the user node
    #[argh(option)]
    pub peer_port: Option<Port>,

    /// bitcoin, testnet, regtest, or signet.
    #[argh(option)]
    pub network: Network,

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

    /// protocol://host:port of the backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.to_owned()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.to_owned()")]
    pub runner_url: String,

    /// protocol://host:port of Lexe's Esplora server.
    #[argh(option, default = "DEFAULT_ESPLORA_URL.to_owned()")]
    pub esplora_url: String,

    /// the <node_pk>@<sock_addr> of the LSP.
    #[argh(option)]
    // XXX(max): We need to verify this somehow; otherwise the node may accept
    // channels from someone pretending to be Lexe.
    pub lsp: ChannelPeer,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option, default = "NODE_RUN_DNS.to_owned()")]
    pub node_dns_name: String,

    /// whether to use a mock API client. Only available during development.
    #[argh(switch, short = 'm')]
    pub mock: bool,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for RunArgs {
    /// Non-`Option<T>` fields are required by the node, with no node defaults.
    /// `Option<T>` fields are not required by the node, and use node defaults.
    fn default() -> Self {
        use crate::ln::peer::DUMMY_LSP;
        Self {
            user_pk: UserPk::from_u64(1), // Test user
            owner_port: None,
            host_port: None,
            peer_port: None,
            network: Network::default(),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 3600,
            repl: false,
            node_dns_name: NODE_RUN_DNS.to_owned(),
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
            esplora_url: DEFAULT_ESPLORA_URL.to_owned(),
            lsp: DUMMY_LSP.clone(),
            mock: false,
        }
    }
}

impl RunArgs {
    /// Constructs a Command from the contained args, adding no extra options.
    /// Requires the path to the node binary.
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        let mut cmd = Command::new(bin_path);
        self.append_args(&mut cmd);
        cmd
    }

    /// Serialize and append the args to an existing [`Command`].
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        cmd.arg("run")
            .arg("--user-pk")
            .arg(&self.user_pk.to_string())
            .arg("-i")
            .arg(&self.inactivity_timer_sec.to_string())
            .arg("--network")
            .arg(&self.network.to_string())
            .arg("--backend-url")
            .arg(&self.backend_url)
            .arg("--runner-url")
            .arg(&self.runner_url)
            .arg("--esplora-url")
            .arg(&self.esplora_url)
            .arg("--lsp")
            .arg(&self.lsp.to_string())
            .arg("--node-dns-name")
            .arg(&self.node_dns_name);

        if self.shutdown_after_sync_if_no_activity {
            cmd.arg("-s");
        }
        if self.mock {
            cmd.arg("--mock");
        }
        if self.repl {
            cmd.arg("--repl");
        }
        if let Some(owner_port) = self.owner_port {
            cmd.arg("--owner-port").arg(&owner_port.to_string());
        }
        if let Some(host_port) = self.host_port {
            cmd.arg("--host-port").arg(&host_port.to_string());
        }
        if let Some(peer_port) = self.peer_port {
            cmd.arg("--peer-port").arg(&peer_port.to_string());
        }

        cmd
    }
}

/// Provision a new user node
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgs {
    /// the Lexe user pk to provision the node for
    #[argh(option)]
    pub user_pk: UserPk,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the which client will expect in its connection
    #[argh(option, default = "NODE_PROVISION_DNS.to_owned()")]
    pub node_dns_name: String,

    /// protocol://host:port of the backend.
    #[argh(option, default = "DEFAULT_BACKEND_URL.to_owned()")]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option, default = "DEFAULT_RUNNER_URL.to_owned()")]
    pub runner_url: String,

    /// the port on which to accept a provision request from the client.
    #[argh(option)]
    pub port: Option<Port>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for ProvisionArgs {
    fn default() -> Self {
        Self {
            user_pk: UserPk::from_u64(1), // Test user
            node_dns_name: "provision.lexe.tech".to_owned(),
            port: None,
            backend_url: DEFAULT_BACKEND_URL.to_owned(),
            runner_url: DEFAULT_RUNNER_URL.to_owned(),
        }
    }
}

impl ProvisionArgs {
    /// Constructs a Command from the contained args, adding no extra options.
    /// Requires the path to the node binary.
    pub fn to_cmd(&self, bin_path: &Path) -> Command {
        let mut cmd = Command::new(bin_path);
        self.append_args(&mut cmd);
        cmd
    }

    /// Serialize and append the args to an existing [`Command`].
    pub fn append_args<'a>(&self, cmd: &'a mut Command) -> &'a mut Command {
        cmd.arg("provision")
            .arg("--user-pk")
            .arg(&self.user_pk.to_string())
            .arg("--node-dns-name")
            .arg(&self.node_dns_name)
            .arg("--backend-url")
            .arg(&self.backend_url)
            .arg("--runner-url")
            .arg(&self.runner_url);
        if let Some(port) = self.port {
            cmd.arg("--port").arg(&port.to_string());
        }
        cmd
    }
}

/// There are slight variations is how the network is represented as strings
/// across bitcoin, lightning, Lexe, etc. For consistency, we use the mapping
/// defined in [`bitcoin::Network`]'s `FromStr` impl, which is:
///
/// - Bitcoin <-> "bitcoin"
/// - Testnet <-> "testnet",
/// - Signet <-> "signet",
/// - Regtest <-> "regtest"
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Network(bitcoin::Network);

impl Network {
    pub fn to_inner(self) -> bitcoin::Network {
        self.0
    }

    pub fn to_str(self) -> &'static str {
        match self.to_inner() {
            bitcoin::Network::Bitcoin => "bitcoin",
            bitcoin::Network::Testnet => "testnet",
            bitcoin::Network::Regtest => "regtest",
            bitcoin::Network::Signet => "signet",
        }
    }

    /// Gets the blockhash of the genesis block of this [`Network`]
    pub fn genesis_hash(self) -> BlockHash {
        constants::genesis_block(self.to_inner())
            .header
            .block_hash()
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for Network {
    fn default() -> Self {
        Self(bitcoin::Network::Regtest)
    }
}

impl FromStr for Network {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let network = bitcoin::Network::from_str(s)?;
        ensure!(
            !matches!(network, bitcoin::Network::Bitcoin),
            "Mainnet is disabled for now"
        );
        Ok(Self(network))
    }
}

impl Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl From<Network> for bitcoin_bech32::constants::Network {
    fn from(network: Network) -> Self {
        match network.to_inner() {
            bitcoin::Network::Bitcoin => {
                bitcoin_bech32::constants::Network::Bitcoin
            }
            bitcoin::Network::Testnet => {
                bitcoin_bech32::constants::Network::Testnet
            }
            bitcoin::Network::Regtest => {
                bitcoin_bech32::constants::Network::Regtest
            }
            bitcoin::Network::Signet => {
                bitcoin_bech32::constants::Network::Signet
            }
        }
    }
}

impl From<Network> for Currency {
    fn from(network: Network) -> Self {
        match network.to_inner() {
            bitcoin::Network::Bitcoin => Currency::Bitcoin,
            bitcoin::Network::Testnet => Currency::BitcoinTestnet,
            bitcoin::Network::Regtest => Currency::Regtest,
            bitcoin::Network::Signet => Currency::Signet,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for Network {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        proptest::prop_oneof! {
            // TODO: Mainnet is disabled for now
            // Just(Network(bitcoin::Network::Bitcoin)),
            Just(Network(bitcoin::Network::Testnet)),
            Just(Network(bitcoin::Network::Regtest)),
            Just(Network(bitcoin::Network::Signet)),
        }
        .boxed()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_network_roundtrip() {
        // TODO: Mainnet is disabled for now
        // let mainnet1 = Network(bitcoin::Network::Bitcoin);
        let testnet1 = Network(bitcoin::Network::Testnet);
        let regtest1 = Network(bitcoin::Network::Regtest);
        let signet1 = Network(bitcoin::Network::Signet);

        // let mainnet2 = Network::from_str(&mainnet1.to_string()).unwrap();
        let testnet2 = Network::from_str(&testnet1.to_string()).unwrap();
        let regtest2 = Network::from_str(&regtest1.to_string()).unwrap();
        let signet2 = Network::from_str(&signet1.to_string()).unwrap();

        // assert_eq!(mainnet1, mainnet2);
        assert_eq!(testnet1, testnet2);
        assert_eq!(regtest1, regtest2);
        assert_eq!(signet1, signet2);
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test_notsgx {
    use proptest::proptest;

    use super::*;

    proptest! {
        #[test]
        fn proptest_cmd_roundtrip(
            path_str in ".*",
            cmd in any::<NodeCommand>(),
        ) {
            do_cmd_roundtrip(path_str, &cmd);
        }
    }

    fn do_cmd_roundtrip(path_str: String, cmd1: &NodeCommand) {
        let path = Path::new(&path_str);
        // Convert to std::process::Command
        let std_cmd = cmd1.to_cmd(path);
        // Convert to an iterator over &str args
        let mut args_iter = std_cmd.get_args().filter_map(|s| s.to_str());
        // Pop the first arg which contains the subcommand name e.g. 'run'
        let subcommand = args_iter.next().unwrap();
        // Collect the remaining args into a vec
        let cmd_args: Vec<&str> = args_iter.collect();
        dbg!(&cmd_args);
        // Deserialize back into struct
        let cmd2 = NodeCommand::from_args(&[subcommand], &cmd_args).unwrap();
        // Assert
        assert_eq!(*cmd1, cmd2);
    }
}
