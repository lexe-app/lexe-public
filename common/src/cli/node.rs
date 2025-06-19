#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::{user::UserPk, MegaId},
    cli::{EnclaveArgs, LspInfo, OAuthConfig},
    env::DeployEnv,
    ln::network::LxNetwork,
};

#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MegaArgs {
    /// A randomly generated id for this mega node.
    pub mega_id: MegaId,

    /// protocol://host:port of the backend.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub backend_url: String,

    /// protocol://host:port of the runner.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub runner_url: String,

    /// How long the usernode stays online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever activity is
    /// seen. The meganode also uses this value + a few seconds.
    pub inactivity_timer_sec: u64,

    /// info relating to Lexe's LSP.
    pub lsp: LspInfo,

    /// configuration info for Google OAuth2.
    /// Required only if running in staging / prod.
    pub oauth: Option<OAuthConfig>,

    /// The value to set for `RUST_BACKTRACE`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_backtrace: Option<String>,

    /// The value to set for `RUST_LOG`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_log: Option<String>,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// Esplora urls which someone in Lexe's infra says we should use.
    /// We'll only use urls contained in our whitelist.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_vec_simple_string()")
    )]
    pub untrusted_esplora_urls: Vec<String>,

    /// The current deploy network passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_network: LxNetwork,
}

impl EnclaveArgs for MegaArgs {
    const NAME: &str = "mega";
}

/// Run a user node
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RunArgs {
    /// the Lexe user pk used in queries to the persistence API
    pub user_pk: UserPk,

    /// whether the node should shut down after completing sync and other
    /// maintenance tasks. This only applies if no activity was detected prior
    /// to the completion of sync (which is usually what happens). Useful when
    /// starting nodes for maintenance purposes.
    pub shutdown_after_sync: bool,

    /// How long the usernode stays online (in seconds) without any activity
    /// before shutting itself down. The timer resets whenever activity is
    /// seen.
    pub inactivity_timer_sec: u64,

    /// whether the node is allowed to use mock clients instead of real ones.
    /// This option exists as a safeguard to prevent accidentally using a mock
    /// client by forgetting to pass `Some(url)` for the various Lexe services.
    /// Mock clients are only available during dev, and are cfg'd out in prod.
    // TODO(max): Remove this
    pub allow_mock: bool,

    /// protocol://host:port of the backend. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    // TODO(max): Make real url non-optional
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub backend_url: Option<String>,

    /// protocol://host:port of the runner. Defaults to a mock client if not
    /// supplied, provided that `--allow-mock` is set and we are not in prod.
    // TODO(max): Make real url non-optional
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub runner_url: Option<String>,

    /// info relating to Lexe's LSP.
    pub lsp: LspInfo,

    /// The value to set for `RUST_BACKTRACE`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_backtrace: Option<String>,

    /// The value to set for `RUST_LOG`. Does nothing if set to [`None`].
    /// Passed as an arg since envs aren't available in SGX.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_option_simple_string()")
    )]
    pub rust_log: Option<String>,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// Esplora urls which someone in Lexe's infra says we should use.
    /// We'll only use urls contained in our whitelist.
    #[cfg_attr(
        test,
        proptest(strategy = "arbitrary::any_vec_simple_string()")
    )]
    pub untrusted_esplora_urls: Vec<String>,

    /// bitcoin, testnet, regtest, or signet.
    pub untrusted_network: LxNetwork,
}

impl EnclaveArgs for RunArgs {
    const NAME: &str = "run";
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn node_args_json_string_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<MegaArgs>();
        roundtrip::json_string_roundtrip_proptest::<RunArgs>();
    }
}
