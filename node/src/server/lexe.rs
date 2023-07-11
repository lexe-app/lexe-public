use common::{
    api::{
        command::OpenChannelRequest, error::NodeApiError, qs::GetByUserPk,
        UserPk,
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
) -> Result<String, NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        // TODO Actually get status
        Ok(String::from("OK"))
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}

pub async fn open_channel(
    req: OpenChannelRequest,
    channel_manager: NodeChannelManager,
    peer_manager: NodePeerManager,
    lsp_channel_peer: ChannelPeer,
) -> anyhow::Result<()> {
    cfg_if::cfg_if! {
        // TODO(max): This needs to switch to #[cfg(feature = "test-utils")],
        // otherwise this will break the SGX integration tests.
        if #[cfg(any(test, not(target_env = "sgx")))] {
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
) -> Result<(), NodeApiError> {
    let given_pk = given.user_pk;
    if current_pk == given_pk {
        shutdown.send();
        Ok(())
    } else {
        Err(NodeApiError::wrong_user_pk(current_pk, given_pk))
    }
}