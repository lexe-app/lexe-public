use std::{fmt, fmt::Display, path::Path, str::FromStr};

use anyhow::Context;
#[cfg(test)]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::user::NodePk,
    ln::{addr::LxSocketAddress, amount::Amount},
};

/// User node CLI args.
pub mod node;

/// A trait for the arguments to an enclave command.
pub trait EnclaveArgs: Serialize + DeserializeOwned {
    /// The name of the command, e.g. "run", "provision", "mega"
    const NAME: &str;

    /// Construct a [`std::process::Command`] from the contained args.
    /// Requires the path to the binary.
    fn to_command(&self, bin_path: &Path) -> std::process::Command {
        let mut command = std::process::Command::new(bin_path);
        self.append_args(&mut command);
        command
    }

    /// Serialize and append the contained args to an existing
    /// [`std::process::Command`].
    fn append_args(&self, cmd: &mut std::process::Command) {
        cmd.arg(Self::NAME).arg(self.to_json_string());
    }

    fn from_json_str(json_str: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json_str)
    }

    fn to_json_string(&self) -> String {
        serde_json::to_string(self).expect("JSON serialization failed")
    }
}

/// Information about the LSP which the user node needs to connect and to
/// generate route hints when no channel exists.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LspInfo {
    /// The protocol://host:port of the LSP's HTTP server. The node will
    /// default to a mock client if not supplied, provided that
    /// `--allow-mock` is set and we are not in prod.
    // compat: alias added in {node,lsp}-v0.7.0
    #[serde(rename = "url", alias = "node_api_url")]
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub node_api_url: Option<String>,
    pub node_pk: NodePk,
    /// The socket on which the LSP accepts P2P LN connections from user nodes
    // compat: alias added in {node,lsp}-v0.7.0
    #[serde(rename = "addr", alias = "private_p2p_addr")]
    pub private_p2p_addr: LxSocketAddress,

    // -- LSP -> User fees -- //
    /// LSP's configured base fee for forwarding over LSP -> User channels.
    ///
    /// - For inbound payments, this fee is encoded in the invoice route hints
    ///   (as part of the `RoutingFees` struct)
    /// - Also used to estimate how much can be sent to another Lexe user.
    // compat: alias added in {node,lsp}-v0.7.0
    #[serde(rename = "base_msat", alias = "lsp_usernode_base_fee_msat")]
    pub lsp_usernode_base_fee_msat: u32,
    /// LSP's configured prop fee for forwarding over LSP -> User channels.
    ///
    /// - For inbound payments, this fee is encoded in the invoice route hints
    ///   (as part of the `RoutingFees` struct)
    /// - Also used to estimate how much can be sent to another Lexe user.
    // compat: alias added in {node,lsp}-v0.7.0
    #[serde(
        rename = "proportional_millionths",
        alias = "lsp_usernode_prop_fee_ppm"
    )]
    pub lsp_usernode_prop_fee_ppm: u32,

    // -- LSP -> External fees -- //
    /// LSP's configured prop fee for forwarding over LSP -> External channels.
    pub lsp_external_prop_fee_ppm: u32,
    /// LSP's configured base fee for forwarding over LSP -> External channels.
    pub lsp_external_base_fee_msat: u32,

    // -- RouteHintHop fields -- //
    pub cltv_expiry_delta: u16,
    pub htlc_minimum_msat: u64,
    pub htlc_maximum_msat: u64,
}

/// Information about Lexe's LSP's fees.
// TODO(max): It would be nice if these were included in `LspInfo` as
// `LspInfo::lsp_fees` with `#[serde(flatten)]` for forward compatibility,
// but this struct uses newtypes, so we'd have to write some custom serde
// attributes to (de)serialize as msat / ppm to make that work.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct LspFees {
    /// The Lsp -> User base fee as an [`Amount`].
    pub lsp_usernode_base_fee: Amount,
    /// The Lsp -> User prop fee as a [`Decimal`], i.e. ppm / 1_000_000.
    pub lsp_usernode_prop_fee: Decimal,
    /// The Lsp -> External base fee as an [`Amount`].
    pub lsp_external_base_fee: Amount,
    /// The Lsp -> External prop fee as a [`Decimal`], i.e. ppm / 1_000_000.
    pub lsp_external_prop_fee: Decimal,
}

/// Configuration info relating to Google OAuth2. When combined with an auth
/// `code`, can be used to obtain a GDrive access token and refresh token.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct OAuthConfig {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_id: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_secret: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub redirect_uri: String,
}

// --- impl LspInfo --- //

impl LspInfo {
    /// Get the [`LspFees`] from this [`LspInfo`].
    pub fn lsp_fees(&self) -> LspFees {
        let lsp_usernode_base_fee =
            Amount::from_msat(u64::from(self.lsp_usernode_base_fee_msat));
        let lsp_usernode_prop_fee =
            Decimal::from(self.lsp_usernode_prop_fee_ppm)
                .checked_div(dec!(1_000_000))
                .expect("Can't overflow because divisor is > 1");

        let lsp_external_base_fee =
            Amount::from_msat(u64::from(self.lsp_external_base_fee_msat));
        let lsp_external_prop_fee =
            Decimal::from(self.lsp_external_prop_fee_ppm)
                .checked_div(dec!(1_000_000))
                .expect("Can't overflow because divisor is > 1");

        LspFees {
            lsp_usernode_base_fee,
            lsp_usernode_prop_fee,
            lsp_external_base_fee,
            lsp_external_prop_fee,
        }
    }

    /// Returns a dummy [`LspInfo`] which can be used in tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn dummy() -> Self {
        use std::net::Ipv6Addr;

        use crate::{rng::FastRng, root_seed::RootSeed, test_utils};

        let mut rng = FastRng::from_u64(20230216);
        let node_pk = RootSeed::from_rng(&mut rng).derive_node_pk(&mut rng);
        let addr = LxSocketAddress::TcpIpv6 {
            ip: Ipv6Addr::LOCALHOST,
            port: 42069,
        };

        Self {
            node_api_url: Some(test_utils::DUMMY_LSP_URL.to_owned()),
            node_pk,
            private_p2p_addr: addr,
            lsp_usernode_base_fee_msat: 0,
            lsp_usernode_prop_fee_ppm: 4250,
            lsp_external_base_fee_msat: 0,
            lsp_external_prop_fee_ppm: 750,
            cltv_expiry_delta: 72,
            htlc_minimum_msat: 1,
            htlc_maximum_msat: u64::MAX,
        }
    }
}

impl FromStr for LspInfo {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).context("Invalid JSON")
    }
}

impl Display for LspInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json_str = serde_json::to_string(&self)
            .expect("Does not contain map with non-string keys");
        write!(f, "{json_str}")
    }
}

// --- impl OAuthConfig --- //

impl fmt::Debug for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let client_id = &self.client_id;
        let redirect_uri = &self.redirect_uri;
        write!(
            f,
            "OAuthConfig {{ \
                client_id: {client_id}, \
                redirect_uri: {redirect_uri}, \
                .. \
            }}"
        )
    }
}

impl FromStr for OAuthConfig {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s).context("Invalid JSON")
    }
}

impl Display for OAuthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json_str = serde_json::to_string(&self)
            .expect("Does not contain map with non-string keys");
        write!(f, "{json_str}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn lsp_info_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<LspInfo>();
        roundtrip::fromstr_display_roundtrip_proptest::<LspInfo>();
    }

    #[test]
    fn oauth_config_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<OAuthConfig>();
        roundtrip::fromstr_display_roundtrip_proptest::<OAuthConfig>();
    }
}
