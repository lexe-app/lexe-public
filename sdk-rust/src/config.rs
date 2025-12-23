use std::{borrow::Cow, fmt, path::PathBuf, sync::LazyLock};

use common::{api::user::UserPk, env::DeployEnv, ln::network::LxNetwork};

use crate::unstable::provision;

/// The user agent string used for SDK requests to Lexe infrastructure.
///
/// Format: `sdk-rust-v<sdk_version> (node-v<latest_node_version>)`
///
/// Example: `sdk-rust-v0.1.0 (node-v0.8.11)`
pub static SDK_USER_AGENT: LazyLock<&'static str> = LazyLock::new(|| {
    // Get the latest node version.
    let releases = provision::releases_json();
    let node_releases =
        releases.0.get("node").expect("No 'node' in releases.json");
    let (latest_node_version, _release) =
        node_releases.last_key_value().expect("No node releases");

    let sdk_with_version = lexe_api_core::user_agent_to_lexe!();
    let user_agent =
        format!("{sdk_with_version} (node-v{latest_node_version})");

    Box::leak(user_agent.into_boxed_str())
});

// --- Structs --- //
//
// - WalletEnv (`wallet_env`)
// - WalletEnvConfig (`env_config`)
// - WalletUserConfig (`user_config`)
// - WalletEnvDbConfig (`env_db_config`)
// - WalletUserDbConfig (`user_db_config`)

/// A wallet environment, e.g. "prod-mainnet-true".
///
/// We use this to disambiguate persisted state and secrets so we don't
/// accidentally clobber state when testing across e.g. testnet vs regtest.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WalletEnv {
    /// Prod, staging, or dev.
    pub deploy_env: DeployEnv,
    /// The Bitcoin network: mainnet, testnet, or regtest.
    pub network: LxNetwork,
    /// Whether our node should be running in a real SGX enclave.
    /// Set to [`true`] for prod and staging.
    pub use_sgx: bool,
}

/// A configuration for a wallet environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalletEnvConfig {
    /// The wallet environment.
    pub wallet_env: WalletEnv,
    // NOTE(unstable): Fields should stay pub(crate) until API is more mature
    pub(crate) gateway_url: Cow<'static, str>,
    pub(crate) user_agent: Cow<'static, str>,
}

/// A wallet configuration for a specific user and wallet environment.
#[derive(Clone)]
pub struct WalletUserConfig {
    /// The user public key.
    pub user_pk: UserPk,
    /// The configuration for the wallet environment.
    pub env_config: WalletEnvConfig,
}

/// Database directory configuration for a specific wallet environment.
#[derive(Clone)]
pub struct WalletEnvDbConfig {
    // NOTE(unstable): Fields should stay pub(crate) until API is more mature
    /// The base database directory for all Lexe-related data.
    /// Holds data for different app environments and users.
    pub(crate) lexe_db_dir: PathBuf,
    /// Database directory for a specific wallet environment.
    /// Holds data for all users within that environment.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>`
    pub(crate) env_db_dir: PathBuf,
}

/// Database directory configuration for a specific user and wallet environment.
#[derive(Clone)]
pub struct WalletUserDbConfig {
    // NOTE(unstable): Fields should stay pub(crate) until API is more mature
    /// Environment-level database configuration.
    pub(crate) env_db_config: WalletEnvDbConfig,
    /// The user public key.
    pub(crate) user_pk: UserPk,
    /// Database directory for a specific user.
    /// Contains user-specific data like payments, settings, etc.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>`
    pub(crate) user_db_dir: PathBuf,
}

// --- impl WalletEnv --- //

impl WalletEnv {
    /// Production environment: prod, mainnet, SGX.
    pub fn prod() -> Self {
        Self {
            deploy_env: DeployEnv::Prod,
            network: LxNetwork::Mainnet,
            use_sgx: true,
        }
    }

    /// Staging environment: staging, testnet4, SGX.
    pub fn staging() -> Self {
        Self {
            deploy_env: DeployEnv::Staging,
            network: LxNetwork::Testnet4,
            use_sgx: true,
        }
    }

    /// Dev environment: dev, regtest, with configurable SGX.
    pub fn dev(use_sgx: bool) -> Self {
        Self {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx,
        }
    }
}

impl fmt::Display for WalletEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let deploy_env = self.deploy_env.as_str();
        let network = self.network.as_str();
        let sgx = if self.use_sgx { "sgx" } else { "dbg" };
        write!(f, "{deploy_env}-{network}-{sgx}")
    }
}

// --- impl WalletEnvConfig --- //

impl WalletEnvConfig {
    /// Construct a [`WalletEnvConfig`].
    #[cfg(feature = "unstable")]
    pub fn new(
        wallet_env: WalletEnv,
        gateway_url: Cow<'static, str>,
        user_agent: Cow<'static, str>,
    ) -> Self {
        Self {
            wallet_env,
            gateway_url,
            user_agent,
        }
    }

    /// Get a [`WalletEnvConfig`] for production.
    pub fn prod() -> Self {
        let wallet_env = WalletEnv::prod();
        let dev_gateway_url = None;
        Self {
            gateway_url: wallet_env.deploy_env.gateway_url(dev_gateway_url),
            user_agent: Cow::Borrowed(*SDK_USER_AGENT),
            wallet_env,
        }
    }

    /// Get a [`WalletEnvConfig`] for staging.
    pub fn staging() -> Self {
        let wallet_env = WalletEnv::staging();
        let dev_gateway_url = None;
        Self {
            gateway_url: wallet_env.deploy_env.gateway_url(dev_gateway_url),
            user_agent: Cow::Borrowed(*SDK_USER_AGENT),
            wallet_env,
        }
    }

    /// Get a [`WalletEnvConfig`] for dev/testing.
    pub fn dev(
        use_sgx: bool,
        dev_gateway_url: Option<impl Into<Cow<'static, str>>>,
    ) -> Self {
        let wallet_env = WalletEnv::dev(use_sgx);
        let dev_gateway_url = dev_gateway_url.map(Into::into);
        Self {
            gateway_url: wallet_env.deploy_env.gateway_url(dev_gateway_url),
            user_agent: Cow::Borrowed(*SDK_USER_AGENT),
            wallet_env,
        }
    }

    /// The gateway URL.
    #[cfg(feature = "unstable")]
    pub fn gateway_url(&self) -> &str {
        &self.gateway_url
    }

    /// The user agent string.
    #[cfg(feature = "unstable")]
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }
}

// --- impl WalletEnvDbConfig --- //

impl WalletEnvDbConfig {
    /// Construct a new [`WalletEnvDbConfig`] from the wallet environment and
    /// base database directory.
    pub fn new(wallet_env: WalletEnv, lexe_db_dir: PathBuf) -> Self {
        let env_db_dir = lexe_db_dir.join(wallet_env.to_string());
        Self {
            lexe_db_dir,
            env_db_dir,
        }
    }

    /// The top-level, root, base database directory for Lexe-related data.
    pub fn lexe_db_dir(&self) -> &PathBuf {
        &self.lexe_db_dir
    }

    /// The database directory for this wallet environment.
    pub fn env_db_dir(&self) -> &PathBuf {
        &self.env_db_dir
    }
}

// --- impl WalletUserDbConfig --- //

impl WalletUserDbConfig {
    /// Construct a new [`WalletUserDbConfig`] from the environment database
    /// config and user public key.
    pub fn new(user_pk: UserPk, env_db_config: WalletEnvDbConfig) -> Self {
        let user_db_dir = env_db_config.env_db_dir.join(user_pk.to_string());
        Self {
            env_db_config,
            user_pk,
            user_db_dir,
        }
    }

    /// The environment-level database configuration.
    pub fn env_db_config(&self) -> &WalletEnvDbConfig {
        &self.env_db_config
    }

    /// The user public key.
    pub fn user_pk(&self) -> UserPk {
        self.user_pk
    }

    /// The top-level, root, base database directory for Lexe-related data.
    ///
    /// `<lexe_db_dir>`
    pub fn lexe_db_dir(&self) -> &PathBuf {
        self.env_db_config.lexe_db_dir()
    }

    /// The database directory for this wallet environment.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>`
    pub fn env_db_dir(&self) -> &PathBuf {
        self.env_db_config.env_db_dir()
    }

    /// The user-specific database directory.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>`
    pub fn user_db_dir(&self) -> &PathBuf {
        &self.user_db_dir
    }

    /// Payment records and history.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>/payments_db`
    // Unstable
    pub(crate) fn payments_db_dir(&self) -> PathBuf {
        self.user_db_dir.join("payments_db")
    }

    /// Node provisioning history.
    ///
    /// `<lexe_db_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>/provision_db`
    // Unstable
    pub(crate) fn provision_db_dir(&self) -> PathBuf {
        self.user_db_dir.join("provision_db")
    }

    // --- Old dirs --- //

    /// Old payment database directories that may need cleanup after migration.
    pub(crate) fn old_payment_db_dirs(&self) -> [PathBuf; 1] {
        [
            // BasicPaymentV1
            self.user_db_dir.join("payment_db"),
            // Add more here as needed
        ]
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    /// Ensure SDK_USER_AGENT parses correctly and has the expected format.
    #[test]
    fn test_sdk_user_agent() {
        let user_agent: &str = &SDK_USER_AGENT;

        // Should match: "sdk-rust-v<semver> (node-v<semver>)"
        let (sdk_part, rest) = user_agent
            .split_once(" (node-v")
            .expect("Missing ' (node-v' separator");
        let node_version_str =
            rest.strip_suffix(')').expect("Missing closing ')'");

        // Validate sdk part: "sdk-rust-v<version>"
        let sdk_version_str = sdk_part
            .strip_prefix("sdk-rust-v")
            .expect("Missing 'sdk-rust-v' prefix");
        let _sdk_version = semver::Version::from_str(sdk_version_str)
            .expect("Invalid SDK semver version");

        // Validate node version
        let _node_version = semver::Version::from_str(node_version_str)
            .expect("Invalid node semver version");
    }
}
