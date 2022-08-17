//! Core types and data structures used throughout the lexe-node, or which
//! (temporarily) don't fit anywhere else

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use tokio::task::{JoinError, JoinHandle};

/// Type aliases for concrete impls of LDK traits.
mod alias;
/// Types related to the host (Lexe) infrastructure such as the runner, backend
mod host;
/// Types leftover from ldk-sample, used in the EventHandler and REPL.
/// TODO: These should be converted into Lexe newtypes or removed entirely.
mod ldk;

pub use alias::*;
pub use host::*;
pub use ldk::*;

/// A thin wrapper around [`tokio::task::JoinHandle`] that adds the
/// `#[must_use]` lint to ensure that all spawned tasks are joined or explictly
/// annotated that no joining is required.
#[must_use]
pub struct LxTask<T>(JoinHandle<T>);

impl<T> LxTask<T> {
    #[allow(clippy::disallowed_methods)]
    pub fn spawn<F>(future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self(tokio::spawn(future))
    }
}

impl<T> Future for LxTask<T> {
    type Output = Result<T, JoinError>;
    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
