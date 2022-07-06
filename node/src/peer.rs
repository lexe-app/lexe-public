use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bitcoin::secp256k1::PublicKey;

use crate::types::PeerManagerType;

#[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
pub async fn connect_peer_if_necessary(
    pubkey: PublicKey,
    peer_addr: SocketAddr,
    peer_manager: Arc<PeerManagerType>,
) -> Result<(), ()> {
    for node_pubkey in peer_manager.get_peer_node_ids() {
        if node_pubkey == pubkey {
            return Ok(());
        }
    }
    let res = do_connect_peer(pubkey, peer_addr, peer_manager).await;
    if res.is_err() {
        println!("ERROR: failed to connect to peer");
    }
    res
}

pub async fn do_connect_peer(
    pubkey: PublicKey,
    peer_addr: SocketAddr,
    peer_manager: Arc<PeerManagerType>,
) -> Result<(), ()> {
    match lightning_net_tokio::connect_outbound(
        Arc::clone(&peer_manager),
        pubkey,
        peer_addr,
    )
    .await
    {
        Some(connection_closed_future) => {
            let mut connection_closed_future =
                Box::pin(connection_closed_future);
            loop {
                match futures::poll!(&mut connection_closed_future) {
                    std::task::Poll::Ready(_) => {
                        return Err(());
                    }
                    std::task::Poll::Pending => {}
                }
                // Avoid blocking the tokio context by sleeping a bit
                match peer_manager
                    .get_peer_node_ids()
                    .iter()
                    .find(|id| **id == pubkey)
                {
                    Some(_) => return Ok(()),
                    None => tokio::time::sleep(Duration::from_millis(10)).await,
                }
            }
        }
        None => Err(()),
    }
}
