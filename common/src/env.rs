use std::{borrow::Cow, env, fmt, fmt::Display, str::FromStr};

use anyhow::{Context, anyhow, ensure};
use lexe_std::Apply;
#[cfg(any(test, feature = "test-utils"))]
use proptest::strategy::Strategy;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::Serialize;
use serde_with::DeserializeFromStr;
use strum::VariantArray;

use crate::ln::network::LxNetwork;

/// Represents a validated `DEPLOY_ENVIRONMENT` configuration.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[derive(DeserializeFromStr, VariantArray)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
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

    /// Shorthand to check whether this [`DeployEnv`] is dev.
    #[inline]
    pub fn is_dev(self) -> bool {
        matches!(self, Self::Dev)
    }

    /// Shorthand to check whether this [`DeployEnv`] is staging or prod.
    #[inline]
    pub fn is_staging_or_prod(self) -> bool {
        matches!(self, Self::Staging | Self::Prod)
    }

    /// Get a [`str`] containing "dev", "staging", or "prod"
    #[inline]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Staging => "staging",
            Self::Prod => "prod",
        }
    }

    /// Validate the [`LxNetwork`] parameter for this deploy environment.
    pub fn validate_network(&self, network: LxNetwork) -> anyhow::Result<()> {
        use LxNetwork::*;
        match self {
            Self::Dev => ensure!(
                matches!(network, Regtest | Testnet3 | Testnet4),
                "Dev environment can only be regtest or testnet!"
            ),
            Self::Staging => ensure!(
                matches!(network, Testnet3 | Testnet4),
                "Staging environment can only be testnet!"
            ),
            Self::Prod => ensure!(
                matches!(network, Mainnet),
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

    /// Returns the gateway URL for this deploy environment.
    ///
    /// A custom gateway URL (e.g. from env) can be provided for dev.
    pub fn gateway_url(
        &self,
        dev_gateway_url: Option<Cow<'static, str>>,
    ) -> Cow<'static, str> {
        match self {
            Self::Dev => dev_gateway_url
                .unwrap_or(Cow::Borrowed("https://localhost:4040")),
            Self::Staging => Cow::Borrowed(
                "https://lexe-staging-sgx.uswest2.staging.lexe.app",
            ),
            Self::Prod =>
                Cow::Borrowed("https://lexe-prod.uswest2.prod.lexe.app"),
        }
    }

    /// A strategy for *valid* combinations of [`DeployEnv`] and [`LxNetwork`].
    #[cfg(any(test, feature = "test-utils"))]
    pub fn any_valid_network_combo()
    -> impl Strategy<Value = (DeployEnv, LxNetwork)> {
        use proptest::strategy::Just;
        // We *could* extract an associated const [(DeployEnv, Network); N]
        // enumerating all *valid* combos, then iterate over all *possible*
        // combos to test that `validate_network` is correct, but this
        // boilerplate adds very little value.
        proptest::prop_oneof![
            Just((Self::Dev, LxNetwork::Regtest)),
            Just((Self::Dev, LxNetwork::Testnet3)),
            Just((Self::Dev, LxNetwork::Testnet4)),
            Just((Self::Staging, LxNetwork::Testnet3)),
            Just((Self::Staging, LxNetwork::Testnet4)),
            Just((Self::Prod, LxNetwork::Mainnet)),
        ]
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
        f.write_str(self.as_str())
    }
}

impl Serialize for DeployEnv {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

#[cfg(test)]
mod test {
    use proptest::{prop_assert, proptest};

    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn deploy_env_roundtrip() {
        let expected_ser = r#"["dev","staging","prod"]"#;
        roundtrip::json_unit_enum_backwards_compat::<DeployEnv>(expected_ser);
        roundtrip::fromstr_display_roundtrip_proptest::<DeployEnv>();
    }

    #[test]
    fn test_any_valid_network_combo() {
        proptest!(|(
            (deploy_env, network) in DeployEnv::any_valid_network_combo(),
        )| {
            prop_assert!(deploy_env.validate_network(network).is_ok());
        })
    }
}
