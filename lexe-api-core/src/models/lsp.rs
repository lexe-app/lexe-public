use lexe_common::{
    api::user::{NodePk, Scid},
    ln::amount::Amount,
};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// A list of the LSP's [`ExternalChannel`]s.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GetExternalChannelsResponse {
    pub channels: Vec<ExternalChannel>,
}

/// A usable, announced, external LSP channel.
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExternalChannel {
    /// The channel's real (non-alias) scid.
    pub scid: Scid,
    pub counterparty_node_id: NodePk,
    /// The largest HTLC the LSP can currently send over this channel.
    pub next_outbound_htlc_limit: Amount,
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn get_external_channels_response_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<GetExternalChannelsResponse>(
        );
    }
}
