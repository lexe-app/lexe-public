use std::process::Command;

use argh::FromArgs;
#[cfg(test)]
use proptest_derive::Arbitrary;

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::{ports::Port, UserPk},
    cli::{LspInfo, Network, OAuthConfig, ToCommand},
};

/// Commands accepted by the user node.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, FromArgs)]
#[argh(subcommand)]
#[allow(clippy::large_enum_variant)] // It will be Run most of the time
pub enum NodeCommand {
    Run(RunArgs),
    Provision(ProvisionArgs),
}

impl ToCommand for NodeCommand {
    fn append_args(&self, cmd: &mut Command) {
        match self {
            Self::Run(args) => args.append_args(cmd),
            Self::Provision(args) => args.append_args(cmd),
        }
    }
}

/// Run a user node
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "run")]
pub struct RunArgs {
    /// the Lexe user pk used in queries to the persistence API
    #[argh(option)]
    pub user_pk: UserPk,

    /// bitcoin, testnet, regtest, or signet.
    #[argh(option)]
    pub network: Network,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes.
    #[argh(switch, short = 's')]
    pub shutdown_after_sync: bool,

    /// how long the node will stay online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever the node
    /// receives some activity.
    #[argh(option, short = 'i')]
    pub inactivity_timer_sec: u64,

    /// whether the node is allowed to use mock clients instead of real ones.
    /// This option exists as a safeguard to prevent accidentally using a mock
    /// client by forgetting to pass `Some(url)` for the various Lexe services.
    /// Mock clients are only available during dev, and are cfg'd out in prod.
    #[argh(switch, short = 'm')]
    pub allow_mock: bool,

    /// protocol://host:port of the backend. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    #[argh(option)]
    pub backend_url: Option<String>,

    /// protocol://host:port of the runner. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    #[argh(option)]
    pub runner_url: Option<String>,

    /// protocol://host:port of Lexe's Esplora server.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    #[argh(option)]
    pub esplora_url: String,

    /// info relating to Lexe's LSP.
    #[argh(option)]
    pub lsp: LspInfo,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for RunArgs {
    fn default() -> Self {
        use crate::test_utils::{
            DUMMY_BACKEND_URL, DUMMY_ESPLORA_URL, DUMMY_RUNNER_URL,
        };
        Self {
            user_pk: UserPk::from_u64(1), // Test user
            network: Network::REGTEST,
            shutdown_after_sync: false,
            inactivity_timer_sec: 3600,
            backend_url: Some(DUMMY_BACKEND_URL.to_owned()),
            runner_url: Some(DUMMY_RUNNER_URL.to_owned()),
            esplora_url: DUMMY_ESPLORA_URL.to_owned(),
            lsp: LspInfo::dummy(),
            allow_mock: false,
        }
    }
}

impl ToCommand for RunArgs {
    fn append_args(&self, cmd: &mut Command) {
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
            .arg(&self.lsp.to_string());

        if self.shutdown_after_sync {
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
    }
}

/// Provision a new user node
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, FromArgs)]
#[argh(subcommand, name = "provision")]
pub struct ProvisionArgs {
    /// protocol://host:port of the backend.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    #[argh(option)]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    #[argh(option)]
    pub runner_url: String,

    /// the port on which to accept a provision request from the client.
    #[argh(option)]
    pub port: Option<Port>,

    /// configuration info for Google OAuth2.
    /// Required only if running in staging / prod.
    #[argh(option)]
    pub oauth: Option<OAuthConfig>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for ProvisionArgs {
    fn default() -> Self {
        use crate::test_utils::{DUMMY_BACKEND_URL, DUMMY_RUNNER_URL};
        Self {
            port: None,
            backend_url: DUMMY_BACKEND_URL.to_owned(),
            runner_url: DUMMY_RUNNER_URL.to_owned(),
            oauth: None,
        }
    }
}

impl ToCommand for ProvisionArgs {
    fn append_args(&self, cmd: &mut Command) {
        cmd.arg("provision")
            .arg("--backend-url")
            .arg(&self.backend_url)
            .arg("--runner-url")
            .arg(&self.runner_url);
        if let Some(port) = self.port {
            cmd.arg("--port").arg(&port.to_string());
        }
        if let Some(ref oauth) = self.oauth {
            cmd.arg("--oauth").arg(&oauth.to_string());
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use proptest::{arbitrary::any, proptest, test_runner::Config};

    use super::*;

    #[test]
    fn proptest_cmd_roundtrip() {
        let config = Config {
            max_shrink_iters: 32_000,
            ..Default::default()
        };

        proptest!(config, |(
            path_str in arbitrary::any_simple_string(),
            cmd1 in any::<NodeCommand>(),
        )| {
            let path = Path::new(&path_str);
            // Convert to std::process::Command
            let std_cmd = cmd1.to_command(path);
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
            assert_eq!(cmd1, cmd2);
        })
    }
}
