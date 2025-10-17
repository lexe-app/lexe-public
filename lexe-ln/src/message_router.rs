use std::sync::Arc;

use bitcoin::secp256k1;
use common::{cli::LspInfo, ln::addr::LxSocketAddress, rng::SysRngDerefHack};
#[cfg(doc)]
use lightning::ln::msgs::OnionMessage;
use lightning::{
    blinded_path::{
        IntroductionNode,
        message::{BlindedMessagePath, MessageContext, MessageForwardNode},
    },
    ln::msgs::SocketAddress,
    onion_message::messenger::{Destination, MessageRouter, OnionMessagePath},
    routing::gossip::NodeId,
};

use crate::alias::NetworkGraphType;

/// The Lexe [`MessageRouter`] impl for both the Lexe LSP and our user nodes.
/// This type is responsible for routing [`OnionMessage`]s and building blinded
/// paths back to us.
///
/// Ideally we would have separate structs for LSP and user node, but the
/// resulting generics hell is absolutely not worth it.
pub struct LexeMessageRouter {
    network_graph: Arc<NetworkGraphType>,
    kind: Kind,
}

enum Kind {
    /// User nodes are not public nodes and only peer with the Lexe LSP. When
    /// building blinded paths back to themselves, the blinded path is just
    /// LSP -> node.
    Node {
        lsp_info: LspInfo,
    },
    Lsp,
}

impl LexeMessageRouter {
    /// Create a new [`LexeMessageRouter`] for user nodes.
    pub fn new_user_node(
        network_graph: Arc<NetworkGraphType>,
        lsp_info: LspInfo,
    ) -> Self {
        Self {
            network_graph,
            kind: Kind::Node { lsp_info },
        }
    }

    /// Create a new [`LexeMessageRouter`] for LSP.
    pub fn new_lsp(network_graph: Arc<NetworkGraphType>) -> Self {
        Self {
            network_graph,
            kind: Kind::Lsp,
        }
    }
}

impl MessageRouter for LexeMessageRouter {
    // Find a path from `sender` to `destination`. Doesn't need full payment
    // routing, since we're just sending an onion message across the network.
    //
    // For the LSP, a path to an indirect external node or offline user node
    // will probably surface as a `ConnectionNeeded` event in its
    // `OnionMessageHandler`.
    fn find_path(
        &self,
        sender: secp256k1::PublicKey,
        peers: Vec<secp256k1::PublicKey>,
        mut destination: Destination,
    ) -> Result<OnionMessagePath, ()> {
        let network_graph = self.network_graph.read_only();

        // resolve any directed SCID hops to plain node pks
        destination.resolve(&network_graph);

        // if the destination is a blinded path, this is the first node on the
        // path, otherwise it's just the node itself.
        let intro_node = destination.introduction_node().ok_or(())?;

        // If the first node is a direct peer or the sender itself,
        // we can route directly to it without any intermediate hops or lazy
        // connect.
        if peers.contains(intro_node) || &sender == intro_node {
            return Ok(OnionMessagePath {
                intermediate_nodes: vec![],
                destination,
                first_node_addresses: None,
            });
        }

        match &self.kind {
            // For user nodes, always route through LSP
            Kind::Node { lsp_info } => {
                let lsp_node_pk = lsp_info.node_pk.as_inner();

                // Check if the destination is the LSP itself
                if intro_node == lsp_node_pk {
                    // Direct route to LSP, no intermediate hops needed
                    Ok(OnionMessagePath {
                        intermediate_nodes: vec![],
                        destination,
                        first_node_addresses: None,
                    })
                } else {
                    // TODO(phlip9): user nodes could reject an intro_node that
                    // doesn't have announced ip/hostname addresses or onion
                    // message support (from its view of the network graph).
                    //
                    // This might reduce work for the LSP and/or give an nicer
                    // error message.
                    //
                    // Might be more trouble / brittleness than it's worth tho.

                    // For all other destinations, route through the LSP as the
                    // single intermediate hop. The LSP should then lazily
                    // connect to `intro_node` (if not already connected).
                    Ok(OnionMessagePath {
                        intermediate_nodes: vec![*lsp_node_pk],
                        destination,
                        first_node_addresses: None,
                    })
                }
            }
            // For the LSP, we need to support `find_path` to both external
            // nodes and user nodes, but we can't check if the
            // destination is a user node here (requires async).
            Kind::Lsp => {
                match network_graph.node(&NodeId::from_pubkey(intro_node)) {
                    // This is an external node in the network graph. We can
                    // only route to them if they support onion messages and
                    // have an announced address that we support.
                    Some(node_info) => node_info
                        .announcement_info
                        .as_ref()
                        .and_then(|announce| {
                            // external intro_node must support onion messages
                            let supports_om =
                                announce.features().supports_onion_messages();
                            if !supports_om {
                                return None;
                            }

                            // only allow supported addresses (i.e., no TOR
                            // onion addresses)
                            let addrs = announce
                                .addresses()
                                .iter()
                                .filter_map(|addr| {
                                    LxSocketAddress::try_from(addr.clone())
                                        .ok()
                                        .map(SocketAddress::from)
                                })
                                .collect::<Vec<SocketAddress>>();

                            if !addrs.is_empty() { Some(addrs) } else { None }
                        })
                        .map(|addrs| OnionMessagePath {
                            intermediate_nodes: vec![],
                            destination,
                            first_node_addresses: Some(addrs),
                        })
                        .ok_or(()),
                    // This could either be (1) an unannounced external node or
                    // (2) a user node (should never be in the network graph).
                    // Ideally we could reject (1) here and allow (2), but we'll
                    // have to let our `OnionMessenger` handle that.
                    None => Ok(OnionMessagePath {
                        intermediate_nodes: vec![],
                        destination,
                        first_node_addresses: None,
                    }),
                }
            }
        }
    }

    // Create a blinded _onion message_ path for an external onion message
    // sender to use as the last hops when routing a message to us.
    fn create_blinded_paths<T: secp256k1::Signing + secp256k1::Verification>(
        &self,
        recipient: secp256k1::PublicKey,
        context: MessageContext,
        _peers: Vec<secp256k1::PublicKey>,
        secp_ctx: &secp256k1::Secp256k1<T>,
    ) -> Result<Vec<BlindedMessagePath>, ()> {
        // LDK default `create_blinded_paths` tries to be too smart and breaks
        // in our smoketests. It can't build a blinded path b/c the LSP peer
        // isn't sufficiently connected, as there are no other public nodes.
        //
        // We have very simple useage regardless.
        // - The LSP is a public node, so doesn't need any privacy.
        // - User nodes are always connected to the LSP, so we can just
        //   unconditionally return a blinded path from LSP -> User node.
        let result = match &self.kind {
            // Node => Always return a single blinded path: LSP -> User node
            Kind::Node { lsp_info } => BlindedMessagePath::new(
                // LSP is the introductory node
                &[MessageForwardNode {
                    node_id: lsp_info.node_pk.inner(),
                    short_channel_id: None,
                }],
                recipient,
                context,
                SysRngDerefHack::new(),
                secp_ctx,
            ),
            // LSP doesn't need privacy. Just return a "blinded path" to
            // with itself as the introductory node.
            Kind::Lsp => BlindedMessagePath::one_hop(
                recipient,
                context,
                SysRngDerefHack::new(),
                secp_ctx,
            ),
        };

        result.map(|path| vec![path])
    }

    // `create_compact_blinded_paths` just uses `create_blinded_paths`.
    //
    // TODO(phlip9): would `create_compact_blinded_paths` work for user
    // nodes? We could return a compact blinded path that uses SCIDs and
    // not full NodePk's, which would make our BOLT12 offer codes much more
    // compact.
    //
    // fn create_compact_blinded_paths<
    //     T: secp256k1::Signing + secp256k1::Verification,
    // >(
    //     &self,
    //     recipient: secp256k1::PublicKey,
    //     context: MessageContext,
    //     peers: Vec<lightning::blinded_path::message::MessageForwardNode>,
    //     secp_ctx: &secp256k1::Secp256k1<T>,
    // ) -> Result<Vec<BlindedMessagePath>, ()> {
    //     todo!()
    // }
}

trait DestinationExt {
    /// Returns the introduction node of this [`Destination`], if it is a direct
    /// [`Destination::Node`] or a [`Destination::BlindedPath`] with a direct
    /// introduction node (i.e., not directed SCID).
    fn introduction_node(&self) -> Option<&secp256k1::PublicKey>;
}

impl DestinationExt for Destination {
    fn introduction_node(&self) -> Option<&secp256k1::PublicKey> {
        match self {
            Destination::Node(node_pk) => Some(node_pk),
            Destination::BlindedPath(path) => match path.introduction_node() {
                IntroductionNode::NodeId(node_pk) => Some(node_pk),
                // This case should already be resolved to a `NodeId` by
                // `destination.resolve()` if this channel actually exists.
                IntroductionNode::DirectedShortChannelId(..) => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use bitcoin::secp256k1::PublicKey;
    use common::{
        api::user::NodePk,
        cli::LspInfo,
        ln::addr::LxSocketAddress,
        rng::{Crng, FastRng},
    };
    use lightning::{
        blinded_path::message::OffersContext,
        ln::msgs::{SocketAddress, UnsignedNodeAnnouncement},
        offers::nonce::Nonce,
        onion_message::messenger::Destination,
        routing::gossip::NodeId,
        types::features::{ChannelFeatures, NodeFeatures},
    };

    use super::*;
    use crate::logger::LexeTracingLogger;

    #[test]
    fn test_find_path_user_node() {
        let network_graph = dummy_network_graph();
        let lsp_info = dummy_lsp_info();
        let lsp_pk = lsp_info.node_pk.inner();
        let external_pk = dummy_external_pk_1();
        let user_pk = dummy_user_pk();

        let router = LexeMessageRouter::new_user_node(network_graph, lsp_info);

        // Test user routing directly to LSP node
        let sender = user_pk;
        let peers = vec![lsp_pk];
        let destination = Destination::Node(lsp_pk);
        let path = router.find_path(sender, peers, destination).unwrap();
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![],
                destination: Destination::Node(lsp_pk),
                first_node_addresses: None,
            },
        );

        // Test user routing to other node via LSP
        let sender = user_pk;
        let peers = vec![lsp_pk];
        let destination = Destination::Node(external_pk);
        let path = router.find_path(sender, peers, destination).unwrap();
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![lsp_pk],
                destination: Destination::Node(external_pk),
                first_node_addresses: None,
            },
        );
    }

    #[test]
    fn test_find_path_lsp() {
        let network_graph = dummy_network_graph();
        let lsp_info = dummy_lsp_info();
        let lsp_pk = lsp_info.node_pk.inner();
        let external_pk = dummy_external_pk_1();
        let user_pk = dummy_user_pk();

        let router = LexeMessageRouter::new_lsp(network_graph.clone());

        // Test LSP routing to direct, connected external peer
        let sender = lsp_pk;
        let peers = vec![external_pk];
        let destination = Destination::Node(external_pk);
        let path = router.find_path(sender, peers, destination).unwrap();
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![],
                destination: Destination::Node(external_pk),
                first_node_addresses: None,
            },
        );

        // Test LSP routing to indirect external peer (in network graph but not
        // direct peer)
        let sender = lsp_pk;
        let peers = vec![user_pk];
        let destination = Destination::Node(external_pk);
        let path = router.find_path(sender, peers, destination).unwrap();
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![],
                destination: Destination::Node(external_pk),
                // Should lazy connect to indirect external peer
                first_node_addresses: Some(dummy_supported_external_addrs()),
            },
        );

        // Test LSP routing to disconnected user node (not in network graph) via
        // immediate user node destination
        let sender = lsp_pk;
        let peers = vec![external_pk];
        let destination = Destination::Node(user_pk);
        let path = router.find_path(sender, peers, destination).unwrap();
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![],
                destination: Destination::Node(user_pk),
                first_node_addresses: None,
            },
        );

        // Test routing to disconnected user node via blinded path (LSP is the
        // intro_node).
        let user_router = LexeMessageRouter::new_user_node(
            network_graph.clone(),
            lsp_info.clone(),
        );
        let mut rng = Box::new(FastRng::from_u64(12354654));
        let secp_ctx = rng.gen_secp256k1_ctx();
        let nonce = Nonce::from_entropy_source(rng);
        let msg_ctx =
            MessageContext::Offers(OffersContext::InvoiceRequest { nonce });
        let peers = vec![lsp_pk];
        let blinded_path = user_router
            .create_blinded_paths(dummy_user_pk(), msg_ctx, peers, &secp_ctx)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let sender = lsp_pk;
        let peers = vec![external_pk];
        let destination = Destination::BlindedPath(blinded_path);
        let path = router
            .find_path(sender, peers, destination.clone())
            .unwrap();
        assert_eq!(destination.introduction_node(), Some(&lsp_pk));
        assert_path_eq(
            &path,
            &OnionMessagePath {
                intermediate_nodes: vec![],
                destination,
                first_node_addresses: None,
            },
        );
    }

    // --- helpers --- //

    #[track_caller]
    fn assert_path_eq(actual: &OnionMessagePath, expected: &OnionMessagePath) {
        assert_eq!(
            actual.intermediate_nodes, expected.intermediate_nodes,
            "Intermediate nodes do not match"
        );
        assert_eq!(
            actual.destination, expected.destination,
            "Destination does not match"
        );
        assert_eq!(
            actual.first_node_addresses, expected.first_node_addresses,
            "First node addresses do not match"
        );
    }

    // Create a dummy network graph with a single external peer.
    fn dummy_network_graph() -> Arc<NetworkGraphType> {
        let network_graph = NetworkGraphType::new(
            bitcoin::Network::Regtest,
            LexeTracingLogger::new(),
        );
        let external_pk = dummy_external_pk_1();

        // Add a channel between external peer and some dummy node. This will
        // add both nodes to the network graph so the graph will accept the
        // following node announcement.
        network_graph
            .add_channel_from_partial_announcement(
                12345,      // scid
                1234567890, // timestamp
                ChannelFeatures::empty(),
                external_pk,
                dummy_external_pk_2(),
            )
            .unwrap();

        // Now add node announcement for external peer with dummy data
        let mut node_features = NodeFeatures::empty();
        node_features.set_onion_messages_optional();
        let unsigned_announcement = UnsignedNodeAnnouncement {
            features: node_features,
            timestamp: 20190119,
            node_id: NodeId::from_pubkey(&external_pk),
            rgb: [0, 255, 0],
            alias: lightning::routing::gossip::NodeAlias(
                *b"external_test_node\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            ),
            addresses: dummy_external_addrs(),
            excess_address_data: Vec::new(),
            excess_data: Vec::new(),
        };

        network_graph
            .update_node_from_unsigned_announcement(&unsigned_announcement)
            .unwrap();

        Arc::new(network_graph)
    }

    fn dummy_lsp_info() -> LspInfo {
        let s = "02deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
        LspInfo {
            node_pk: NodePk(PublicKey::from_str(s).unwrap()),
            private_p2p_addr: LxSocketAddress::from_str("127.0.0.1:9735")
                .unwrap(),
            lsp_usernode_base_fee_msat: 1000,
            lsp_usernode_prop_fee_ppm: 100,
            lsp_external_prop_fee_ppm: 200,
            lsp_external_base_fee_msat: 2000,
            cltv_expiry_delta: 144,
            htlc_minimum_msat: 1000,
            htlc_maximum_msat: 1_000_000_000,
        }
    }

    fn dummy_external_pk_1() -> PublicKey {
        let s = "03cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebab3";
        PublicKey::from_str(s).unwrap()
    }

    fn dummy_external_pk_2() -> PublicKey {
        let s = "02dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        PublicKey::from_str(s).unwrap()
    }

    fn dummy_user_pk() -> PublicKey {
        let s = "03cafebabecafebabecafebabecafebabecafebabecafebabecafebabecafebab9";
        PublicKey::from_str(s).unwrap()
    }

    fn dummy_supported_external_addrs() -> Vec<SocketAddress> {
        vec![SocketAddress::TcpIpV4 {
            addr: [10, 0, 0, 69],
            port: 9735,
        }]
    }
    fn dummy_external_addrs() -> Vec<SocketAddress> {
        vec![
            SocketAddress::TcpIpV4 {
                addr: [10, 0, 0, 69],
                port: 9735,
            },
            SocketAddress::OnionV2([0x42; 12]),
        ]
    }
}
