#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::MegaId,
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

    /// Maximum duration for user node leases (in seconds).
    pub lease_lifetime_secs: u64,

    /// Interval at which user nodes should renew their leases (in seconds).
    pub lease_renewal_interval_secs: u64,

    /// info relating to Lexe's LSP.
    pub lsp: LspInfo,

    /// protocol://host:port of the LSP's HTTP server.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub lsp_url: String,

    /// How long the meganode can remain inactive before it shuts itself down.
    pub mega_inactivity_secs: u64,

    /// An estimate of the amount of enclave heap consumed by shared meganode
    /// components such as the network graph, Tokio, connection pools, etc.
    pub memory_overhead: u64,

    /// configuration info for Google OAuth2.
    /// Required only if running in staging / prod.
    pub oauth: Option<OAuthConfig>,

    /// protocol://host:port of the runner.
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_simple_string()"))]
    pub runner_url: String,

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

    /// the allocatable memory available to this enclave in SGX.
    pub sgx_heap_size: u64,

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

    /// How long the usernode can remain inactive (in seconds) before it gets
    /// evicted by the UserRunner.
    pub user_inactivity_secs: u64,

    /// The # of usernodes that the meganode tries to maintain capacity for.
    /// Users are evicted when remaining memory fits fewer than this amount.
    pub usernode_buffer_slots: usize,

    /// An estimate of the amount of enclave heap consumed by each usernode.
    pub usernode_memory: u64,
}

impl EnclaveArgs for MegaArgs {
    const NAME: &str = "mega";
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn node_args_json_string_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<MegaArgs>();
    }
}
