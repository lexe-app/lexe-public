use std::{fmt, fmt::Display, path::Path, process::Command, str::FromStr};

use anyhow::Context;
use lightning::routing::{gossip::RoutingFees, router::RouteHintHop};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::user::{NodePk, Scid},
    ln::addr::LxSocketAddress,
};

/// User node CLI args.
pub mod node;

/// A trait for converting CLI args to [`Command`]s.
pub trait ToCommand {
    /// Construct a [`Command`] from the contained args.
    /// Requires the path to the binary.
    fn to_command(&self, bin_path: &Path) -> Command {
        let mut command = Command::new(bin_path);
        self.append_args(&mut command);
        command
    }

    /// Serialize and append the contained args to an existing [`Command`].
    fn append_args(&self, cmd: &mut Command);
}

/// Information about the LSP which the user node needs to connect and to
/// generate route hints when no channel exists.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LspInfo {
    /// The protocol://host:port of the LSP's HTTP server. The node will
    /// default to a mock client if not supplied, provided that
    /// `--allow-mock` is set and we are not in prod.
    #[serde(rename = "url")] // Original name needed for forward compat
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub node_api_url: Option<String>,
    pub node_pk: NodePk,
    /// The socket on which the LSP accepts P2P LN connections from user nodes
    #[serde(rename = "addr")] // Original name needed for forward compat
    pub private_p2p_addr: LxSocketAddress,
    // - RoutingFees fields - //
    pub base_msat: u32,
    pub proportional_millionths: u32,
    // - RouteHintHop fields - //
    pub cltv_expiry_delta: u16,
    pub htlc_minimum_msat: u64,
    pub htlc_maximum_msat: u64,
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
    pub fn route_hint_hop(&self, scid: Scid) -> RouteHintHop {
        RouteHintHop {
            src_node_id: self.node_pk.0,
            short_channel_id: scid.0,
            fees: RoutingFees {
                base_msat: self.base_msat,
                proportional_millionths: self.proportional_millionths,
            },
            cltv_expiry_delta: self.cltv_expiry_delta,
            htlc_minimum_msat: Some(self.htlc_minimum_msat),
            htlc_maximum_msat: Some(self.htlc_maximum_msat),
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
            base_msat: 0,
            proportional_millionths: 3000,
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
