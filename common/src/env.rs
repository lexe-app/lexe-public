use std::{env, fmt, fmt::Display, str::FromStr};

use anyhow::{anyhow, ensure, Context};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{cli::Network, Apply};

/// Represents a validated `DEPLOY_ENVIRONMENT` configuration.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum DeployEnv {
    /// "dev"
    Dev,
    /// "staging"
    Staging,
    /// "prod"
    Prod,
}

impl DeployEnv {
    /// Read a [`DeployEnv`] from env, or err if it was invalid / didn't exist.
    pub fn from_env() -> anyhow::Result<Self> {
        env::var("DEPLOY_ENVIRONMENT")
            .context("DEPLOY_ENVIRONMENT was not set")?
            .as_str()
            .apply(Self::from_str)
    }

    /// Validate the [`Network`] parameter for this deploy environment.
    pub fn validate_network(&self, network: Network) -> anyhow::Result<()> {
        match self {
            Self::Dev => ensure!(
                matches!(network, Network::REGTEST | Network::TESTNET),
                "Dev environment can only be regtest or testnet!"
            ),
            Self::Staging => ensure!(
                matches!(network, Network::TESTNET),
                "Staging environment can only be testnet!"
            ),
            Self::Prod => ensure!(
                matches!(network, Network::MAINNET),
                "Prod environment can only be mainnet!"
            ),
        }
        Ok(())
    }

    /// Validate the `SGX`/`[use_]sgx` parameter for this deploy environment.
    pub fn validate_sgx(&self, use_sgx: bool) -> anyhow::Result<()> {
        if matches!(self, Self::Staging | Self::Prod) {
            ensure!(use_sgx, "Staging and prod can only run in SGX!");
        }
        Ok(())
    }
}

impl FromStr for DeployEnv {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "dev" => Ok(Self::Dev),
            "staging" => Ok(Self::Staging),
            "prod" => Ok(Self::Prod),
            _ => Err(anyhow!(
                "Unrecognized DEPLOY_ENVIRONMENT '{s}': \
                must be 'dev', 'staging', or 'prod'"
            )),
        }
    }
}

impl Display for DeployEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Dev => "dev",
            Self::Staging => "staging",
            Self::Prod => "prod",
        };
        write!(f, "{s}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn deploy_env_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<DeployEnv>();
        roundtrip::json_string_roundtrip_proptest::<DeployEnv>();
    }
}
