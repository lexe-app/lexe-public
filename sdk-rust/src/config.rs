use std::{borrow::Cow, fmt, path::PathBuf, sync::LazyLock};

use anyhow::Context;
use common::{api::user::UserPk, env::DeployEnv, ln::network::LxNetwork};
use node_client::credentials::CredentialsRef;

use crate::unstable::provision;

/// The user agent string used for SDK requests to Lexe infrastructure.
///
/// Format: `sdk-rust/<sdk_version> node/<latest_node_version>`
///
/// Example: `sdk-rust/0.1.0 node/0.8.11`
pub static SDK_USER_AGENT: LazyLock<&'static str> = LazyLock::new(|| {
    // Get the latest node version.
    let releases = provision::releases_json();
    let node_releases =
        releases.0.get("node").expect("No 'node' in releases.json");
    let (latest_node_version, _release) =
        node_releases.last_key_value().expect("No node releases");

    let sdk_with_version = lexe_api_core::user_agent_to_lexe!();
    let user_agent = format!("{sdk_with_version} node/{latest_node_version}");

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
    /// The base data directory for all Lexe-related data.
    /// Holds data for different app environments and users.
    pub(crate) lexe_data_dir: PathBuf,
    /// Database directory for a specific wallet environment.
    /// Holds data for all users within that environment.
    ///
    /// `<lexe_data_dir>/<deploy_env>-<network>-<use_sgx>`
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
    /// `<lexe_data_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>`
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
    /// base data directory.
    pub fn new(wallet_env: WalletEnv, lexe_data_dir: PathBuf) -> Self {
        let env_db_dir = lexe_data_dir.join(wallet_env.to_string());
        Self {
            lexe_data_dir,
            env_db_dir,
        }
    }

    /// The top-level, root, base data directory for Lexe-related data.
    pub fn lexe_data_dir(&self) -> &PathBuf {
        &self.lexe_data_dir
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
    pub fn new(env_db_config: WalletEnvDbConfig, user_pk: UserPk) -> Self {
        let user_db_dir = env_db_config.env_db_dir.join(user_pk.to_string());
        Self {
            env_db_config,
            user_pk,
            user_db_dir,
        }
    }

    /// Construct a new [`WalletUserDbConfig`] from credentials and the
    /// environment database config.
    pub fn from_credentials(
        credentials: CredentialsRef<'_>,
        env_db_config: WalletEnvDbConfig,
    ) -> anyhow::Result<Self> {
        // Is `Some(_)` if the credentials were created by `node-v0.8.11+`.
        let user_pk = credentials.user_pk().context(
            "Client credentials are out of date. \
             Please create a new one from within the Lexe wallet app.",
        )?;
        Ok(Self::new(env_db_config, user_pk))
    }

    /// The environment-level database configuration.
    pub fn env_db_config(&self) -> &WalletEnvDbConfig {
        &self.env_db_config
    }

    /// The user public key.
    pub fn user_pk(&self) -> UserPk {
        self.user_pk
    }

    /// The top-level, root, base data directory for Lexe-related data.
    ///
    /// `<lexe_data_dir>`
    pub fn lexe_data_dir(&self) -> &PathBuf {
        self.env_db_config.lexe_data_dir()
    }

    /// The database directory for this wallet environment.
    ///
    /// `<lexe_data_dir>/<deploy_env>-<network>-<use_sgx>`
    pub fn env_db_dir(&self) -> &PathBuf {
        self.env_db_config.env_db_dir()
    }

    /// The user-specific database directory.
    ///
    /// `<lexe_data_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>`
    pub fn user_db_dir(&self) -> &PathBuf {
        &self.user_db_dir
    }

    /// Payment records and history.
    ///
    /// `<lexe_data_dir>/<deploy_env>-<network>-<use_sgx>/<user_pk>/payments_db`
    // Unstable
    pub(crate) fn payments_db_dir(&self) -> PathBuf {
        self.user_db_dir.join("payments_db")
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

    /// Old provision database directory that may need cleanup.
    pub(crate) fn old_provision_db_dir(&self) -> PathBuf {
        self.user_db_dir.join("provision_db")
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

        // Should match: "sdk-rust/<semver> node/<semver>"
        let (sdk_part, node_part) = user_agent
            .split_once(" node/")
            .expect("Missing ' node/' separator");

        // Validate sdk part: "sdk-rust/<version>"
        let sdk_version_str = sdk_part
            .strip_prefix("sdk-rust/")
            .expect("Missing 'sdk-rust/' prefix");
        let _sdk_version = semver::Version::from_str(sdk_version_str)
            .expect("Invalid SDK semver version");

        // Validate node version
        let _node_version = semver::Version::from_str(node_part)
            .expect("Invalid node semver version");
    }
}
