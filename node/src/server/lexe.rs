use common::{
    api::{
        command::OpenChannelRequest, error::NodeApiError, qs::GetByUserPk,
        Empty, UserPk,
    },
    ln::peer::ChannelPeer,
    shutdown::ShutdownChannel,
};

use crate::{
    channel_manager::NodeChannelManager, peer_manager::NodePeerManager,
};

pub async fn status(
    given: GetByUserPk,
    current_pk: UserPk,
) -> Result<Empty, NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        Ok(Empty {})
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}

pub async fn open_channel(
    req: OpenChannelRequest,
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    lsp_channel_peer: ChannelPeer,
) -> anyhow::Result<Empty> {
    cfg_if::cfg_if! {
        if #[cfg(any(test, feature = "test-utils"))] {
            use anyhow::Context;
            use common::rng::SysRng;
            use lexe_ln::{channel, channel::ChannelRelationship};
            use crate::channel_manager;

            let mut rng = SysRng::new();
            let user_channel_id = channel::get_random_u128(&mut rng);
            let relationship =
                ChannelRelationship::UserToLsp { lsp_channel_peer };
            lexe_ln::channel::open_channel(
                channel_manager,
                peer_manager,
                user_channel_id,
                req.value,
                relationship,
                channel_manager::USER_CONFIG,
            )
            .await
            .context("Failed to open channel to LSP")
        } else {
            let _ = req;
            let _ = channel_manager;
            let _ = peer_manager;
            let _ = lsp_channel_peer;
            anyhow::bail!("This endpoint is disabled in staging/prod");
        }
    }
}

pub fn shutdown(
    given: GetByUserPk,
    current_pk: UserPk,
    shutdown: ShutdownChannel,
) -> Result<Empty, NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        shutdown.send();
        Ok(Empty {})
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}
