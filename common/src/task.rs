use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use tokio::task::{JoinError, JoinHandle};

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
