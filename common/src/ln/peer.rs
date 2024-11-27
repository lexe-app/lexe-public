use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::Context;
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{api::user::NodePk, ln::addr::LxSocketAddress};

/// Represents a Lightning Network peer, which we might have a channel with.
#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct LnPeer {
    pub node_pk: NodePk,
    pub addr: LxSocketAddress,
}

/// `<node_pk>@<addr>`
impl FromStr for LnPeer {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        let (node_pk_str, addr_str) = s
            .split_once('@')
            .context("no '@' separator. missing socket addr or node pk.")?;

        let node_pk =
            NodePk::from_str(node_pk_str).context("Invalid node public key")?;
        let addr = LxSocketAddress::from_str(addr_str)
            .context("Invalid socket address")?;

        Ok(Self { node_pk, addr })
    }
}

/// `<node_pk>@<addr>`
impl Display for LnPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let node_pk = &self.node_pk;
        let addr = &self.addr;
        write!(f, "{node_pk}@{addr}")
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn test_basic() {
        let assert_fromstr = |s| LnPeer::from_str(s).unwrap();
        let assert_json = |s| serde_json::from_str::<LnPeer>(s).unwrap();

        assert_fromstr("024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8@1.2.3.4:5050");
        assert_fromstr("024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8@[2600:1700::a2c2:d3f1]:5050");
        assert_fromstr("024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8@lsp.lexe.app:5050");

        assert_json(
            r#"{
                "node_pk": "024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8",
                "addr": "1.2.3.4:5050"
            }"#,
        );
        assert_json(
            r#"{
                "node_pk": "024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8",
                "addr": "[2600:1700::a2c2:d3f1]:5050"
            }"#,
        );
        assert_json(
            r#"{
                "node_pk": "024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8",
                "addr": "lsp.lexe.app:5050"
            }"#,
        );
    }

    #[test]
    fn test_from_str_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LnPeer>();
    }

    #[test]
    fn test_json_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<LnPeer>();
    }
}
