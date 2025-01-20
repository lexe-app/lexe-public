// TODO(phlip9): remove
#![allow(dead_code)]

use std::{
    future::Future,
    hash::Hash,
    io,
    marker::PhantomData,
    mem::size_of,
    num::NonZeroUsize,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, ensure, Context};
use common::{api::user::NodePk, backoff, ln::addr::LxSocketAddress, Apply};
use lightning::ln::peer_handler::PeerHandleError;
use lightning_net_tokio::Executor;
use tokio::{
    io::Interest,
    net::TcpStream,
    sync::{
        mpsc::{self, error::TrySendError},
        Notify,
    },
    time,
};
use tracing::{debug, instrument, trace, warn};

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
        #[allow(clippy::disallowed_methods)] // Have to return `JoinHandle` here
        tokio::spawn(tracing::Instrument::in_current_span(fut))
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
        .context("TcpStream::connect() failed")?;

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

//
// --- WIP lightning-net-tokio replacement ---
//

/// Used to generate the next unique ID for a new connection.
static CONNECTION_ID: AtomicU64 = AtomicU64::new(0);

/// A handle to a [`Connection`]. Used to request the socket to send data.
/// Cheaply cloneable.
#[derive(Clone)]
struct ConnectionTx {
    /// Update the connection control state (disconnect/pause reads).
    ctl: Arc<ConnectionCtl>,
    /// Send write data requests to [`Connection`] for writing to socket.
    tx: mpsc::Sender<Box<[u8]>>,
}

const _: [(); 2 * size_of::<usize>()] = [(); size_of::<ConnectionTx>()];

/// A Lightning p2p connection. Wraps a tokio [`TcpStream`] in additional logic
/// required to interface with LDK's `PeerManager`.
struct Connection<CM, PS, PM> {
    /// Get notified of connection control updates (disconnect/resume_read).
    ctl: Arc<ConnectionCtl>,

    /// Receive write data requests from [`ConnectionTx::send_data`].
    rx: mpsc::Receiver<Box<[u8]>>,

    /// Handle to LDK PeerManager.
    peer_manager: PM,

    /// The underlying TCP socket.
    stream: TcpStream,

    /// The next enqueued write.
    write_buf: Option<Box<[u8]>>,
    /// If we didn't manage to fully write `write_buf` to the socket, then
    /// we'll start our next write at this offset in `write_buf`.
    write_offset: usize,

    /// A fixed buffer to hold data read from the socket, before we immediately
    /// pass it on to `PeerManager::read_event`.
    read_buf: Box<[u8; 8 << 10]>,

    /// LDK requires us to pass a full `ConnectionTx` to `read_event` etc...,
    /// so we have to hold onto an extra one inside `Connection`...
    conn_tx: ConnectionTx,

    // HACK: make generics work
    phantom_cm: PhantomData<CM>,
    phantom_ps: PhantomData<PS>,
}

/// [`Connection`] control state. Used to notify the [`Connection`] that it
/// should disconnect or resume reads.
///
/// This control-plane state is intentionally separate from the `tx` data
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

// NOTE: change `ConnectionCtl::resume_read` if more states are added for some
// reason.

/// Connection is running normally.
const STATE_NORMAL: usize = 0;
/// Connection has its reads paused.
const STATE_PAUSE_READ: usize = 1;
/// Connection is disconnected or in the process of disconnecting.
const STATE_DISCONNECT: usize = 2;

/// The reason for a [`Connection`] disconnect.
#[derive(Debug)]
enum Disconnect {
    /// Socket error (peer immediate disconnect).
    Socket(std::io::ErrorKind),
    /// We can't read from socket anymore--remote peer write half-close.
    ReadClosed,
    /// We can't write to the socket anymore--remote peer read half-close.
    WriteClosed,
    /// PeerManager called [`ConnectionTx::disconnect_socket`].
    PeerManager,
}

const _: [(); 1] = [(); size_of::<Result<(), Disconnect>>()];

/// A trait that encapsulates the exact interface we require from the LDK
/// `PeerManager` as far as [`Connection`] is concerned.
trait PeerManagerTrait<CM, PS>: Clone {
    /// Returns `true` if we're connected to a peer with [`NodePk`].
    fn is_connected(&self, node_pk: &NodePk) -> bool;

    /// Register a new inbound connection with the `PeerManager`. Returns an
    /// initial write that should be sent immediately. May return `Err` to
    /// reject the new connection, which should then be disconnected.
    fn new_outbound_connection(
        &self,
        node_pk: &NodePk,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<Vec<u8>, PeerHandleError>;

    /// Register a new outbound connection with the `PeerManager`. May return
    /// `Err` to reject the new connection, which should then be disconnected.
    fn new_inbound_connection(
        &self,
        conn_tx: ConnectionTx,
        addr: Option<LxSocketAddress>,
    ) -> Result<(), PeerHandleError>;

    /// Notify the `PeerManager` that the connection associated with `conn_tx`
    /// has disconnected.
    ///
    /// This fn is idempotent, so it's safe to call multiple times.
    fn socket_disconnected(&self, conn_tx: &ConnectionTx);

    /// Feed the `PeerManager` new data read from the socket associated with
    /// `conn_tx`.
    ///
    /// Returns `Ok(true)`, if the connection should apply backpressure on
    /// reads. That means it should avoid calling `PeerManager::read_event`
    /// until the next `ConnectionTx::send_data(.., resume_read: true)` request.
    ///
    /// Returns `Err` if the socket should be disconnected. You do not need to
    /// call `socket_disconnected`.
    ///
    /// You SHOULD call `PeerManager::process_events` sometime after a
    /// `read_event` to generate subsequent `send_data` calls.
    ///
    /// This will NOT call `send_data` to avoid re-entrancy reasons.
    fn read_event(
        &self,
        conn_tx: &mut ConnectionTx,
        data: &[u8],
    ) -> Result<bool, PeerHandleError>;

    /// Drive the `PeerManager` state machine to handle new `read_event`s.
    ///
    /// May call `send_data` on various peer `ConnectionTx`'s.
    fn process_events(&self);

    /// Notify the `PeerManager` that the connection associated with `conn_tx`
    /// now has room for more `send_data` write requests.
    ///
    /// May call `send_data` on the `conn_tx` multiple times.
    fn write_buffer_space_avail(
        &self,
        conn_tx: &mut ConnectionTx,
    ) -> Result<(), PeerHandleError>;
}

//
// --- impl Connection ---
//

impl<CM, PS, PM: PeerManagerTrait<CM, PS>> Connection<CM, PS, PM> {
    fn new(peer_manager: PM, stream: TcpStream) -> (ConnectionTx, Self) {
        let ctl = Arc::new(ConnectionCtl::new());
        let (tx, rx) = mpsc::channel(8);
        let conn_tx = ConnectionTx {
            ctl: ctl.clone(),
            tx,
        };
        let conn = Self {
            ctl,
            rx,
            stream,
            peer_manager,
            write_buf: None,
            write_offset: 0,
            read_buf: Box::new([0u8; 8 << 10]),
            conn_tx: conn_tx.clone(),
            phantom_cm: PhantomData,
            phantom_ps: PhantomData,
        };
        (conn_tx, conn)
    }

    fn setup_outbound(
        peer_manager: &PM,
        stream: TcpStream,
        addr: LxSocketAddress,
        node_pk: &NodePk,
    ) -> Result<Self, PeerHandleError> {
        let (conn_tx, mut conn) = Self::new(peer_manager.clone(), stream);
        let initial_write = peer_manager.new_outbound_connection(
            node_pk,
            conn_tx,
            Some(addr),
        )?;
        conn.write_buf = Some(initial_write.into());
        Ok(conn)
    }

    fn setup_inbound(
        peer_manager: &PM,
        stream: TcpStream,
    ) -> Result<Self, PeerHandleError> {
        let addr = stream
            .peer_addr()
            .ok()
            .and_then(|sockaddr| LxSocketAddress::try_from(sockaddr).ok());
        let (conn_tx, conn) = Self::new(peer_manager.clone(), stream);

        // Fortanix SGX doesn't support socket half-close...

        match peer_manager.new_inbound_connection(conn_tx, addr) {
            Ok(()) => Ok(conn),
            Err(err) => Err(err),
        }
    }

    #[instrument(skip_all, name = "(p2p-conn)")]
    async fn run(mut self) {
        trace!("start");

        let disconnect = loop {
            // Read new control state for this iter.
            //
            // If `pause_read=true`, we'll avoid calling
            // `PeerManager::read_event` until the next
            // `ConnectionTx::send_data(.., resume_read: true)` request.
            let pause_read = match self.read_ctl_state() {
                Ok(pause_read) => pause_read,
                Err(disconnect) => break disconnect,
            };

            // The socket events (if any) we want to be notified of in this
            // select iter.
            let interest = self.socket_interest(pause_read);

            trace!(
                write_buf = ?self.write_buf.as_ref().map(|b| b.len()),
                write_offset = %self.write_offset,
                %pause_read,
                ?interest,
            );

            tokio::select! {
                // `ConnectionCtl` notified disconnect/resume_read
                // -> nothing (we'll pick up the new state on the next iter)
                () = self.ctl.notify.notified() => {},

                // `ConnectionTx::try_send_data(data=&[..])`
                // -> enqueue for writing to socket
                // -> notify `PeerManager::write_buffer_space_avail`
                req = self.rx.recv(), if self.write_buf.is_none() => {
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
                // -> is_writable => write_buf[write_offset..] -> stream.try_write
                // -> is_readable => stream.try_read -> read_buf -> PeerManager::read_event
                res = self.stream.ready(interest.unwrap()), if interest.is_some() => {
                    let ready = match res {
                        Ok(ready) => ready,
                        Err(err) => break Disconnect::Socket(err.kind()),
                    };

                    // If socket says it's ready to write -> try to write.
                    if ready.is_writable() {
                        if let Err(disconnect) = self.try_write_buf() {
                            break disconnect;
                        }
                    };

                    // If socket says it's ready to read -> try to read.
                    let bytes_read: Option<NonZeroUsize> = if ready.is_readable() {
                        match self.try_read_buf() {
                            Ok(bytes_read) => bytes_read,
                            Err(disconnect) => break disconnect,
                        }
                    } else {
                        None
                    };

                    // Tell `PeerManager` about data we just read.
                    if let Some(bytes_read) = bytes_read {
                        trace!(%bytes_read);

                        let data = &self.read_buf[..bytes_read.get()];
                        let pause_read = match self.peer_manager.read_event(&mut self.conn_tx, data) {
                            // It may want us to pause reads
                            Ok(pause_read) => pause_read,
                            Err(PeerHandleError {}) => break Disconnect::PeerManager,
                        };

                        let state = if pause_read { STATE_PAUSE_READ } else { STATE_NORMAL };
                        if let Err(disconnect) = self.set_ctl_state(state) {
                            break disconnect;
                        }

                        // TODO(phlip9): move into separate task
                        self.peer_manager.process_events();
                    }
                }
            }

            // TODO(phlip9): remove?
            tokio::task::yield_now().await;
        };

        if !disconnect.is_peer_manager() {
            self.peer_manager.socket_disconnected(&self.conn_tx);
        }

        // TODO(phlip9): log socket error
        // TODO(phlip9): graceful shutdown

        trace!(?disconnect);
    }

    /// Read [`ConnectionCtl::state`] and maybe resume reads or disconnect.
    fn read_ctl_state(&mut self) -> Result<bool, Disconnect> {
        let state = self.ctl.state.load(Ordering::SeqCst);
        if state != STATE_DISCONNECT {
            let pause_read = state == STATE_PAUSE_READ;
            Ok(pause_read)
        } else {
            Err(Disconnect::PeerManager)
        }
    }

    /// Set [`ConnectionCtl::state`]. If we raced with a disconnect, return Err.
    fn set_ctl_state(&mut self, state: usize) -> Result<(), Disconnect> {
        let prev = self.ctl.state.swap(state, Ordering::SeqCst);
        if prev != STATE_DISCONNECT {
            Ok(())
        } else {
            Err(Disconnect::PeerManager)
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

    /// Tell `PeerManager` we have space for more write data requests in the
    /// mpsc queue.
    fn notify_send_data_channel_space_avail(
        &mut self,
    ) -> Result<(), Disconnect> {
        self.peer_manager
            .write_buffer_space_avail(&mut self.conn_tx)
            .map_err(|PeerHandleError {}| Disconnect::PeerManager)
    }

    /// Attempt a `stream.try_write(&write_buf[write_offset..])`. Returns `true`
    /// if another write might succeed immediately afterward.
    fn try_write_buf(&mut self) -> Result<(), Disconnect> {
        let write_buf: &[u8] = self.write_buf.as_ref().expect(
            "we should only get write readiness if write_buf.is_some()",
        );
        assert!(!write_buf.is_empty());

        let to_write = &write_buf[self.write_offset..];
        assert!(!to_write.is_empty());

        // TODO(phlip9): remove. testing partial writes.
        #[cfg(test)]
        let to_write = {
            let to_write_len = std::cmp::min(512, to_write.len());
            &to_write[..to_write_len]
        };

        let _bytes_written = match self.stream.try_write(to_write) {
            // Wrote some bytes -> update `write_buf`
            Ok(bytes_written) => {
                let bytes_written = match NonZeroUsize::new(bytes_written) {
                    // write=0 => Remote peer read half-close
                    None => return Err(Disconnect::WriteClosed),
                    Some(bytes_written) => bytes_written,
                };

                trace!(%bytes_written);

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

        Ok(())
    }

    /// Attempt a `stream.try_read(&mut read_buf)`. Returns the number of bytes
    /// read, if any.
    fn try_read_buf(&mut self) -> Result<Option<NonZeroUsize>, Disconnect> {
        match self.stream.try_read(self.read_buf.as_mut_slice()) {
            Ok(bytes_read) => match NonZeroUsize::new(bytes_read) {
                // read=0 => Remote peer write half-close
                None => Err(Disconnect::ReadClosed),
                Some(bytes_read) => Ok(Some(bytes_read)),
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
    /// `PeerManager::write_buffer_space_avail` when it has room for more
    /// writes.
    fn send_data(&mut self, data: &[u8], resume_read: bool) -> usize {
        let bytes_enqueued = self.try_send_data(data);
        if resume_read {
            self.ctl.resume_read();
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
        match self.tx.try_reserve() {
            // Enqueue `data` to be written
            Ok(send_permit) => {
                let write_len = data.len();
                trace!(%write_len, "ConnectionTx =>");
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

    /// Notify the [`Connection`] that the `PeerManager` wants to disconnect.
    fn disconnect_socket(&mut self) {
        trace!("ConnectionTx => disconnect");
        self.ctl.disconnect()
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
        Self {
            id: CONNECTION_ID.fetch_add(1, Ordering::Relaxed),
            state: AtomicUsize::new(STATE_NORMAL),
            notify: Notify::new(),
        }
    }

    /// Tell [`Connection`] to disconnect.
    fn disconnect(&self) {
        self.state.store(STATE_DISCONNECT, Ordering::SeqCst);
        self.notify.notify_one();
    }

    /// Tell [`Connection`] to resume reads (if not already resumed or
    /// disconnected).
    fn resume_read(&self) {
        let curr = self.state.load(Ordering::SeqCst);

        // If reads are paused, then try to resume back to `NORMAL`.
        if curr == STATE_PAUSE_READ {
            let new = STATE_NORMAL;
            let res = self.state.compare_exchange(
                curr,
                new,
                Ordering::SeqCst,
                Ordering::SeqCst,
            );
            match res {
                // We succeeded in setting state -> `NORMAL`, notify the
                // connection that state changed.
                Ok(_) => {
                    trace!("ConnectionTx => resume read");
                    self.notify.notify_one()
                }
                // NOTE: don't need to loop:
                // case actual == STATE_NORMAL: => someone raced to resume
                // case actual == STATE_DISCONNECT: => give up anyway
                Err(_actual) => {}
            }
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

//
// --- impl PeerManagerTrait ---
//

// TODO(phlip9): uncomment after LexePeerManager switches to this connection
// impl

// impl<CM, PS, LPM, T> PeerManagerTrait<CM, PS> for T
// where
//     T: Deref<Target = LPM> + Clone,
//     CM: LexeChannelManager<PS>,
//     LPM: LexePeerManager<CM, PS>,
//     PS: LexePersister,
// {
//     fn is_connected(&self, node_pk: &NodePk) -> bool {
//         // TODO(max): This LDK fn is O(n) in the # of peers...
//         self.as_ref().peer_by_node_id(&node_pk.0).is_some()
//     }
//
//     fn new_outbound_connection(
//         &self,
//         node_pk: &NodePk,
//         conn_tx: ConnectionTx,
//         addr: Option<LxSocketAddress>,
//     ) -> Result<Vec<u8>, PeerHandleError> {
//         self.as_ref().new_outbound_connection(
//             node_pk.0,
//             conn_tx,
//             addr.map(SocketAddress::from),
//         )
//     }
//
//     fn new_inbound_connection(
//         &self,
//         conn_tx: ConnectionTx,
//         addr: Option<LxSocketAddress>,
//     ) -> Result<(), PeerHandleError> {
//         self.as_ref()
//             .new_inbound_connection(conn_tx, addr.map(SocketAddress::from))
//     }
//
//     fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
//         self.as_ref().socket_disconnected(conn_tx)
//     }
//
//     fn read_event(
//         &self,
//         conn_tx: &mut ConnectionTx,
//         data: &[u8],
//     ) -> Result<bool, PeerHandleError> {
//         self.as_ref().read_event(conn_tx, data)
//     }
//
//     fn process_events(&self) {
//         self.as_ref().process_events()
//     }
//
//     fn write_buffer_space_avail(
//         &self,
//         conn_tx: &mut ConnectionTx,
//     ) -> Result<(), PeerHandleError> {
//         self.as_ref().write_buffer_space_avail(conn_tx)
//     }
// }

#[cfg(test)]
mod test {
    use std::{
        cmp::min,
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use common::{rng::FastRng, task::LxTask};
    use io::BufRead;
    use rand::RngCore;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{ldk_test::make_tcp_connection, *};
    use crate::logger;

    #[tokio::test]
    async fn test_echo() {
        const MSG_LEN: usize = 123_456;
        const WRITE_LEN: usize = 969;

        logger::init_for_testing();
        let mut rng = FastRng::from_u64(948298498);

        let (tcp_a, tcp_b) = make_tcp_connection().await;
        let peer_manager = Arc::new(Mutex::new(EchoPeerManager::new()));
        let (conn_tx, conn) = Connection::new(peer_manager.clone(), tcp_a);

        let addr = None;
        peer_manager.new_inbound_connection(conn_tx, addr).unwrap();

        let mut msg = vec![0u8; MSG_LEN];
        rng.fill_bytes(&mut msg);

        let write_msg = msg.clone();
        let (mut tcp_b_read, mut tcp_b_write) = tcp_b.into_split();

        let conn_task = LxTask::spawn_named("conn", conn.run());
        let b_task = LxTask::spawn_named("b", async move {
            let write_task = LxTask::spawn_named("write", async move {
                let mut msg = write_msg.as_slice();
                // let msg = write_msg.as_slice();
                // tcp_b_write.write_all(msg).await.unwrap();

                let mut total_written = 0;
                loop {
                    let to_write_len = min(WRITE_LEN, msg.len());
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

                    // TODO(phlip9): remove?
                    tokio::task::yield_now().await;
                }
                assert_eq!(total_written, write_msg.len());

                // write half-close TCP stream
                drop(tcp_b_write);
            });
            let read_task = LxTask::spawn_named("read", async move {
                let mut read_msg = Vec::with_capacity(MSG_LEN);
                tcp_b_read.read_to_end(&mut read_msg).await.unwrap();

                let read_msg_len = read_msg.len();
                let msg_len = msg.len();
                trace!(%read_msg_len, %msg_len);

                // TODO(phlip9): check that all `ConnectionTx` ACK'd writes make
                // it through with graceful shutdown
                assert!(
                    read_msg_len <= msg_len,
                    "left: {read_msg_len}, right: {msg_len}",
                );
                assert_eq!(&read_msg, &msg[..read_msg_len]);

                drop(tcp_b_read);
            });

            // let (mut tcp_b_read, tcp_b_write) =
            tokio::try_join!(read_task, write_task).unwrap();

            // // close TCP stream
            // drop(tcp_b_write);
            //
            // // read EOF
            // let mut buf = Vec::new();
            // tcp_b_read.read_to_end(&mut buf).await.unwrap();
            // assert_eq!(buf.len(), 0);
        });

        tokio::try_join!(conn_task, b_task).unwrap();
    }

    /// PeerManager that just echoes back data
    struct EchoPeerManager {
        peer: Option<EchoPeer>,
    }

    struct EchoPeer {
        conn_tx: ConnectionTx,
        buf: VecDeque<u8>,
    }

    impl EchoPeerManager {
        fn new() -> Self {
            Self { peer: None }
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

    impl PeerManagerTrait<(), ()> for Arc<Mutex<EchoPeerManager>> {
        fn is_connected(&self, _node_pk: &NodePk) -> bool {
            todo!()
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
                locked.peer = None;
            }
        }

        fn read_event(
            &self,
            conn_tx: &mut ConnectionTx,
            data: &[u8],
        ) -> Result<bool, PeerHandleError> {
            if let Some(peer) = self.lock().unwrap().peer_for(conn_tx) {
                peer.read_event(data)
            } else {
                Err(PeerHandleError {})
            }
        }

        fn process_events(&self) {
            while let Some(peer) = self.lock().unwrap().peer.as_mut() {
                let can_send_more = peer.send_next();
                if !can_send_more {
                    break;
                }
            }
        }

        fn write_buffer_space_avail(
            &self,
            conn_tx: &mut ConnectionTx,
        ) -> Result<(), PeerHandleError> {
            loop {
                match self.lock().unwrap().peer_for(conn_tx) {
                    Some(peer) =>
                        if !peer.send_next() {
                            return Ok(());
                        },
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
            }
        }

        fn read_event(&mut self, data: &[u8]) -> Result<bool, PeerHandleError> {
            self.buf.extend(data);
            Ok(false)
        }

        fn send_next(&mut self) -> bool {
            const WRITE_LEN: usize = 696;

            let data = self.buf.fill_buf().unwrap();
            let to_write_len = min(WRITE_LEN, data.len());

            if to_write_len == 0 {
                return false;
            }

            let data = &data[..to_write_len];
            let resume_read = false;
            let written_len = self.conn_tx.send_data(data, resume_read);

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
    use common::{
        rng::{Crng, FastRng},
        task::LxTask,
    };
    use lightning::{
        events::*,
        ln::{
            features::*,
            msgs::*,
            peer_handler::{
                IgnoringMessageHandler, MessageHandler, PeerManager,
            },
        },
        offers::{
            invoice::UnsignedBolt12Invoice,
            invoice_request::UnsignedInvoiceRequest,
        },
        routing::gossip::NodeId,
        sign::{KeyMaterial, NodeSigner, Recipient},
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
        let a_manager: TestPeerManager = Arc::new(PeerManager::new(
            a_msg_handler,
            0,
            &[1; 32],
            logger::LexeTracingLogger::new(),
            Arc::new(TestNodeSigner::new(a_key)),
        ));

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
        let b_manager = Arc::new(PeerManager::new(
            b_msg_handler,
            0,
            &[2; 32],
            logger::LexeTracingLogger::new(),
            Arc::new(TestNodeSigner::new(b_key)),
        ));

        let (tcp_a, tcp_b) = make_tcp_connection().await;

        let addr_b =
            LxSocketAddress::try_from(tcp_a.peer_addr().unwrap()).unwrap();
        let conn_a = Connection::setup_outbound(
            &a_manager,
            tcp_a,
            addr_b,
            &NodePk(b_pub),
        )
        .unwrap();
        let fut_a = LxTask::spawn(conn_a.run());

        let conn_b = Connection::setup_inbound(&b_manager, tcp_b).unwrap();
        let fut_b = LxTask::spawn(conn_b.run());

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

    type TestPeerManager = Arc<
        PeerManager<
            ConnectionTx,
            Arc<MsgHandler>,
            Arc<MsgHandler>,
            Arc<IgnoringMessageHandler>,
            logger::LexeTracingLogger,
            Arc<IgnoringMessageHandler>,
            Arc<TestNodeSigner>,
        >,
    >;

    impl PeerManagerTrait<(), ()> for TestPeerManager {
        fn is_connected(&self, node_pk: &NodePk) -> bool {
            self.as_ref().peer_by_node_id(&node_pk.0).is_some()
        }

        fn new_outbound_connection(
            &self,
            node_pk: &NodePk,
            conn_tx: ConnectionTx,
            addr: Option<LxSocketAddress>,
        ) -> Result<Vec<u8>, PeerHandleError> {
            self.as_ref().new_outbound_connection(
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
                .new_inbound_connection(conn_tx, addr.map(SocketAddress::from))
        }

        fn socket_disconnected(&self, conn_tx: &ConnectionTx) {
            self.as_ref().socket_disconnected(conn_tx)
        }

        fn read_event(
            &self,
            conn_tx: &mut ConnectionTx,
            data: &[u8],
        ) -> Result<bool, PeerHandleError> {
            self.as_ref().read_event(conn_tx, data)
        }

        fn process_events(&self) {
            self.as_ref().process_events()
        }

        fn write_buffer_space_avail(
            &self,
            conn_tx: &mut ConnectionTx,
        ) -> Result<(), PeerHandleError> {
            self.as_ref().write_buffer_space_avail(conn_tx)
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

        fn get_inbound_payment_key_material(&self) -> KeyMaterial { unreachable!() }
        fn sign_invoice(&self, _: &RawBolt11Invoice, _: Recipient) -> Result<ecdsa::RecoverableSignature, ()> { unreachable!() }
        fn sign_bolt12_invoice_request(&self, _invoice_request: &UnsignedInvoiceRequest) -> Result<schnorr::Signature, ()> { unreachable!() }
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
        fn handle_node_announcement(&self, _msg: &NodeAnnouncement) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn handle_channel_announcement(&self, _msg: &ChannelAnnouncement) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn handle_channel_update(&self, _msg: &ChannelUpdate) -> Result<bool, LightningError> {
            Ok(false)
        }
        fn get_next_channel_announcement(&self, _starting_point: u64) -> Option<(ChannelAnnouncement, Option<ChannelUpdate>, Option<ChannelUpdate>)> {
            None
        }
        fn get_next_node_announcement(&self, _starting_point: Option<&NodeId>) -> Option<NodeAnnouncement> {
            None
        }
        fn peer_connected(&self, _their_node_id: &secp256k1::PublicKey, _init_msg: &Init, _inbound: bool) -> Result<(), ()> {
            Ok(())
        }
        fn handle_reply_channel_range(&self, _their_node_id: &secp256k1::PublicKey, _msg: ReplyChannelRange) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_reply_short_channel_ids_end(&self, _their_node_id: &secp256k1::PublicKey, _msg: ReplyShortChannelIdsEnd) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_query_channel_range(&self, _their_node_id: &secp256k1::PublicKey, _msg: QueryChannelRange) -> Result<(), LightningError> {
            Ok(())
        }
        fn handle_query_short_channel_ids(&self, _their_node_id: &secp256k1::PublicKey, _msg: QueryShortChannelIds) -> Result<(), LightningError> {
            Ok(())
        }
        fn provided_node_features(&self) -> NodeFeatures {
            NodeFeatures::empty()
        }
        fn provided_init_features( &self, _their_node_id: &secp256k1::PublicKey,) -> InitFeatures {
            InitFeatures::empty()
        }
        fn processing_queue_high(&self) -> bool { false }
    }
    #[rustfmt::skip]
    impl ChannelMessageHandler for MsgHandler {
        fn peer_disconnected(&self, their_node_id: &secp256k1::PublicKey) {
            if *their_node_id == self.expected_pubkey {
                self.disconnected_flag.store(true, Ordering::SeqCst);
                self.pubkey_disconnected.clone().try_send(()).unwrap();
            }
        }
        fn peer_connected(
            &self,
            their_node_id: &secp256k1::PublicKey,
            _init_msg: &Init,
            _inbound: bool,
        ) -> Result<(), ()> {
            if *their_node_id == self.expected_pubkey {
                self.pubkey_connected.clone().try_send(()).unwrap();
            }
            Ok(())
        }
        fn get_chain_hashes(&self) -> Option<Vec<ChainHash>> {
            Some(vec![ChainHash::using_genesis_block(Network::Testnet)])
        }

        fn handle_open_channel(&self, _their_node_id: &secp256k1::PublicKey, _msg: &OpenChannel) {}
        fn handle_accept_channel(&self, _their_node_id: &secp256k1::PublicKey, _msg: &AcceptChannel) {}
        fn handle_funding_created(&self, _their_node_id: &secp256k1::PublicKey, _msg: &FundingCreated) {}
        fn handle_funding_signed(&self, _their_node_id: &secp256k1::PublicKey, _msg: &FundingSigned) {}
        fn handle_channel_ready(&self, _their_node_id: &secp256k1::PublicKey, _msg: &ChannelReady) {}
        fn handle_shutdown(&self, _their_node_id: &secp256k1::PublicKey, _msg: &Shutdown) {}
        fn handle_closing_signed(&self, _their_node_id: &secp256k1::PublicKey, _msg: &ClosingSigned) {}
        fn handle_update_add_htlc(&self, _their_node_id: &secp256k1::PublicKey, _msg: &UpdateAddHTLC) {}
        fn handle_update_fulfill_htlc(&self, _their_node_id: &secp256k1::PublicKey, _msg: &UpdateFulfillHTLC) {}
        fn handle_update_fail_htlc(&self, _their_node_id: &secp256k1::PublicKey, _msg: &UpdateFailHTLC) {}
        fn handle_update_fail_malformed_htlc(&self, _their_node_id: &secp256k1::PublicKey, _msg: &UpdateFailMalformedHTLC) {}
        fn handle_commitment_signed(&self, _their_node_id: &secp256k1::PublicKey, _msg: &CommitmentSigned) {}
        fn handle_revoke_and_ack(&self, _their_node_id: &secp256k1::PublicKey, _msg: &RevokeAndACK) {}
        fn handle_update_fee(&self, _their_node_id: &secp256k1::PublicKey, _msg: &UpdateFee) {}
        fn handle_announcement_signatures(&self, _their_node_id: &secp256k1::PublicKey, _msg: &AnnouncementSignatures) {}
        fn handle_channel_update(&self, _their_node_id: &secp256k1::PublicKey, _msg: &ChannelUpdate) {}
        fn handle_open_channel_v2(&self, _their_node_id: &secp256k1::PublicKey, _msg: &OpenChannelV2) {}
        fn handle_accept_channel_v2(&self, _their_node_id: &secp256k1::PublicKey, _msg: &AcceptChannelV2) {}
        fn handle_stfu(&self, _their_node_id: &secp256k1::PublicKey, _msg: &Stfu) {}
        fn handle_tx_add_input(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxAddInput) {}
        fn handle_tx_add_output(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxAddOutput) {}
        fn handle_tx_remove_input(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxRemoveInput) {}
        fn handle_tx_remove_output(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxRemoveOutput) {}
        fn handle_tx_complete(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxComplete) {}
        fn handle_tx_signatures(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxSignatures) {}
        fn handle_tx_init_rbf(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxInitRbf) {}
        fn handle_tx_ack_rbf(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxAckRbf) {}
        fn handle_tx_abort(&self, _their_node_id: &secp256k1::PublicKey, _msg: &TxAbort) {}
        fn handle_channel_reestablish(&self, _their_node_id: &secp256k1::PublicKey, _msg: &ChannelReestablish) { }
        fn handle_error(&self, _their_node_id: &secp256k1::PublicKey, _msg: &ErrorMessage) {}
        fn provided_node_features(&self) -> NodeFeatures { NodeFeatures::empty() }
        fn provided_init_features( &self, _their_node_id: &secp256k1::PublicKey,) -> InitFeatures { InitFeatures::empty() }
    }
    impl MessageSendEventsProvider for MsgHandler {
        fn get_and_clear_pending_msg_events(&self) -> Vec<MessageSendEvent> {
            let mut ret = Vec::new();
            mem::swap(&mut *self.msg_events.lock().unwrap(), &mut ret);
            ret
        }
    }
}
