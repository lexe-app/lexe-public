use std::sync::Arc;

use bitcoin::secp256k1;
use common::{cli::LspInfo, rng::SysRngDerefHack};
#[cfg(doc)]
use lightning::ln::msgs::OnionMessage;
use lightning::{
    blinded_path::message::{
        BlindedMessagePath, MessageContext, MessageForwardNode,
    },
    onion_message::messenger::{
        DefaultMessageRouter, Destination, MessageRouter, OnionMessagePath,
    },
};

use crate::{alias::NetworkGraphType, logger::LexeTracingLogger};

/// The default LDK [`MessageRouter`] impl with concrete Lexe types filled in.
type DefaultMessageRouterType = DefaultMessageRouter<
    Arc<NetworkGraphType>,
    LexeTracingLogger,
    SysRngDerefHack,
>;

/// The Lexe [`MessageRouter`] impl for both the Lexe LSP and our user nodes.
/// This type is responsible for routing [`OnionMessage`]s and building blinded
/// paths back to us.
///
/// Ideally these variants would be separated into different impls, but the
/// resulting generics hell is absolutely not worth it.
pub enum LexeMessageRouter {
    /// An LDK [`MessageRouter`] specialized for user nodes. This is resonsible
    /// for routing [`OnionMessage`]s, to destinations and building blinded
    /// paths back to us.
    ///
    /// User nodes are not public nodes and only peer with the Lexe LSP. When
    /// building blinded paths back to themselves, the blinded path is just
    /// LSP -> node.
    Node {
        default_router: DefaultMessageRouterType,
        lsp_info: LspInfo,
    },
    /// LSP just uses the default LDK [`MessageRouter`] implementation.
    Lsp {
        default_router: DefaultMessageRouterType,
    },
}

impl LexeMessageRouter {
    /// Create a new [`LexeMessageRouter`] for user nodes.
    pub fn new_user_node(
        network_graph: Arc<NetworkGraphType>,
        lsp_info: LspInfo,
    ) -> Self {
        let rng = SysRngDerefHack::new();
        let default_router = DefaultMessageRouterType::new(network_graph, rng);
        Self::Node {
            default_router,
            lsp_info,
        }
    }

    /// Create a new [`LexeMessageRouter`] for LSP.
    pub fn new_lsp(network_graph: Arc<NetworkGraphType>) -> Self {
        let rng = SysRngDerefHack::new();
        let default_router = DefaultMessageRouterType::new(network_graph, rng);
        Self::Lsp { default_router }
    }

    fn default_router(&self) -> &DefaultMessageRouterType {
        match self {
            Self::Node { default_router, .. } => default_router,
            Self::Lsp { default_router } => default_router,
        }
    }
}

impl MessageRouter for LexeMessageRouter {
    // Find a path from `sender` to `destination`. Doesn't need full payment
    // routing, since we're just sending an onion message across the network.
    fn find_path(
        &self,
        sender: secp256k1::PublicKey,
        peers: Vec<secp256k1::PublicKey>,
        destination: Destination,
    ) -> Result<OnionMessagePath, ()> {
        // Delegate message path finding to default MessageRouter impl
        MessageRouter::find_path(
            self.default_router(),
            sender,
            peers,
            destination,
        )
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
        let result = match self {
            // Node => Always return a single blinded path: LSP -> User node
            Self::Node { lsp_info, .. } => BlindedMessagePath::new(
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
            Self::Lsp { .. } => BlindedMessagePath::one_hop(
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
