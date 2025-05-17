//! The core I/O logic for a single Lightning P2P connection and its interfacing
//! with the LDK [`PeerManager`].
//!
//! Open a new outbound connection to a peer with [`connect_peer_if_necessary`].
//!
//! Handle a newly accepted connection with [`spawn_inbound`].
//!
//! ## Design
//!
//! Each `Connection` task owns the underlying [`TcpStream`] and is responsible
//! for reading and writing bytes to and from that connection. It just handles
//! socket I/O and sees only opaque bytes. Newly read data is pumped into
//! [`PeerManager::read_event`]. The [`PeerManager`] writes to the connection
//! via `ConnectionTx::send_data`, which enqueues data packets onto the
//! associated mpsc channel. `Connection` later dequeues these data packets and
//! writes them to the [`TcpStream`].
//!
//! ### Read path
//!
//! ```text
//! TcpStream -> read -> Connection -> PeerManager::read_event()
//! ```
//!
//! Read backpressure is applied when [`PeerManager::read_event`] returns
//! `true`. We then wait for [`PeerManager`] to send a corresponding
//! `ConnectionTx::send_data` with `resume_read=true` to stop applying
//! read backpressure.
//!
//! After any [`PeerManager::read_event`], we're also responsible for calling
//! [`PeerManager::process_events`]. We use a separate, shared task
//! ([`spawn_process_events_task`]) that all `Connection` tasks can notify to
//! call [`PeerManager::process_events`].
//!
//! ### Write path
//!
//! ```text
//! PeerManager -> ConnectionTx::send_data -> mpsc channel -> Connection -> write -> TcpStream
//! ```
//!
//! Write backpressure is applied when we can't write quickly enough to the TCP
//! connection (e.g., remote peer is slow, connection is bad, we're overloaded).
//! When the mpsc channel to fills up, [`PeerManager`] observes the write
//! backpressure as `ConnectionTx::send_data` returning `0`.
//!
//! We call [`PeerManager::write_buffer_space_avail`] whenever there is empty
//! space in the mpsc channel for the [`PeerManager`] to fill.
//!
//! ### Disconnect
//!
//! When the connection disconnects, we call
//! [`PeerManager::socket_disconnected`].
//!
//! When the [`PeerManager`] wants to disconnect, it calls
//! `ConnectionTx::disconnect_socket`, which notifies the `Connection` task.
//!
//! There is no graceful shutdown; we just close the connection immediately when
//! we want to disconnect. Ideally we would send a Lightning protocol
//! `DisconnectPeerWithWarning` message or something, but LDK doesn't currently
//! do this. Graceful shutdown is also further complicated by SGX [`TcpStream`]
//! not supporting read/write TCP half-close.
//!
//! ### Why does this module exist
//!
//! LDK already provides a crate called
//! [`lightning-net-tokio`](https://docs.rs/lightning-net-tokio) that we used
//! previously. Unfortunately, we ran into some serious deadlocks and strange
//! behavior in SGX that was possibly related.
//!
//! With `lightning-net-tokio`, the [`TcpStream`] is shared; any task that calls
//! into [`PeerManager`] can `write` to the [`TcpStream`]. A [`TcpStream`] is
//! [not supposed to be shared/used between more than 2 tasks concurrently](https://github.com/tokio-rs/tokio/blob/eaf81ed324e7cca0fa9b497a6747746da37eea93/tokio/src/io/poll_evented.rs#L25-L30),
//! and we think the SGX async impl might be handling that poorly.
//! `lightning-net-tokio` also does some non-standard things with `Waker`s.
//!
//! This module instead has minimal sharing and no locking; communication
//! happens over a channel. The `TcpStream` has a single owner and only a single
//! task ever reads and writes to it. We think this is more likely to be
//! correct.
//!
//! The major downside with this approach is the additional layer of buffering
//! and unnecessary write-copy between the [`PeerManager`] and the connection.
//! There is also an extra task context switch on write, which adds some small
//! latency.
//!
//! [`ConnectionTx`]: crate::p2p::ConnectionTx
//! [`PeerManager::process_events`]: lightning::ln::peer_handler::PeerManager::process_events
//! [`PeerManager::read_event`]: lightning::ln::peer_handler::PeerManager::read_event
//! [`PeerManager::socket_disconnected`]: lightning::ln::peer_handler::PeerManager::socket_disconnected
//! [`PeerManager::write_buffer_space_avail`]: lightning::ln::peer_handler::PeerManager::write_buffer_space_avail
//! [`PeerManager`]: lightning::ln::peer_handler::PeerManager
//! [`TcpStream`]: tokio::net::TcpStream
//! [`connect_peer_if_necessary`]: crate::p2p::connect_peer_if_necessary
//! [`spawn_inbound`]: crate::p2p::spawn_inbound
//! [`spawn_process_events_task`]: crate::p2p::spawn_process_events_task

use std::{
    hash::Hash,
    io,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, ensure, Context};
use common::{api::user::NodePk, backoff, ln::addr::LxSocketAddress};
use lexe_std::Apply;
use lexe_tokio::{
    notify,
    notify_once::NotifyOnce,
    task::{LxTask, MaybeLxTask},
};
use lightning::ln::peer_handler::PeerHandleError;
#[cfg(doc)]
use lightning::ln::peer_handler::PeerManager;
use tokio::{
    io::Interest,
    net::TcpStream,
    sync::{
        mpsc::{
            self,
            error::{TryRecvError, TrySendError},
        },
        Notify,
    },
    time,
};
use tracing::{debug, info, info_span, instrument, trace, warn};

/// The max time we'll wait for an outbound p2p TCP connection to connect.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The maximum amount of time we'll allow LDK to complete the P2P handshake.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The size of each [`Connection::read_buf`]. LDK suggests 4 KiB.
const READ_BUF_LEN: usize = 4 << 10; // 4 KiB

//
// --- p2p module public interface ---
//

/// Connects to a LN peer, returning early if we were already connected.
/// Cycles through the given addresses until we run out of connect attempts.
pub async fn connect_peer_if_necessary<PM>(
    peer_manager: &PM,
    node_pk: &NodePk,
    addrs: &[LxSocketAddress],
) -> anyhow::Result<MaybeLxTask<()>>
where
    PM: PeerManagerTrait,
{
    ensure!(!addrs.is_empty(), "No addrs were provided");

    // Early return if we're already connected
    // TODO(max): LDK's fn is O(n) in the # of peers...
    if peer_manager.is_connected(node_pk) {
        return Ok(MaybeLxTask(None));
    }

    // Cycle the given addresses in order
    let mut addrs = addrs.iter().cycle();

    // Retry at least a couple times to mitigate an outbound connect race
    // between the reconnector and open_channel which has been observed.
    let retries = 5;
    for _ in 0..retries {
        let addr = addrs.next().expect("Cycling through a non-empty slice");

        match do_connect_peer(peer_manager, node_pk, addr).await {
            Ok(conn_task) => return Ok(MaybeLxTask(Some(conn_task))),
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
            return Ok(MaybeLxTask(None));
        }
    }

    // Do the last attempt.
    let addr = addrs.next().expect("Cycling through a non-empty slice");
    let conn_task = do_connect_peer(peer_manager, node_pk, addr)
        .await
        .context("Failed to connect to peer")?;

    Ok(MaybeLxTask(Some(conn_task)))
}

async fn do_connect_peer<PM>(
    peer_manager: &PM,
    node_pk: &NodePk,
    addr: &LxSocketAddress,
) -> anyhow::Result<LxTask<()>>
where
    PM: PeerManagerTrait,
{
    debug!(%node_pk, %addr, "Starting do_connect_peer");

    // TcpStream::connect takes a `String` in SGX.
    let addr_str = addr.to_string();
    let stream = TcpStream::connect(addr_str)
        .apply(|fut| time::timeout(CONNECT_TIMEOUT, fut))
        .await
        .context("Connect request timed out")?
        .context("TcpStream::connect() failed")?;

    let (mut conn_tx, conn) =
        Connection::setup_outbound(peer_manager, stream, addr.clone(), node_pk);
    let task_name = format!(
        "p2p-conn-{}-{}",
        hex::display(&node_pk.to_array().as_slice()[..4]),
        conn.ctl.id
    );
    let mut conn_task = LxTask::spawn(task_name, conn.run());

    // A future which completes once the connection is usable.
    //
    // Since LDK's API doesn't let you know when the connection establishes
    // and handshake completes, you have to keep polling
    // `peer_manager.is_connected()`. completes.
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
            Ok(conn_task)
        }
        res = &mut conn_task => {
            let msg = format!(
                "New outbound p2p conn d/c'd before handshake complete: \
                 {res:?}"
            );
            warn!("{msg}"); // Also log; this code is historically finicky
            Err(anyhow!("{msg}"))
        }
        _ = tokio::time::sleep(HANDSHAKE_TIMEOUT) => {
            // Tell connection to d/c and unregister from PeerManager
            conn_tx.disconnect_socket();
            peer_manager.socket_disconnected(&conn_tx);
            // TODO(phlip9): wait for `conn_task`?

            let msg =
                "New outbound p2p conn timed out before handshake complete";
            warn!(%node_pk, %addr, "{msg}");
            Err(anyhow!("{msg}: {node_pk}@{addr}"))
        }
    }
}

/// Spawn a task to handle a new inbound p2p TCP connection and register it with
/// the LDK [`PeerManager`].
pub fn spawn_inbound<PM: PeerManagerTrait>(
    peer_manager: &PM,
    stream: TcpStream,
) -> LxTask<()> {
    let conn = Connection::setup_inbound(peer_manager, stream);
    // TODO(phlip9): find a way to set task name with node_pk after handshake?
    let task_name = format!("p2p-conn--inbound-{}", conn.ctl.id);
    LxTask::spawn(task_name, conn.run())
}

/// Spawn a task that calls [`PeerManager::process_events`] on notification.
// TODO(phlip9): move BGP pm_timer.tick() here?
// TODO(phlip9): move BGP process_events -> peer_manager.process_events() here?
pub fn spawn_process_events_task<PM: PeerManagerTrait>(
    peer_manager: PM,
    mut process_events_rx: notify::Receiver,
    mut shutdown: NotifyOnce,
) -> LxTask<()> {
    const SPAN_NAME: &str = "(process-p2p)(peer-man)";
    LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
        let mut iter: usize = 0;
        loop {
            // Cheap check
            if shutdown.try_recv() {
                break;
            }
            // Greedily poll `process_events_rx` first since it probably
            // has work.
            tokio::select! {
                biased;
                () = process_events_rx.recv() => peer_manager.process_events(),
                () = shutdown.recv() => break,
            }

            // Yield every few iters. Reduce task starvation, since
            // `process_events` can do lots of work each iter and
            // `process_events_rx` won't run out of work under load. The
            // default 128 await budget is probably too large here.
            iter = iter.wrapping_add(1);
            if iter % 8 == 0 {
                tokio::task::yield_now().await;
            }
        }
        trace!("shutdown");
    })
}

//
// --- p2p module internals ---
//

/// A handle to a `Connection`. Used to request the socket to send data and/or
/// disconnect. Cheaply cloneable.
#[derive(Clone)]
pub struct ConnectionTx {
    /// Update the connection control state (disconnect/pause reads).
    ctl: Arc<ConnectionCtl>,
    /// Send write data requests to [`Connection`] for writing to socket.
    write_tx: mpsc::Sender<Box<[u8]>>,
}

/// A Lightning p2p connection. Wraps a tokio [`TcpStream`] in additional logic
/// required to interface with LDK's [`PeerManager`].
struct Connection<PM> {
    /// Get notified of connection control updates (disconnect/resume_read).
    ctl: Arc<ConnectionCtl>,

    /// Receive write data requests from [`ConnectionTx::send_data`].
    write_rx: mpsc::Receiver<Box<[u8]>>,

    /// Handle to LDK [`PeerManager`].
    peer_manager: PM,

    /// The underlying TCP connection.
    stream: TcpStream,

    /// The next enqueued write.
    write_buf: Option<Box<[u8]>>,
    /// If we didn't manage to fully write `write_buf` to the socket, then
    /// we'll start our next write at this offset in `write_buf`.
    write_offset: usize,

    /// A fixed buffer to hold data read from the socket, before we immediately
    /// pass it on to [`PeerManager::read_event`].
    read_buf: Box<[u8; READ_BUF_LEN]>,

    /// Connection statistics
    stats: ConnectionStats,

    /// LDK requires us to pass a full [`ConnectionTx`] to `read_event` etc...,
    /// so we have to hold onto an extra one inside `Connection`...
    conn_tx: ConnectionTx,
}

/// [`Connection`] control state. Used to notify the [`Connection`] that it
/// should disconnect or resume reads.
///
/// This control-plane state is intentionally separate from the `write_tx` data
/// plane, since control should not be subject to backpressure. Without this
/// separation, we can accidentally lose `resume_read=true` commands when the
/// [`ConnectionTx`] -> [`Connection`] write queue is full.
struct ConnectionCtl {
    /// The connection id.
    id: u64,
    /// The current [`Connection`] control state.
    ///
    /// One of [`STATE_NORMAL`] or [`STATE_PAUSE_READ`] or [`STATE_DISCONNECT`]
    state: AtomicUsize,
    /// Notify the associated [`Connection`] task that `state` changed.
    notify: Notify,
}

// NOTE: the methods that touch `ConnectionCtl::state` will all need to change
// if another case is added.

/// [`ConnectionCtl::state`] when [`Connection`] is running normally.
const STATE_NORMAL: usize = 0;
/// [`ConnectionCtl::state`] when [`Connection`] has its reads paused.
const STATE_PAUSE_READ: usize = 1;
/// [`ConnectionCtl::state`] when [`Connection`] is disconnected or in the
/// process of disconnecting.
const STATE_DISCONNECT: usize = 2;

/// Track some overall per-`Connection` stats.
// TODO(phlip9): do something with this outside of tests?
struct ConnectionStats {
    total_bytes_written: usize,
    total_bytes_read: usize,
}

/// The reason for a `Connection` disconnect.
#[derive(Debug)]
pub enum Disconnect {
    /// Socket error (peer immediate disconnect).
    Socket(std::io::ErrorKind),
    /// We can't read from the socket anymore: remote peer write half-close.
    ReadClosed,
    /// We can't write to the socket anymore: remote peer read half-close.
    WriteClosed,
    /// PeerManager called `ConnectionTx::disconnect_socket`.
    PeerManager,
}

/// A trait that encapsulates the exact interface we require from the LDK
/// [`PeerManager`] as far as `Connection` is concerned.
pub trait PeerManagerTrait: Clone + Send + 'static {
    // --- Lexe ---

    /// Returns `true` if we're connected to a peer with [`NodePk`].
    /// NOTE: current [`PeerManager`] impl is O(#peers) and locks each peer
    /// struct, which is not ideal...
    fn is_connected(&self, node_pk: &NodePk) -> bool;

    /// Notify seperate process events task that it should call
    /// [`PeerManager::process_events`].
    fn notify_process_events_task(&self);

    // --- LDK ---

    /// Register a new inbound connection with the [`PeerManager`]. Returns an
    /// initial write that should be sent immediately. May return `Err` to
    /// reject the new connection, which should then be disconnected.
    ///
    /// See: [`PeerManager::new_outbound_connection`]
    fn new_outbound_connection(
        &self,
        node_pk: &NodePk,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<Vec<u8>, PeerHandleError>;

    /// Register a new outbound connection with the [`PeerManager`]. May return
    /// `Err` to reject the new connection, which should then be disconnected.
    ///
    /// See: [`PeerManager::new_inbound_connection`]
    fn new_inbound_connection(
        &self,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<(), PeerHandleError>;

    /// Notify the [`PeerManager`] that the connection associated with `conn_tx`
    /// has disconnected.
    ///
    /// This fn is idempotent, so it's safe to call multiple times.
    ///
    /// See: [`PeerManager::socket_disconnected`]
    fn socket_disconnected(&self, conn_tx: &ConnectionTx);

    /// Feed the [`PeerManager`] new data read from the socket associated with
    /// `conn_tx`.
    ///
    /// Returns `Ok(true)`, if the connection should apply backpressure on
    /// reads. That means it should avoid calling [`PeerManager::read_event`]
    /// until the next `ConnectionTx::send_data(.., resume_read: true)` request.
    ///
    /// Returns `Err` if the socket should be disconnected. You do not need to
    /// call `socket_disconnected`.
    ///
    /// You SHOULD call [`PeerManager::process_events`] sometime after a
    /// `read_event` to generate subsequent `send_data` calls.
    ///
    /// This will NOT call `send_data` to avoid re-entrancy reasons.
    ///
    /// See: [`PeerManager::read_event`]
    fn read_event(
        &self,
        conn_tx: &mut ConnectionTx,
        data: &[u8],
    ) -> Result<bool, PeerHandleError>;

    /// Drive the [`PeerManager`] state machine to handle new `read_event`s.
    /// Drives ALL peers in the [`PeerManager`].
    ///
    /// May call `send_data` on various peer `ConnectionTx`'s.
    ///
    /// See: [`PeerManager::process_events`]
    fn process_events(&self);

    /// Notify the [`PeerManager`] that the connection associated with `conn_tx`
    /// now has room for more `send_data` write requests.
    ///
    /// May call `send_data` on the `conn_tx` multiple times.
    ///
    /// See: [`PeerManager::write_buffer_space_avail`]
    fn write_buffer_space_avail(
        &self,
        conn_tx: &mut ConnectionTx,
    ) -> Result<(), PeerHandleError>;
}

//
// --- impl Connection ---
//

impl<PM: PeerManagerTrait> Connection<PM> {
    fn new(peer_manager: PM, stream: TcpStream) -> (ConnectionTx, Self) {
        let ctl = Arc::new(ConnectionCtl::new());
        let (write_tx, write_rx) = mpsc::channel(8);
        let conn_tx = ConnectionTx {
            ctl: ctl.clone(),
            write_tx,
        };
        let conn = Self {
            ctl,
            write_rx,
            stream,
            peer_manager,
            write_buf: None,
            write_offset: 0,
            read_buf: Box::new([0u8; READ_BUF_LEN]),
            stats: ConnectionStats::new(),
            conn_tx: conn_tx.clone(),
        };
        (conn_tx, conn)
    }

    fn setup_outbound(
        peer_manager: &PM,
        stream: TcpStream,
        addr: LxSocketAddress,
        node_pk: &NodePk,
    ) -> (ConnectionTx, Self) {
        let (conn_tx, mut conn) = Self::new(peer_manager.clone(), stream);
        let initial_write = peer_manager
            .new_outbound_connection(node_pk, conn_tx.clone(), Some(addr))
            .expect(
                "outbound: somehow registered conn_tx multiple times with PeerManager",
            );
        conn.write_buf = Some(initial_write.into());
        (conn_tx, conn)
    }

    fn setup_inbound(peer_manager: &PM, stream: TcpStream) -> Self {
        let addr = stream
            .peer_addr()
            .ok()
            .and_then(|sockaddr| LxSocketAddress::try_from(sockaddr).ok());
        let (conn_tx, conn) = Self::new(peer_manager.clone(), stream);
        peer_manager.new_inbound_connection(conn_tx, addr).expect("inbound: somehow registered conn_tx multiple times with PeerManager");
        conn
    }

    async fn run(mut self) {
        self.run_ref().await
    }

    #[instrument(skip_all, name = "(p2p-conn)", fields(id = self.ctl.id))]
    async fn run_ref(&mut self) {
        trace!("start");

        let disconnect = loop {
            // Read new control state for this iter.
            //
            // If `pause_read=true`, we'll avoid calling
            // `PeerManager::read_event` until the next
            // `ConnectionTx::send_data(.., resume_read: true)` request.
            let pause_read = match self.ctl.load_ctl_state() {
                Ok(pause_read) => pause_read,
                Err(disconnect) => break disconnect,
            };

            // The socket events (if any) we want to be notified of in this
            // select iter.
            let interest = self.socket_interest(pause_read);

            // True if we did some real work (read/write), so we can yield to
            // other tasks.
            let mut did_work = false;

            trace!(
                write_buf = ?self.write_buf.as_ref().map(|b| b.len()),
                write_offset = self.write_offset,
                pause_read,
                ?interest,
            );

            tokio::select! {
                // `ConnectionCtl` notified disconnect/resume_read
                // -> nothing (we'll pick up the new state on the next iter)
                () = self.ctl.notify.notified() => {
                    trace!("notified");
                },

                // `ConnectionTx::try_send_data(data=&[..])`
                // -> enqueue for writing to socket
                // -> notify `PeerManager::write_buffer_space_avail`
                req = self.write_rx.recv(), if self.write_buf.is_none() => {
                    trace!(write_buf = req.as_ref().map(|b| b.len()), "recv");
                    if let Err(disconnect) = self.handle_rx_write_req(req) {
                        break disconnect;
                    }
                    if let Err(disconnect) =
                        self.notify_send_data_channel_space_avail()
                    {
                        break disconnect;
                    }
                }

                // Socket is ready to read or write
                // -> is_writable => (xN) write_buf -> stream.try_write && write_rx.try_recv
                // -> is_readable => (xN) stream.try_read -> read_buf -> PeerManager::read_event
                //                   PeerManager::notify_process_events_task
                //
                // NOTE: `unwrap_or(Interest::ERROR)` is needed b/c we have to
                // evaluate the `ready` future with _some_ interest, but the
                // future will not be polled as `interest.is_none()`.
                res = self.stream.ready(interest.unwrap_or(Interest::ERROR)),
                    if interest.is_some() =>
                {
                    trace!(?res, "ready");
                    let ready = match res {
                        Ok(ready) => ready,
                        Err(err) => break Disconnect::Socket(err.kind()),
                    };

                    // If socket says it's ready to write
                    // -> try to write as much as we can.
                    if ready.is_writable() {
                        if let Err(disconnect) = self.try_write_buf_many() {
                            break disconnect;
                        }
                        did_work = true;
                    };

                    // If socket says it's ready to read
                    // -> try to read a few times.
                    if ready.is_readable() {
                        if let Err(disconnect) = self.try_read_buf_many() {
                            break disconnect;
                        }
                        did_work = true;
                    }
                }
            }

            // If we did some real work (read/write), we'll yield to other tasks
            // to avoid starvation.
            if did_work || cfg!(test) {
                #[cfg(not(test))]
                tokio::task::yield_now().await;

                #[cfg(test)] // Generate different task interleavings in test
                test::maybe_yield("conn_iter").await;
            }
        };

        // Disconnect

        // Tell `PeerManager`
        if !disconnect.is_peer_manager() {
            self.peer_manager.socket_disconnected(&self.conn_tx);
        }

        // Set `STATE_DISCONNECT`
        self.ctl.store_state_disconnect();

        // Close the mpsc queue
        self.write_rx.close();

        match disconnect {
            Disconnect::Socket(error) =>
                warn!("Disconnected: Socket error: {error}"),
            Disconnect::ReadClosed => info!("Disconnected: Read closed"),
            Disconnect::WriteClosed => info!("Disconnected: Write closed"),
            Disconnect::PeerManager =>
                info!("Disconnected: PeerManager called disconnect_socket"),
        }
    }

    /// Do we want to read and/or write to the socket?
    ///
    /// ->  Read: reads are unpaused
    /// -> Write: have a write buffered
    fn socket_interest(&self, pause_read: bool) -> Option<Interest> {
        // Read if reads are unpaused.
        let want_read = !pause_read;
        // Write if we have something queued up.
        let want_write = self.write_buf.is_some();

        if want_read && want_write {
            Some(Interest::READABLE | Interest::WRITABLE)
        } else if want_read && !want_write {
            Some(Interest::READABLE)
        } else if !want_read && want_write {
            Some(Interest::WRITABLE)
        } else {
            None
        }
    }

    /// Handle a write data request from a [`ConnectionTx`].
    ///
    /// -> enqueue for writing to socket
    fn handle_rx_write_req(
        &mut self,
        rx_write_req: Option<Box<[u8]>>,
    ) -> Result<(), Disconnect> {
        assert!(self.write_buf.is_none());
        assert_eq!(self.write_offset, 0);

        let data = match rx_write_req {
            Some(data) => data,
            // case: all `ConnectionTx` dropped.
            //
            // Technically this is unreachable, since we hold on to a
            // `ConnectionTx` at all times, so the rx should never close from no
            // live tx's...
            None =>
                if cfg!(debug_assertions) {
                    unreachable!()
                } else {
                    return Err(Disconnect::PeerManager);
                },
        };
        assert_ne!(data.len(), 0);

        // Enqueue next write
        self.write_buf = Some(data);
        self.write_offset = 0;

        Ok(())
    }

    /// Tell [`PeerManager`] we have space for more write data requests in the
    /// mpsc queue.
    fn notify_send_data_channel_space_avail(
        &mut self,
    ) -> Result<(), Disconnect> {
        self.peer_manager
            .write_buffer_space_avail(&mut self.conn_tx)
            .map_err(|PeerHandleError {}| Disconnect::PeerManager)
    }

    /// Loop `try_write_buf` + `write_rx.try_recv` until we either drain our
    /// `write_rx` write queue or we get socket write backpressure.
    fn try_write_buf_many(&mut self) -> Result<(), Disconnect> {
        // `true` if we successfully recv from `write_rx`. We'll notify
        // `PeerManager` at the end if this is `true`.
        let mut is_send_data_space_avail = false;

        loop {
            // Try to write the current `self.write_buf` to the socket.
            let can_write_more = self.try_write_buf()?;
            if !can_write_more {
                break;
            }

            // We can write more. Try to pop off another write buffer from the
            // `write_rx` queue.
            assert!(self.write_buf.is_none());
            match self.write_rx.try_recv() {
                Ok(write_buf) => {
                    self.write_buf = Some(write_buf);
                    self.write_offset = 0;
                    is_send_data_space_avail = true;
                }
                // No more buffers in the `write_rx` queue; we're done
                // flushing.
                Err(TryRecvError::Empty) => {
                    is_send_data_space_avail = true;
                    break;
                }
                Err(TryRecvError::Disconnected) =>
                    return Err(Disconnect::PeerManager),
            }
        }

        // We successfully recv'd on `write_rx`
        // ==> there's space available for `PeerManager` to `send_data`
        if is_send_data_space_avail {
            self.notify_send_data_channel_space_avail()?;
        }

        Ok(())
    }

    /// Attempt a `stream.try_write(&write_buf[write_offset..])`.
    ///
    /// Returns `Ok(true)` if we can potentially write more to the socket.
    fn try_write_buf(&mut self) -> Result<bool, Disconnect> {
        let write_buf: &[u8] = self.write_buf.as_ref().expect(
            "we should only get write readiness if write_buf.is_some()",
        );
        assert!(!write_buf.is_empty());

        let to_write = &write_buf[self.write_offset..];
        assert!(!to_write.is_empty());

        #[cfg(not(test))]
        let res = self.stream.try_write(to_write);
        #[cfg(test)] // inject partial writes and io::ErrorKind::WouldBlock
        let res = test::maybe_stream_try_write(&mut self.stream, to_write);

        let bytes_written = match res {
            // Wrote some bytes -> update `write_buf`
            Ok(bytes_written) => {
                let bytes_written = match NonZeroUsize::new(bytes_written) {
                    // write=0 => Remote peer read half-close
                    None => return Err(Disconnect::WriteClosed),
                    Some(bytes_written) => {
                        self.stats.total_bytes_written += bytes_written.get();
                        bytes_written
                    }
                };

                trace!(bytes_written);

                let new_write_offset = self.write_offset + bytes_written.get();
                assert!(new_write_offset <= write_buf.len());

                if new_write_offset == write_buf.len() {
                    self.write_buf = None;
                    self.write_offset = 0;
                } else {
                    self.write_offset = new_write_offset;
                }

                Some(bytes_written)
            }
            // `ready` can return false positive
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => None,
            Err(err) => return Err(Disconnect::Socket(err.kind())),
        };

        let can_write_more = match bytes_written {
            Some(_bytes_written) => self.write_buf.is_none(),
            None => false,
        };
        Ok(can_write_more)
    }

    /// Loop `try_read_buf` + `peer_manager.read_buf` until we either drain our
    /// TCP stream read queue or we get LDK read backpressure. Then call
    /// [`PeerManagerTrait::notify_process_events_task`] if we read anything.
    fn try_read_buf_many(&mut self) -> Result<(), Disconnect> {
        // If we successfully read any data at all, we should eventually call
        // `PeerManager::process_events`
        let mut any_data_read = false;

        // Read up to `8 * READ_BUF_LEN` ≈ 32 KiB in one call
        for _ in 0..8 {
            // Try to fill `self.read_buf`
            let bytes_read = match self.try_read_buf()? {
                Some(bytes_read) => {
                    any_data_read = true;
                    bytes_read
                }
                None => break,
            };

            // We can read more if we completely filled `self.read_buf`.
            let can_read_more = bytes_read.get() == READ_BUF_LEN;

            // Give `PeerManager` the data we just read.
            let data = &self.read_buf[..bytes_read.get()];
            let pause_read =
                match self.peer_manager.read_event(&mut self.conn_tx, data) {
                    // LDK may apply read backpressure and ask us to pause reads
                    Ok(pause_read) => pause_read,
                    Err(PeerHandleError {}) =>
                        return Err(Disconnect::PeerManager),
                };

            // Update our shared state with the `ConnectionTx` handles.
            let state = if pause_read {
                STATE_PAUSE_READ
            } else {
                STATE_NORMAL
            };
            self.ctl.store_state_normal_or_pause_read(state)?;

            // Stop reading
            if pause_read || !can_read_more {
                break;
            }
        }

        // Notify `process_events_task` that it should call
        // `PeerManager::process_events`.
        if any_data_read {
            self.peer_manager.notify_process_events_task();
        }

        Ok(())
    }

    /// Attempt a `stream.try_read(&mut read_buf)`. Returns the number of bytes
    /// read, if any.
    fn try_read_buf(&mut self) -> Result<Option<NonZeroUsize>, Disconnect> {
        let read_buf = self.read_buf.as_mut_slice();

        #[cfg(not(test))]
        let res = self.stream.try_read(read_buf);
        #[cfg(test)] // inject partial reads and io::ErrorKind::WouldBlock
        let res = test::maybe_stream_try_read(&mut self.stream, read_buf);

        match res {
            Ok(bytes_read) => match NonZeroUsize::new(bytes_read) {
                // read=0 => Remote peer write half-close
                None => Err(Disconnect::ReadClosed),
                Some(bytes_read) => {
                    self.stats.total_bytes_read += bytes_read.get();
                    Ok(Some(bytes_read))
                }
            },
            // `ready` can return false positive
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(Disconnect::Socket(err.kind())),
        }
    }
}

//
// --- impl ConnectionTx ---
//

impl ConnectionTx {
    /// Try to send some data to the connection task for writing to a remote
    /// peer and/or request for reads to be unpaused.
    ///
    /// Returns the number of bytes enqueued, which will be zero if the
    /// `write_tx` channel is full, indicating write backpressure.
    ///
    /// If there is write backpressure, the [`Connection`] MUST call
    /// [`PeerManager::write_buffer_space_avail`] when it has room for more
    /// writes.
    fn send_data(&mut self, data: &[u8], resume_read: bool) -> usize {
        trace!(
            write_len = data.len(),
            resume_read,
            "ConnectionTx => send_data"
        );
        let bytes_enqueued = self.try_send_data(data);
        if resume_read {
            self.ctl.resume_read_and_notify();
        }
        bytes_enqueued
    }

    fn try_send_data(&mut self, data: &[u8]) -> usize {
        if data.is_empty() {
            return 0;
        }

        // Since we're not async, we first try to acquire a send permit to see
        // if we're getting backpressure/disconnected. This also lets us avoid
        // copying `data` until we know we can actually enqueue it.
        match self.write_tx.try_reserve() {
            // Enqueue `data` to be written
            Ok(send_permit) => {
                let write_len = data.len();
                trace!(write_len, "ConnectionTx => try_send_data");
                send_permit.send(data.into());
                write_len
            }

            // case: channel full => backpressure => write_len = 0
            //
            // NOTE: the `Connection` task MUST call `PeerManager`
            // `write_buffer_space_avail` in the future to unpause writes!
            Err(TrySendError::Full(())) => 0,

            // case: channel closed => D/C'd => drop write => write_len = 0
            Err(TrySendError::Closed(())) => 0,
        }
    }

    /// Notify the [`Connection`] that the [`PeerManager`] wants to disconnect.
    fn disconnect_socket(&mut self) {
        trace!("ConnectionTx => disconnect");
        self.ctl.disconnect_and_notify()
    }
}

// NOTE: use separate impl to make rust-doc links work.
impl lightning::ln::peer_handler::SocketDescriptor for ConnectionTx {
    #[inline]
    fn send_data(&mut self, data: &[u8], resume_read: bool) -> usize {
        ConnectionTx::send_data(self, data, resume_read)
    }
    #[inline]
    fn disconnect_socket(&mut self) {
        ConnectionTx::disconnect_socket(self)
    }
}

impl PartialEq for ConnectionTx {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.ctl.id == other.ctl.id
    }
}
impl Eq for ConnectionTx {}

impl Hash for ConnectionTx {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.ctl.id)
    }
}

//
// --- impl ConnectionCtl ---
//

impl ConnectionCtl {
    fn new() -> Self {
        static CONNECTION_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: CONNECTION_ID.fetch_add(1, Ordering::Relaxed),
            state: AtomicUsize::new(STATE_NORMAL),
            notify: Notify::new(),
        }
    }

    /// Tell [`Connection`] to disconnect.
    fn disconnect_and_notify(&self) {
        self.store_state_disconnect();
        self.notify.notify_one();
    }

    /// Tell [`Connection`] to resume reads (if not already resumed or
    /// disconnected).
    fn resume_read_and_notify(&self) {
        if let Ok(true) = self.store_state_normal_or_pause_read(STATE_NORMAL) {
            trace!("ConnectionTx => resume read");
            self.notify.notify_one()
        }
    }

    /// Read [`ConnectionCtl::state`] and maybe resume reads or disconnect.
    fn load_ctl_state(&self) -> Result<bool, Disconnect> {
        let state = self.state.load(Ordering::SeqCst);
        if state != STATE_DISCONNECT {
            let pause_read = state == STATE_PAUSE_READ;
            Ok(pause_read)
        } else {
            Err(Disconnect::PeerManager)
        }
    }

    /// Set [`ConnectionCtl::state`] to [`STATE_DISCONNECT`].
    #[inline]
    fn store_state_disconnect(&self) {
        self.state.store(STATE_DISCONNECT, Ordering::SeqCst);
    }

    /// Set [`ConnectionCtl::state`] to [`STATE_NORMAL`] or
    /// [`STATE_PAUSE_READ`].
    ///
    /// Returns `Ok(true)` if the state changed and `Ok(false)` if the state was
    /// already `new`. If we raced with a disconnect, return
    /// `Err(Disconnect::PeerManager)`.
    fn store_state_normal_or_pause_read(
        &self,
        new: usize,
    ) -> Result<bool, Disconnect> {
        // Setup `compare_exchange` so that we'll only hit `Ok(_)` if our state
        // changes from `STATE_NORMAL` -> `STATE_PAUSE_READ` or
        // `STATE_PAUSE_READ` -> `STATE_NORMAL`.
        //
        // This prevents accidentally overwriting a `STATE_DISCONNECT`.
        let opposite = if new == STATE_NORMAL {
            STATE_PAUSE_READ
        } else if new == STATE_PAUSE_READ {
            STATE_NORMAL
        } else {
            unreachable!()
        };
        let curr = opposite;
        let res = self.state.compare_exchange(
            curr,
            new,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        match res {
            // case 1: state was `opposite` => state changed to `new`
            Ok(_) => Ok(true),
            // case 2: state was already `new` => (no change)
            // case 3: state was `STATE_DISCONNECT` => (no change)
            Err(prev) =>
                if prev != STATE_DISCONNECT {
                    Ok(false)
                } else {
                    Err(Disconnect::PeerManager)
                },
        }
    }
}

//
// --- impl ConnectionStats ---
//

impl ConnectionStats {
    fn new() -> Self {
        Self {
            total_bytes_written: 0,
            total_bytes_read: 0,
        }
    }
}

//
// --- impl Disconnect ---
//

impl Disconnect {
    fn is_peer_manager(&self) -> bool {
        matches!(self, Self::PeerManager)
    }
}

#[cfg(test)]
mod test {
    use std::{
        cell::Cell,
        cmp::min,
        collections::VecDeque,
        io,
        sync::{Arc, Mutex},
    };

    use common::rng::ThreadFastRng;
    use io::BufRead;
    use lexe_tokio::task::LxTask;
    use rand::{seq::SliceRandom, Rng, RngCore};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        sync::oneshot,
    };

    use super::{ldk_test::make_tcp_connection, *};

    // TODO(phlip9): get probabilities from thread-local `TestCtx`?

    fn maybe(p: f64) -> bool {
        ThreadFastRng::new().gen_bool(p)
    }

    pub async fn maybe_yield(label: &'static str) {
        if maybe(0.25) {
            trace!("yield_now({label})");
            tokio::task::yield_now().await
        }
    }

    pub fn maybe_stream_try_write(
        stream: &mut TcpStream,
        to_write: &[u8],
    ) -> io::Result<usize> {
        if maybe(0.1) {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        } else {
            let to_write = maybe_partial_write(to_write);
            stream.try_write(to_write)
        }
    }

    pub fn maybe_partial_write(to_write: &[u8]) -> &[u8] {
        if maybe(0.25) {
            let to_write_len = to_write.len();
            let to_write_len = ThreadFastRng::new().gen_range(1..=to_write_len);
            &to_write[..to_write_len]
        } else {
            to_write
        }
    }

    pub fn maybe_stream_try_read(
        stream: &mut TcpStream,
        read_buf: &mut [u8],
    ) -> io::Result<usize> {
        if maybe(0.1) {
            Err(io::Error::from(io::ErrorKind::WouldBlock))
        } else {
            let read_buf = maybe_partial_read(read_buf);
            stream.try_read(read_buf)
        }
    }

    pub fn maybe_partial_read(read_buf: &mut [u8]) -> &mut [u8] {
        if maybe(0.25) {
            let read_buf_len = read_buf.len();
            let read_buf_len = ThreadFastRng::new().gen_range(1..=read_buf_len);
            &mut read_buf[..read_buf_len]
        } else {
            read_buf
        }
    }

    #[tokio::test]
    async fn test_echo() {
        use std::str::FromStr;
        crate::logger::init_for_testing();
        let iters = std::env::var("TEST_ECHO_ITERS")
            .map_err(|_| ())
            .and_then(|s| u64::from_str(&s).map_err(|_| ()))
            .unwrap_or(100);
        for seed in 0..iters {
            println!("seed = {seed}");
            do_test_echo(TestCtx::new(seed)).await;
        }
    }

    #[tokio::test]
    async fn test_echo_1() {
        crate::logger::init_for_testing();
        do_test_echo(TestCtx::new(123)).await;
    }

    #[tokio::test]
    async fn test_echo_2() {
        crate::logger::init_for_testing();
        do_test_echo(TestCtx::new(993)).await;
    }

    #[tokio::test]
    async fn test_echo_3() {
        crate::logger::init_for_testing();
        do_test_echo(TestCtx::new(138)).await;
    }

    #[derive(Copy, Clone)]
    struct TestCtx {
        /// The rng seed for this test iteration
        seed: u64,
        /// Size of the message we'll try to send/recv to/from the `EchoPeer`
        msg_len: usize,
        /// Test harness' max chunk to_write_len
        to_write_len: usize,
        /// Don't disconnect test harness TCP until it reads at least this much
        /// Helps catch `pause_read` deadlocks.
        min_read_len: usize,
        /// Make `EchoPeer` pause_read when it has this many bytes buffered.
        pause_read_threshold: usize,
    }

    thread_local! {
        // clippy errors when built for SGX without without this lint line
        // TODO(phlip9): incorrect lint, remove when clippy not broken
        #[allow(clippy::missing_const_for_thread_local)]
        static TEST_CTX: Cell<Option<TestCtx>> = const { Cell::new(None) };
    }
    fn ctx() -> TestCtx {
        TEST_CTX.get().unwrap()
    }
    fn set_ctx(ctx: TestCtx) {
        TEST_CTX.set(Some(ctx))
    }

    impl TestCtx {
        fn new(seed: u64) -> Self {
            ThreadFastRng::seed(seed);
            let mut rng = ThreadFastRng::new();
            let msg_len = rng.gen_range(1..(128 << 10));
            let to_write_len = if rng.gen_bool(0.5) {
                rng.gen_range(1..=512)
            } else {
                rng.gen_range(1..=msg_len)
            };
            let min_read_len = if rng.gen_bool(0.5) {
                msg_len
            } else {
                rng.gen_range(1..=msg_len)
            };
            let pause_read_threshold =
                *[1, 512, 16 << 10, usize::MAX].choose(&mut rng).unwrap();
            let ctx = Self {
                seed,
                msg_len,
                to_write_len,
                min_read_len,
                pause_read_threshold,
            };
            set_ctx(ctx);
            ctx
        }

        fn print(&self) {
            let ctx = self;
            trace!(
                ctx.seed,
                ctx.msg_len,
                ctx.to_write_len,
                ctx.min_read_len,
                ctx.pause_read_threshold
            );
        }
    }

    async fn do_test_echo(ctx: TestCtx) {
        ctx.print();

        // Setup
        let mut rng = ThreadFastRng::new();
        let (tcp_a, tcp_b) = make_tcp_connection().await;
        let (process_events_tx, process_events_rx) = notify::channel();
        let peer_manager =
            Arc::new(Mutex::new(EchoPeerManager::new(process_events_tx)));
        let shutdown = NotifyOnce::new();
        let process_events_task = spawn_process_events_task(
            peer_manager.clone(),
            process_events_rx,
            shutdown.clone(),
        );

        let (conn_tx, mut conn) = Connection::new(peer_manager.clone(), tcp_a);

        let addr = None;
        peer_manager.new_inbound_connection(conn_tx, addr).unwrap();

        let mut msg = vec![0u8; ctx.msg_len];
        rng.fill_bytes(&mut msg);

        // TODO(phlip9): timeouts

        // `Connection`
        let conn_task = LxTask::spawn("conn", async move {
            conn.run_ref().await;
            conn.stats
        });

        // Client
        let write_msg = msg.clone();
        let (mut tcp_b_read, mut tcp_b_write) = tcp_b.into_split();
        let client_task = LxTask::spawn("client", async move {
            let (min_read_done_tx, min_read_done_rx) = oneshot::channel::<()>();
            let (tcp_write_closed_tx, tcp_write_closed_rx) =
                oneshot::channel::<()>();

            let write_task = LxTask::spawn("client_write", async move {
                let mut msg = write_msg.as_slice();

                let mut total_written = 0;
                loop {
                    let to_write_len = min(ctx.to_write_len, msg.len());
                    if to_write_len == 0 {
                        break;
                    }
                    let data = &msg[..to_write_len];
                    let write_len = tcp_b_write.write(data).await.unwrap();
                    if write_len == 0 {
                        break;
                    }
                    msg.consume(write_len);
                    total_written += write_len;

                    test::maybe_yield("client_write").await;
                }
                assert_eq!(total_written, write_msg.len());

                // wait for `read_task` to finish reading at least
                // `ctx.min_read_len`.
                min_read_done_rx.await.unwrap();

                // only then write half-close TCP stream (not effective in SGX)
                drop(tcp_b_write);

                // SGX: TCP half-close does nothing. Manually terminate reader
                // so test completes.
                let out = if cfg!(target_env = "sgx") || maybe(0.25) {
                    let _ = tcp_write_closed_tx.send(());
                    None
                } else {
                    Some(tcp_write_closed_tx)
                };

                trace!("client_write: done");

                out
            });
            let read_task = LxTask::spawn("client_read", async move {
                let mut read_msg = vec![0u8; ctx.min_read_len];

                // read at least `ctx.min_read_len`
                tcp_b_read
                    .read_exact(read_msg.as_mut_slice())
                    .await
                    .unwrap();

                // signal to `write_task` that it's ok to close
                min_read_done_tx.send(()).unwrap();

                // try to read as much as possible
                tokio::select! {
                    res = tcp_b_read.read_to_end(&mut read_msg) => {
                        res.unwrap();
                    }
                    // SGX: see note above
                    res = tcp_write_closed_rx => {
                        trace!("tcp_write_closed_rx");
                        res.unwrap();
                    }
                }

                drop(tcp_b_read);

                trace!("client_read: done");

                read_msg
            });

            let (read_msg, _) =
                tokio::try_join!(read_task, write_task).unwrap();

            read_msg
        });

        let (conn_stats, read_msg) =
            tokio::try_join!(conn_task, client_task).unwrap();

        // Stop process_events_task
        shutdown.send();
        process_events_task.await.unwrap();

        let peer = {
            let mut locked = peer_manager.lock().unwrap();
            locked.disconnected_peer.take().unwrap()
        };

        // Print test metrics
        ctx.print();
        let read_msg_len = read_msg.len();
        trace!(
            conn_stats.total_bytes_read,
            peer.total_read_event_len,
            peer.total_write_len_queued,
            conn_stats.total_bytes_written,
            read_msg_len,
        );

        // Check that each stage flows more bytes than the next stage.
        assert!(ctx.msg_len >= conn_stats.total_bytes_read);
        assert!(conn_stats.total_bytes_read >= peer.total_read_event_len);
        assert!(peer.total_read_event_len >= peer.total_write_len_queued);
        assert!(peer.total_write_len_queued >= conn_stats.total_bytes_written);
        assert!(conn_stats.total_bytes_written >= read_msg_len);
        // The min_read_len helps ensure liveness.
        assert!(read_msg_len >= ctx.min_read_len);

        // The final read_msg must be a strict prefix of the original msg
        // (no lost bytes).
        assert_eq!(&read_msg, &msg[..read_msg_len]);
    }

    /// PeerManager that just echoes back data
    struct EchoPeerManager {
        peer: Option<EchoPeer>,
        disconnected_peer: Option<EchoPeer>,
        process_events_tx: notify::Sender,
    }

    struct EchoPeer {
        conn_tx: ConnectionTx,

        ///  in: read_event -> buf
        /// out: process_events/write_buffer_space_avail -> buf -> send_data
        buf: VecDeque<u8>,

        /// When `true` we'll pause_read when `buf` goes above
        /// `pause_read_threshold` and resume_read when it goes below.
        pause_read: bool,

        /// Total number of bytes pushed to `read_event`.
        total_read_event_len: usize,

        /// The number of bytes successfully pushed into the `ConnectionTx::tx`
        /// write queue.
        total_write_len_queued: usize,
    }

    impl EchoPeerManager {
        fn new(process_events_tx: notify::Sender) -> Self {
            Self {
                peer: None,
                disconnected_peer: None,
                process_events_tx,
            }
        }

        fn peer_for(
            &mut self,
            conn_tx: &ConnectionTx,
        ) -> Option<&mut EchoPeer> {
            match &mut self.peer {
                Some(peer) if &peer.conn_tx == conn_tx => Some(peer),
                _ => None,
            }
        }
    }

    impl PeerManagerTrait for Arc<Mutex<EchoPeerManager>> {
        fn is_connected(&self, _node_pk: &NodePk) -> bool {
            todo!()
        }

        fn notify_process_events_task(&self) {
            self.lock().unwrap().process_events_tx.send();
        }

        fn new_outbound_connection(
            &self,
            _node_pk: &NodePk,
            _conn_tx: ConnectionTx,
            _addr: Option<LxSocketAddress>,
        ) -> Result<Vec<u8>, PeerHandleError> {
            todo!()
        }

        fn new_inbound_connection(
            &self,
            conn_tx: ConnectionTx,
            _addr: Option<LxSocketAddress>,
        ) -> Result<(), PeerHandleError> {
            let mut locked = self.lock().unwrap();
            if locked.peer.is_none() {
                locked.peer = Some(EchoPeer::new(conn_tx));
                Ok(())
            } else {
                Err(PeerHandleError {})
            }
        }

        fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
            let mut locked = self.lock().unwrap();
            if locked.peer_for(conn_tx).is_some() {
                // save the now disconnected per for later inspection
                locked.disconnected_peer = locked.peer.take();
            }
        }

        fn read_event(
            &self,
            conn_tx: &mut ConnectionTx,
            data: &[u8],
        ) -> Result<bool, PeerHandleError> {
            trace!("PeerManager::read_event");
            if let Some(peer) = self.lock().unwrap().peer_for(conn_tx) {
                peer.read_event(data)
            } else {
                Err(PeerHandleError {})
            }
        }

        fn process_events(&self) {
            trace!("PeerManager::process_events");
            while let Some(peer) = self.lock().unwrap().peer.as_mut() {
                let can_send_more = peer.send_next();
                if !can_send_more {
                    break;
                }
                // TODO(phlip9): simulate yield by randomly return here
            }
        }

        fn write_buffer_space_avail(
            &self,
            conn_tx: &mut ConnectionTx,
        ) -> Result<(), PeerHandleError> {
            trace!("PeerManager::write_buffer_space_avail");
            loop {
                match self.lock().unwrap().peer_for(conn_tx) {
                    Some(peer) => {
                        let can_send_more = peer.send_next();
                        if !can_send_more {
                            return Ok(());
                        }
                        // TODO(phlip9): simulate yield by randomly return here
                    }
                    None => return Err(PeerHandleError {}),
                }
            }
        }
    }

    impl EchoPeer {
        fn new(conn_tx: ConnectionTx) -> Self {
            Self {
                conn_tx,
                buf: VecDeque::new(),
                pause_read: false,
                total_read_event_len: 0,
                total_write_len_queued: 0,
            }
        }

        // Called from `Connection` task when it has read more data. Return
        // `true` to tell `Connection` to pause read.
        fn read_event(&mut self, data: &[u8]) -> Result<bool, PeerHandleError> {
            trace!(
                buf_len = self.buf.len(),
                read_len = data.len(),
                "EchoPeer::read_event"
            );
            assert_ne!(data.len(), 0);

            // NOTE: pause_read for LDK is just a suggestion and isn't actually
            // enforced. We'll enforce it here to check that our logic is
            // working.
            assert!(!self.pause_read);

            self.buf.extend(data);
            self.total_read_event_len += data.len();

            self.pause_read = self.buf.len() >= ctx().pause_read_threshold;
            Ok(self.pause_read)
        }

        fn send_next(&mut self) -> bool {
            let buf_len = self.buf.len();
            trace!(
                buf_len,
                pause_read = self.pause_read,
                "EchoPeer::send_next"
            );

            let data = self.buf.fill_buf().unwrap();
            if data.is_empty() {
                let prev_pause_read = self.pause_read;
                self.pause_read = buf_len >= ctx().pause_read_threshold;
                let resume_read = prev_pause_read && !self.pause_read;

                if resume_read {
                    let write_len = self.conn_tx.send_data(&[], resume_read);
                    assert_eq!(write_len, 0);
                }
                return false;
            }

            // Inject chaos
            let data = maybe_partial_write(data);
            let to_write_len = data.len();
            assert_ne!(to_write_len, 0);

            // NOTE: we're not guaranteed that there is actually write space
            // available, since `process_events` is a potential caller.
            // Therefore we can't safely `resume_read` until we know
            // `written_len > 0`.
            let resume_read = false;
            let written_len = self.conn_tx.send_data(data, resume_read);
            trace!(written_len, "EchoPeer::send_data ->");

            // Only `resume_read` if we actually managed to queue a write that
            // puts us under the pause_read_threshold.
            let prev_pause_read = self.pause_read;
            self.pause_read =
                buf_len - written_len >= ctx().pause_read_threshold;
            let resume_read = prev_pause_read && !self.pause_read;
            self.conn_tx.send_data(&[], resume_read);

            self.total_write_len_queued += written_len;
            assert!(written_len <= to_write_len);

            self.buf.consume(written_len);
            written_len == to_write_len
        }
    }
}

/// lightning-net-tokio test
#[cfg(test)]
mod ldk_test {
    use std::{
        mem,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use bitcoin::{
        constants::ChainHash,
        secp256k1::{self, ecdh, ecdsa, schnorr},
        Network,
    };
    use common::rng::{Crng, FastRng, RngExt, ThreadFastRng};
    use lexe_tokio::task::LxTask;
    use lightning::{
        events::*,
        ln::{
            inbound_payment::ExpandedKey,
            msgs::*,
            peer_handler::{
                IgnoringMessageHandler, MessageHandler, PeerManager,
            },
        },
        offers::invoice::UnsignedBolt12Invoice,
        routing::gossip::NodeId,
        sign::{NodeSigner, Recipient},
        types::features::{InitFeatures, NodeFeatures},
    };
    use lightning_invoice::RawBolt11Invoice;
    use tokio::net::TcpListener;

    use super::*;
    use crate::logger;

    // Basic integration test copied over from `lightning-net-tokio`.
    #[tokio::test]
    async fn do_basic_connection_test() {
        logger::init_for_testing();

        let mut rng = FastRng::from_u64(328974289374);
        ThreadFastRng::seed(rng.gen_u64());

        let secp_ctx = rng.gen_secp256k1_ctx();
        let a_key = secp256k1::SecretKey::from_slice(&[1; 32]).unwrap();
        let b_key = secp256k1::SecretKey::from_slice(&[2; 32]).unwrap();
        let a_pub = secp256k1::PublicKey::from_secret_key(&secp_ctx, &a_key);
        let b_pub = secp256k1::PublicKey::from_secret_key(&secp_ctx, &b_key);

        let (a_connected_sender, mut a_connected) = mpsc::channel(1);
        let (a_disconnected_sender, mut a_disconnected) = mpsc::channel(1);
        let a_handler = Arc::new(MsgHandler {
            expected_pubkey: b_pub,
            pubkey_connected: a_connected_sender,
            pubkey_disconnected: a_disconnected_sender,
            disconnected_flag: AtomicBool::new(false),
            msg_events: Mutex::new(Vec::new()),
        });
        let a_msg_handler: TestMessageHandler = MessageHandler {
            chan_handler: Arc::clone(&a_handler),
            route_handler: Arc::clone(&a_handler),
            onion_message_handler: Arc::new(IgnoringMessageHandler {}),
            custom_message_handler: Arc::new(IgnoringMessageHandler {}),
        };
        let (a_process_events_tx, a_process_events_rx) = notify::channel();
        let a_manager: TestPeerManager = Arc::new((
            PeerManager::new(
                a_msg_handler,
                0,
                &[1; 32],
                logger::LexeTracingLogger::new(),
                Arc::new(TestNodeSigner::new(a_key)),
            ),
            a_process_events_tx,
        ));
        let shutdown = NotifyOnce::new();
        let a_process_events_task = spawn_process_events_task(
            a_manager.clone(),
            a_process_events_rx,
            shutdown.clone(),
        );

        let (b_connected_sender, mut b_connected) = mpsc::channel(1);
        let (b_disconnected_sender, mut b_disconnected) = mpsc::channel(1);
        let b_handler = Arc::new(MsgHandler {
            expected_pubkey: a_pub,
            pubkey_connected: b_connected_sender,
            pubkey_disconnected: b_disconnected_sender,
            disconnected_flag: AtomicBool::new(false),
            msg_events: Mutex::new(Vec::new()),
        });
        let b_msg_handler: TestMessageHandler = MessageHandler {
            chan_handler: Arc::clone(&b_handler),
            route_handler: Arc::clone(&b_handler),
            onion_message_handler: Arc::new(IgnoringMessageHandler {}),
            custom_message_handler: Arc::new(IgnoringMessageHandler {}),
        };
        let (b_process_events_tx, b_process_events_rx) = notify::channel();
        let b_manager = Arc::new((
            PeerManager::new(
                b_msg_handler,
                0,
                &[2; 32],
                logger::LexeTracingLogger::new(),
                Arc::new(TestNodeSigner::new(b_key)),
            ),
            b_process_events_tx,
        ));
        let b_process_events_task = spawn_process_events_task(
            b_manager.clone(),
            b_process_events_rx,
            shutdown.clone(),
        );

        let (tcp_a, tcp_b) = make_tcp_connection().await;

        let addr_b =
            LxSocketAddress::try_from(tcp_a.peer_addr().unwrap()).unwrap();
        let (_conn_tx_a, conn_a) = Connection::setup_outbound(
            &a_manager,
            tcp_a,
            addr_b,
            &NodePk(b_pub),
        );
        let fut_a = LxTask::spawn_unnamed(conn_a.run());

        let conn_b = Connection::setup_inbound(&b_manager, tcp_b);
        let fut_b = LxTask::spawn_unnamed(conn_b.run());

        tokio::time::timeout(Duration::from_secs(10), a_connected.recv())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), b_connected.recv())
            .await
            .unwrap();

        a_handler.msg_events.lock().unwrap().push(
            MessageSendEvent::HandleError {
                node_id: b_pub,
                action: ErrorAction::DisconnectPeer { msg: None },
            },
        );
        assert!(!a_handler.disconnected_flag.load(Ordering::SeqCst));
        assert!(!b_handler.disconnected_flag.load(Ordering::SeqCst));

        a_manager.process_events();
        tokio::time::timeout(Duration::from_secs(10), a_disconnected.recv())
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(1), b_disconnected.recv())
            .await
            .unwrap();
        assert!(a_handler.disconnected_flag.load(Ordering::SeqCst));
        assert!(b_handler.disconnected_flag.load(Ordering::SeqCst));

        fut_a.await.unwrap();
        fut_b.await.unwrap();

        shutdown.send();
        a_process_events_task.await.unwrap();
        b_process_events_task.await.unwrap();
    }

    pub async fn make_tcp_connection() -> (TcpStream, TcpStream) {
        let sock = TcpListener::bind("[::1]:0").await.unwrap();
        let addr = sock.local_addr().unwrap();
        let accept = async move {
            let (conn, _addr) = sock.accept().await.unwrap();
            conn
        };
        let connect = async move { TcpStream::connect(addr).await.unwrap() };
        tokio::join!(accept, connect)
    }

    //
    // --- Generics boilerplate ---
    //

    type TestMessageHandler = MessageHandler<
        Arc<MsgHandler>,
        Arc<MsgHandler>,
        Arc<IgnoringMessageHandler>,
        Arc<IgnoringMessageHandler>,
    >;

    type TestPeerManager = Arc<(
        PeerManager<
            ConnectionTx,
            Arc<MsgHandler>,
            Arc<MsgHandler>,
            Arc<IgnoringMessageHandler>,
            logger::LexeTracingLogger,
            Arc<IgnoringMessageHandler>,
            Arc<TestNodeSigner>,
        >,
        notify::Sender,
    )>;

    impl PeerManagerTrait for TestPeerManager {
        fn is_connected(&self, node_pk: &NodePk) -> bool {
            self.as_ref().0.peer_by_node_id(&node_pk.0).is_some()
        }

        fn notify_process_events_task(&self) {
            self.as_ref().1.send();
        }

        fn new_outbound_connection(
            &self,
            node_pk: &NodePk,
            conn_tx: ConnectionTx,
            addr: Option<LxSocketAddress>,
        ) -> Result<Vec<u8>, PeerHandleError> {
            self.as_ref().0.new_outbound_connection(
                node_pk.0,
                conn_tx,
                addr.map(SocketAddress::from),
            )
        }

        fn new_inbound_connection(
            &self,
            conn_tx: ConnectionTx,
            addr: Option<LxSocketAddress>,
        ) -> Result<(), PeerHandleError> {
            self.as_ref()
                .0
                .new_inbound_connection(conn_tx, addr.map(SocketAddress::from))
        }

        fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
            self.as_ref().0.socket_disconnected(conn_tx)
        }

        fn read_event(
            &self,
            conn_tx: &mut ConnectionTx,
            data: &[u8],
        ) -> Result<bool, PeerHandleError> {
            self.as_ref().0.read_event(conn_tx, data)
        }

        fn process_events(&self) {
            self.as_ref().0.process_events()
        }

        fn write_buffer_space_avail(
            &self,
            conn_tx: &mut ConnectionTx,
        ) -> Result<(), PeerHandleError> {
            self.as_ref().0.write_buffer_space_avail(conn_tx)
        }
    }

    //
    // --- LDK fakes ---
    //

    struct TestNodeSigner {
        node_secret: secp256k1::SecretKey,
    }
    impl TestNodeSigner {
        pub fn new(node_secret: secp256k1::SecretKey) -> Self {
            Self { node_secret }
        }
    }
    #[rustfmt::skip]
    impl NodeSigner for TestNodeSigner {
        fn get_node_id(&self, recipient: Recipient) -> Result<secp256k1::PublicKey, ()> {
            let node_secret = match recipient {
                Recipient::Node => Ok(&self.node_secret),
                Recipient::PhantomNode => Err(()),
            }?;
            Ok(secp256k1::PublicKey::from_secret_key(
                &FastRng::from_u64(324234).gen_secp256k1_ctx_signing(),
                node_secret,
            ))
        }

        fn ecdh(
            &self,
            recipient: Recipient,
            other_key: &secp256k1::PublicKey,
            tweak: Option<&secp256k1::Scalar>,
        ) -> Result<ecdh::SharedSecret, ()> {
            let mut node_secret = match recipient {
                Recipient::Node => Ok(self.node_secret),
                Recipient::PhantomNode => Err(()),
            }?;
            if let Some(tweak) = tweak {
                node_secret = node_secret.mul_tweak(tweak).map_err(|_| ())?;
            }
            Ok(ecdh::SharedSecret::new(other_key, &node_secret))
        }

        fn get_inbound_payment_key(&self) -> ExpandedKey { unreachable!() }
        fn sign_invoice(&self, _: &RawBolt11Invoice, _: Recipient) -> Result<ecdsa::RecoverableSignature, ()> { unreachable!() }
        fn sign_bolt12_invoice(&self, _invoice: &UnsignedBolt12Invoice) -> Result<schnorr::Signature, ()> { unreachable!() }
        fn sign_gossip_message(&self, _msg: UnsignedGossipMessage) -> Result<ecdsa::Signature, ()> { unreachable!() }
    }

    struct MsgHandler {
        expected_pubkey: secp256k1::PublicKey,
        pubkey_connected: mpsc::Sender<()>,
        pubkey_disconnected: mpsc::Sender<()>,
        disconnected_flag: AtomicBool,
        msg_events: Mutex<Vec<MessageSendEvent>>,
    }
    #[rustfmt::skip]
    impl RoutingMessageHandler for MsgHandler {
        fn handle_node_announcement(&self, _pk: Option<secp256k1::PublicKey>, _msg: &NodeAnnouncement) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn handle_channel_announcement(&self, _pk: Option<secp256k1::PublicKey>, _msg: &ChannelAnnouncement) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn handle_channel_update(&self, _pk: Option<secp256k1::PublicKey>, _msg: &ChannelUpdate) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn get_next_channel_announcement(&self, _starting_point: u64) -> Option<(ChannelAnnouncement, Option<ChannelUpdate>, Option<ChannelUpdate>)> {
            None
        }
        fn get_next_node_announcement(&self, _starting_point: Option<&NodeId>) -> Option<NodeAnnouncement> {
            None
        }
        fn peer_connected(&self, _their_node_id: secp256k1::PublicKey, _init_msg: &Init, _inbound: bool) -> Result<(), ()> {
            Ok(())
        }
        fn handle_reply_channel_range(&self, _their_node_id: secp256k1::PublicKey, _msg: ReplyChannelRange) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_reply_short_channel_ids_end(&self, _their_node_id: secp256k1::PublicKey, _msg: ReplyShortChannelIdsEnd) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_query_channel_range(&self, _their_node_id: secp256k1::PublicKey, _msg: QueryChannelRange) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_query_short_channel_ids(&self, _their_node_id: secp256k1::PublicKey, _msg: QueryShortChannelIds) -> Result<(), LightningError> {
            Ok(())
        }
        fn provided_node_features(&self) -> NodeFeatures {
            NodeFeatures::empty()
        }
        fn provided_init_features( &self, _their_node_id: secp256k1::PublicKey) -> InitFeatures {
            InitFeatures::empty()
        }
        fn processing_queue_high(&self) -> bool { false }
    }
    #[rustfmt::skip]
    impl ChannelMessageHandler for MsgHandler {
        fn peer_disconnected(&self, their_node_id: secp256k1::PublicKey) {
            if their_node_id == self.expected_pubkey {
                self.disconnected_flag.store(true, Ordering::SeqCst);
                self.pubkey_disconnected.clone().try_send(()).unwrap();
            }
        }
        fn peer_connected(
            &self,
            their_node_id: secp256k1::PublicKey,
            _init_msg: &Init,
            _inbound: bool,
        ) -> Result<(), ()> {
            if their_node_id == self.expected_pubkey {
                self.pubkey_connected.clone().try_send(()).unwrap();
            }
            Ok(())
        }
        fn get_chain_hashes(&self) -> Option<Vec<ChainHash>> {
            Some(vec![ChainHash::using_genesis_block(Network::Testnet)])
        }

        fn handle_open_channel(&self, _their_node_id: secp256k1::PublicKey, _msg: &OpenChannel) {}
        fn handle_accept_channel(&self, _their_node_id: secp256k1::PublicKey, _msg: &AcceptChannel) {}
        fn handle_funding_created(&self, _their_node_id: secp256k1::PublicKey, _msg: &FundingCreated) {}
        fn handle_funding_signed(&self, _their_node_id: secp256k1::PublicKey, _msg: &FundingSigned) {}
        fn handle_channel_ready(&self, _their_node_id: secp256k1::PublicKey, _msg: &ChannelReady) {}
        fn handle_shutdown(&self, _their_node_id: secp256k1::PublicKey, _msg: &Shutdown) {}
        fn handle_closing_signed(&self, _their_node_id: secp256k1::PublicKey, _msg: &ClosingSigned) {}
        fn handle_update_add_htlc(&self, _their_node_id: secp256k1::PublicKey, _msg: &UpdateAddHTLC) {}
        fn handle_update_fulfill_htlc(&self, _their_node_id: secp256k1::PublicKey, _msg: &UpdateFulfillHTLC) {}
        fn handle_update_fail_htlc(&self, _their_node_id: secp256k1::PublicKey, _msg: &UpdateFailHTLC) {}
        fn handle_update_fail_malformed_htlc(&self, _their_node_id: secp256k1::PublicKey, _msg: &UpdateFailMalformedHTLC) {}
        fn handle_commitment_signed(&self, _their_node_id: secp256k1::PublicKey, _msg: &CommitmentSigned) {}
        fn handle_revoke_and_ack(&self, _their_node_id: secp256k1::PublicKey, _msg: &RevokeAndACK) {}
        fn handle_update_fee(&self, _their_node_id: secp256k1::PublicKey, _msg: &UpdateFee) {}
        fn handle_announcement_signatures(&self, _their_node_id: secp256k1::PublicKey, _msg: &AnnouncementSignatures) {}
        fn handle_channel_update(&self, _their_node_id: secp256k1::PublicKey, _msg: &ChannelUpdate) {}
        fn handle_open_channel_v2(&self, _their_node_id: secp256k1::PublicKey, _msg: &OpenChannelV2) {}
        fn handle_accept_channel_v2(&self, _their_node_id: secp256k1::PublicKey, _msg: &AcceptChannelV2) {}
        fn handle_stfu(&self, _their_node_id: secp256k1::PublicKey, _msg: &Stfu) {}
        fn handle_tx_add_input(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxAddInput) {}
        fn handle_tx_add_output(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxAddOutput) {}
        fn handle_tx_remove_input(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxRemoveInput) {}
        fn handle_tx_remove_output(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxRemoveOutput) {}
        fn handle_tx_complete(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxComplete) {}
        fn handle_tx_signatures(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxSignatures) {}
        fn handle_tx_init_rbf(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxInitRbf) {}
        fn handle_tx_ack_rbf(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxAckRbf) {}
        fn handle_tx_abort(&self, _their_node_id: secp256k1::PublicKey, _msg: &TxAbort) {}
        fn handle_channel_reestablish(&self, _their_node_id: secp256k1::PublicKey, _msg: &ChannelReestablish) { }
        fn handle_error(&self, _their_node_id: secp256k1::PublicKey, _msg: &ErrorMessage) {}
        fn provided_node_features(&self) -> NodeFeatures { NodeFeatures::empty() }
        fn provided_init_features( &self, _their_node_id: secp256k1::PublicKey,) -> InitFeatures { InitFeatures::empty() }
        fn message_received(&self) {}
    }
    impl MessageSendEventsProvider for MsgHandler {
        fn get_and_clear_pending_msg_events(&self) -> Vec<MessageSendEvent> {
            let mut ret = Vec::new();
            mem::swap(&mut *self.msg_events.lock().unwrap(), &mut ret);
            ret
        }
    }
}
