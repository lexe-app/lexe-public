use std::{
    fmt::{self, Display},
    process::Command,
    str::FromStr,
};

#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::user::UserPk,
    cli::{EnclaveArgs, LspInfo, OAuthConfig},
    env::DeployEnv,
    ln::network::LxNetwork,
};

/// Run a user node
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunArgs {
    /// the Lexe user pk used in queries to the persistence API
    pub user_pk: UserPk,

    /// bitcoin, testnet, regtest, or signet.
    pub network: LxNetwork,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes.
    pub shutdown_after_sync: bool,

    /// how long the node will stay online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever the node
    /// receives some activity.
    pub inactivity_timer_sec: u64,

    /// whether the node is allowed to use mock clients instead of real ones.
    /// This option exists as a safeguard to prevent accidentally using a mock
    /// client by forgetting to pass `Some(url)` for the various Lexe services.
    /// Mock clients are only available during dev, and are cfg'd out in prod.
    pub allow_mock: bool,

    /// protocol://host:port of the backend. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub backend_url: Option<String>,

    /// protocol://host:port of the runner. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub runner_url: Option<String>,

    /// Esplora urls which someone in Lexe's infra says we should use.
    /// We'll only use urls contained in our whitelist.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_vec_simple_string()")
    )]
    pub esplora_urls: Vec<String>,

    /// info relating to Lexe's LSP.
    pub lsp: LspInfo,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// The value to set for `RUST_LOG`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_log: Option<String>,

    /// The value to set for `RUST_BACKTRACE`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_backtrace: Option<String>,
}

impl EnclaveArgs for RunArgs {
    fn append_args(&self, cmd: &mut Command) {
        cmd.arg("run").arg(self.to_string());
    }
}

impl FromStr for RunArgs {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

impl Display for RunArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // serde_json::to_writer takes io::Write but `f` only impls fmt::Write
        let s =
            serde_json::to_string(&self).expect("JSON serialization failed");
        write!(f, "{s}")
    }
}

/// Provision a new user node
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProvisionArgs {
    /// protocol://host:port of the backend.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub runner_url: String,

    /// configuration info for Google OAuth2.
    /// Required only if running in staging / prod.
    pub oauth: Option<OAuthConfig>,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// The current deploy network passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_network: LxNetwork,

    /// The value to set for `RUST_LOG`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_log: Option<String>,

    /// The value to set for `RUST_BACKTRACE`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_backtrace: Option<String>,
}

impl EnclaveArgs for ProvisionArgs {
    fn append_args(&self, cmd: &mut Command) {
        cmd.arg("provision").arg(self.to_string());
    }
}

impl FromStr for ProvisionArgs {
    type Err = serde_json::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

impl Display for ProvisionArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // serde_json::to_writer takes io::Write but `f` only impls fmt::Write
        let s =
            serde_json::to_string(&self).expect("JSON serialization failed");
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn node_args_json_string_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<RunArgs>();
        roundtrip::json_string_roundtrip_proptest::<ProvisionArgs>();
    }

    #[test]
    fn node_args_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<RunArgs>();
        roundtrip::fromstr_display_roundtrip_proptest::<ProvisionArgs>();
    }
}
