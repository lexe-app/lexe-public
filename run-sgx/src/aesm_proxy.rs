//! A small SGX enclave [`UsercallExtension`] that proxies TCP connections from
//! the enclave targetting "aesm.local" and forwards them to the local AESM
//! service's unix socket.

use std::{
    future::Future,
    io::{Error as IoError, Result as IoResult},
    os::unix::net::UnixStream as StdUnixStream,
    pin::Pin,
    task::{Context, Poll},
};

use enclave_runner::usercalls::{AsyncStream, UsercallExtension};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::UnixStream,
};

const DEFAULT_AESM_SOCKET_PATH: &str = "/var/run/aesmd/aesm.socket";
const AESM_FAKE_DNS_NAME: &str = "aesm.local";

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Enclave-facing service for the Enclave to interact with the Intel AESM.
/// Provided as an enclave [`UsercallExtension`].
///
/// Listens for TCP connections to "aesm.local" from the enclave. These
/// connections are then transparently proxied to the local AESM unix socket.
#[derive(Debug)]
pub struct AesmProxy;

// TODO(phlip9): change [`aesm_client`] to open a fresh TCP connection per req

/// A single proxied connection to the AESM unix socket.
///
/// ## Implementation Note
///
/// 1. We can't just open one [`UnixStream`] to the AESM and use that for all
///    requests. Instead, the AESM expects a new [`UnixStream`] connection for
///    _each_ request/response pair.
///
/// 2. The [`aesm_client`] impl for enclaves uses only one [`TcpStream`] for all
///    RPCs.
///
/// 3. When the enclave makes a new request we open a fresh [`UnixStream`]
///    connection.
///
/// 4. To keep things simple for now, we assume the enclave only uses the
///    [`TcpStream`] like a simplex channel (read _xor_ write). That way we can
///    easily detect a new request just by waiting for the first write after
///    some reads, without actually understanding the protobuf protocol.
///
/// [`TcpStream`]: tokio::net::TcpStream
pub struct AesmProxyStream {
    aesm_sock: Option<UnixStream>,
    just_read: bool,
}

// -- impl AesmProxy -- //

impl UsercallExtension for AesmProxy {
    // When an enclave calls `TcpStream::connect(addr)`, our `UsercallExtension`
    // gets called to possibly hook in its own `AsyncStream` as the backing
    // socket.
    //
    // Here, we return a stream that will forward AESM protobuf messages to/from
    // the AESM Unix Domain Socket (UDS).
    fn connect_stream<'fut>(
        &'fut self,
        addr: &'fut str,
        local_addr: Option<&'fut mut String>,
        peer_addr: Option<&'fut mut String>,
    ) -> BoxFuture<'fut, IoResult<Option<Box<dyn AsyncStream>>>> {
        let fut = async move {
            if addr == AESM_FAKE_DNS_NAME {
                // TODO(phlip9): what to do here?
                if let Some(local_addr) = local_addr {
                    *local_addr = "enclave.local".to_string();
                }
                if let Some(peer_addr) = peer_addr {
                    *peer_addr = AESM_FAKE_DNS_NAME.to_string();
                }

                let stream = AesmProxyStream::new();
                Ok(Some(Box::new(stream) as _))
            } else {
                Ok(None)
            }
        };

        Box::pin(fut)
    }
}

// -- impl AesmProxyStream -- //

impl AesmProxyStream {
    pub fn new() -> Self {
        Self {
            aesm_sock: None,
            just_read: false,
        }
    }
}

impl Default for AesmProxyStream {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncWrite for AesmProxyStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<IoResult<usize>> {
        // This is the first request or the enclave has "just read" a response
        // from the AESM. We need to open a fresh socket to handle the next
        // request.
        if self.just_read || self.aesm_sock.is_none() {
            // Blocking open here just to keep things simple for now. O/w would
            // need separate connection state + pin project :/
            let aesm_sock = StdUnixStream::connect(DEFAULT_AESM_SOCKET_PATH)?;
            let aesm_sock = UnixStream::from_std(aesm_sock)?;
            self.aesm_sock = Some(aesm_sock);
            self.just_read = false;
        }

        Pin::new(self.aesm_sock.as_mut().unwrap()).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<IoResult<()>> {
        match self.aesm_sock.as_mut() {
            Some(aesm_sock) => Pin::new(aesm_sock).poll_flush(cx),
            None => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<IoResult<()>> {
        match self.aesm_sock.as_mut() {
            Some(aesm_sock) => Pin::new(aesm_sock).poll_shutdown(cx),
            None => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncRead for AesmProxyStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        self.just_read = true;

        let aesm_sock = self.aesm_sock.as_mut().ok_or_else(|| {
            IoError::other("Enclave must write a request first before reading")
        })?;
        Pin::new(aesm_sock).poll_read(cx, buf)
    }
}
