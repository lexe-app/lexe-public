use std::{path::Path, process::Command};

use argh::FromArgs;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;

use crate::{
    api::{ports::Port, UserPk},
    cli::{LspInfo, Network},
    constants::{NODE_PROVISION_DNS, NODE_RUN_DNS},
};

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
    pub app_port: Option<Port>,

    /// the port warp uses to accept requests from the runner (Lexe).
    /// Defaults to a port assigned by the OS
    #[argh(option)]
    pub runner_port: Option<Port>,

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

    /// whether the node is allowed to use mock clients instead of real ones.
    /// This option exists as a safeguard to prevent accidentally using a mock
    /// client by forgetting to pass `Some(url)` for the various Lexe services.
    /// Mock clients are only available during dev, and are cfg'd out in prod.
    #[argh(switch, short = 'm')]
    pub allow_mock: bool,

    /// protocol://host:port of the backend. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[argh(option)]
    pub backend_url: Option<String>,

    /// protocol://host:port of the runner. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[argh(option)]
    pub runner_url: Option<String>,

    /// protocol://host:port of Lexe's Esplora server.
    #[argh(option)]
    pub esplora_url: String,

    /// the <node_pk>@<sock_addr> of the LSP.
    #[argh(option)]
    // XXX(max): We need to verify this somehow; otherwise the node may accept
    // channels from someone pretending to be Lexe.
    pub lsp: LspInfo,

    /// the DNS name the node enclave should include in its remote attestation
    /// certificate and the client will expect in its connection
    #[argh(option, default = "NODE_RUN_DNS.to_owned()")]
    pub node_dns_name: String,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for RunArgs {
    /// Non-`Option<T>` fields are required by the node, with no node defaults.
    /// `Option<T>` fields are not required by the node, and use node defaults.
    fn default() -> Self {
        use crate::test_utils::{
            DUMMY_BACKEND_URL, DUMMY_ESPLORA_URL, DUMMY_LSP_INFO,
            DUMMY_RUNNER_URL,
        };
        Self {
            user_pk: UserPk::from_u64(1), // Test user
            app_port: None,
            runner_port: None,
            network: Network::default(),
            shutdown_after_sync_if_no_activity: false,
            inactivity_timer_sec: 3600,
            node_dns_name: NODE_RUN_DNS.to_owned(),
            backend_url: Some(DUMMY_BACKEND_URL.to_owned()),
            runner_url: Some(DUMMY_RUNNER_URL.to_owned()),
            esplora_url: DUMMY_ESPLORA_URL.to_owned(),
            lsp: DUMMY_LSP_INFO.clone(),
            allow_mock: false,
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
            .arg("--esplora-url")
            .arg(&self.esplora_url)
            .arg("--lsp")
            .arg(&self.lsp.to_string())
            .arg("--node-dns-name")
            .arg(&self.node_dns_name);

        if self.shutdown_after_sync_if_no_activity {
            cmd.arg("-s");
        }
        if self.allow_mock {
            cmd.arg("--allow-mock");
        }
        if let Some(ref backend_url) = self.backend_url {
            cmd.arg("--backend-url").arg(backend_url);
        }
        if let Some(ref runner_url) = self.runner_url {
            cmd.arg("--runner-url").arg(runner_url);
        }
        if let Some(app_port) = self.app_port {
            cmd.arg("--app-port").arg(&app_port.to_string());
        }
        if let Some(runner_port) = self.runner_port {
            cmd.arg("--runner-port").arg(&runner_port.to_string());
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
    #[argh(option)]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[argh(option)]
    pub runner_url: String,

    /// the port on which to accept a provision request from the client.
    #[argh(option)]
    pub port: Option<Port>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for ProvisionArgs {
    fn default() -> Self {
        use crate::test_utils::{DUMMY_BACKEND_URL, DUMMY_RUNNER_URL};
        Self {
            user_pk: UserPk::from_u64(1), // Test user
            node_dns_name: "provision.lexe.tech".to_owned(),
            port: None,
            backend_url: DUMMY_BACKEND_URL.to_owned(),
            runner_url: DUMMY_RUNNER_URL.to_owned(),
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

#[cfg(all(test, not(target_env = "sgx")))]
mod test_notsgx {
    use proptest::{
        arbitrary::{any, Arbitrary},
        prop_oneof, proptest,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for NodeCommand {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            prop_oneof! {
                any::<RunArgs>().prop_map(Self::Run),
                any::<ProvisionArgs>().prop_map(Self::Provision),
            }
            .boxed()
        }
    }

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
