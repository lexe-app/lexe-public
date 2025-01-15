use std::{future::Future, time::Duration};

use anyhow::{anyhow, ensure, Context};
use common::{api::user::NodePk, backoff, ln::addr::LxSocketAddress, Apply};
use lightning_net_tokio::Executor;
use tokio::{net::TcpStream, time};
use tracing::{debug, warn};

use crate::traits::{LexeChannelManager, LexePeerManager, LexePersister};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The maximum amount of time we'll allow LDK to complete the P2P handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// An [`Executor`] which propagates [`tracing`] spans.
#[derive(Copy, Clone)]
pub struct TracingExecutor;

impl<F> Executor<F> for TracingExecutor
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    fn execute(&self, fut: F) -> tokio::task::JoinHandle<F::Output> {
        // XXX(max): Currently using `spawn_blocking` to hack around a prod
        // deadlock; change this back to tokio::spawn when possible
        let instrumented = tracing::Instrument::in_current_span(fut);
        tokio::task::spawn_blocking(|| {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(instrumented)
        })

        /*
        #[allow(clippy::disallowed_methods)] // Have to return `JoinHandle` here
        tokio::spawn(tracing::Instrument::in_current_span(fut))
        */
    }
}

/// Connects to a LN peer, returning early if we were already connected.
/// Cycles through the given addresses until we run out of connect attempts.
pub async fn connect_peer_if_necessary<CM, PM, PS>(
    peer_manager: &PM,
    node_pk: &NodePk,
    addrs: &[LxSocketAddress],
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    ensure!(!addrs.is_empty(), "No addrs were provided");

    // Early return if we're already connected
    // TODO(max): LDK's fn is O(n) in the # of peers...
    if peer_manager.is_connected(node_pk) {
        return Ok(());
    }

    // Cycle the given addresses in order
    let mut addrs = addrs.iter().cycle();

    // Retry at least a couple times to mitigate an outbound connect race
    // between the reconnector and open_channel which has been observed.
    let retries = 5;
    for _ in 0..retries {
        let addr = addrs.next().expect("Cycling through a non-empty slice");

        match do_connect_peer(peer_manager, node_pk, addr).await {
            Ok(()) => return Ok(()),
            Err(e) => warn!("Failed to connect to peer: {e:#}"),
        }

        // Connect failed; sleep 500ms before the next attempt to give LDK some
        // time to complete the noise / LN handshake. We do NOT need to add a
        // random jitter because LDK's PeerManager already tiebreaks outbound
        // connect races by failing the later attempt.
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Right before the next attempt, check again whether we're connected in
        // case another task managed to connect while we were sleeping.
        if peer_manager.is_connected(node_pk) {
            return Ok(());
        }
    }

    // Do the last attempt.
    let addr = addrs.next().expect("Cycling through a non-empty slice");
    do_connect_peer(peer_manager, node_pk, addr)
        .await
        .context("Failed to connect to peer")?;

    Ok(())
}

async fn do_connect_peer<CM, PM, PS>(
    peer_manager: &PM,
    node_pk: &NodePk,
    addr: &LxSocketAddress,
) -> anyhow::Result<()>
where
    CM: LexeChannelManager<PS>,
    PM: LexePeerManager<CM, PS>,
    PS: LexePersister,
{
    debug!(%node_pk, %addr, "Starting do_connect_peer");

    // TcpStream::connect takes a `String` in SGX.
    let addr_str = addr.to_string();
    let stream = TcpStream::connect(addr_str)
        .apply(|fut| time::timeout(CONNECT_TIMEOUT, fut))
        .await
        .context("Connect request timed out")?
        .context("TcpStream::connect() failed")?
        .into_std()
        .context("Couldn't convert to std TcpStream")?;

    // NOTE: `setup_outbound()` returns a future which completes when the
    // connection closes, which we do not need to poll because a task was
    // spawned for it. However, in the case of an error, the future returned
    // by `setup_outbound()` completes immediately, and does not propagate
    // the error from `peer_manager.new_outbound_connection()`. So, in order
    // to check that there was no error while establishing the connection we
    // have to manually poll the future, and if it completed, return an
    // error (which we don't have access to because `lightning-net-tokio`
    // failed to surface it to us).
    //
    // On the other hand, since LDK's API doesn't let you know when the
    // connection is established, you have to keep calling
    // `peer_manager.get_peer_node_ids()` to see if the connection has been
    // registered yet.
    //
    // TODO: Rewrite / replace lightning-net-tokio entirely
    let connection_closed_fut =
        lightning_net_tokio::setup_outbound_with_executor(
            TracingExecutor,
            peer_manager.clone(),
            node_pk.0,
            stream,
        );
    tokio::pin!(connection_closed_fut);

    // A future which completes iff the noise handshake successfully completes.
    let handshake_complete_fut = async {
        let mut backoff_durations = backoff::iter_with_initial_wait_ms(10);
        loop {
            tokio::time::sleep(backoff_durations.next().unwrap()).await;

            debug!("Checking peer_manager.is_connected()");
            if peer_manager.is_connected(node_pk) {
                debug!(%node_pk, %addr, "Successfully connected to peer");
                return;
            }
        }
    };
    tokio::pin!(handshake_complete_fut);

    tokio::select! {
        () = handshake_complete_fut => {
            debug!(%node_pk, %addr, "Successfully connected to peer");
            // TODO(max): Maybe should return the task handle here so we can
            // propagate any panics without panic=abort (See `bb1f25e1`)
            Ok(())
        }
        () = &mut connection_closed_fut => {
            // TODO(max): Patch lightning-net-tokio so the error is exposed
            let msg = "Failed connection to peer (error unknown)";
            warn!("{msg}"); // Also log; this code is historically finicky
            Err(anyhow!("{msg}"))
        }
        _ = tokio::time::sleep(HANDSHAKE_TIMEOUT) => {
            let msg = "Timed out waiting for noise handshake to complete";
            warn!(%node_pk, %addr, "{msg}");
            Err(anyhow!("{msg}: {node_pk}@{addr}"))
        }
    }
}
