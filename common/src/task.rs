use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::task::{JoinError, JoinHandle};
use tracing::error;

/// A thin wrapper around [`tokio::task::JoinHandle`] that adds the
/// `#[must_use]` lint to ensure that all spawned tasks are joined or explictly
/// annotated that no joining is required.
///
/// `LxTask` also includes an optional task name for improved debuggability.
/// [`LxTask::result_with_name`] will return a Future of the task result
/// alongside the task name.
#[must_use]
pub struct LxTask<T> {
    task: JoinHandle<T>,
    name: &'static str,
}

impl<T> LxTask<T> {
    #[allow(clippy::disallowed_methods)]
    pub fn spawn_named<F>(name: &'static str, future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self {
            task: tokio::spawn(future),
            name,
        }
    }

    pub fn spawn<F>(future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self::spawn_named("<no-name>", future)
    }

    #[inline]
    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn result_with_name(self) -> LxTaskWithNameFut<T> {
        LxTaskWithNameFut(self)
    }
}

impl<T> Future for LxTask<T> {
    type Output = Result<T, JoinError>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        let result = match Pin::new(&mut self.task).poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };

        let result = match result {
            Ok(val) => Ok(val),
            Err(join_err) => match join_err.try_into_panic() {
                // If the inner spawned task panicked, then propagate the panic
                // to the current task.
                Ok(panic_reason) => {
                    error!("'{}' task panicked!!!", self.name());
                    std::panic::resume_unwind(panic_reason)
                }
                Err(join_err) => Err(join_err),
            },
        };

        Poll::Ready(result)
    }
}

pub fn join_res_label(join_res: Result<(), JoinError>) -> &'static str {
    match join_res {
        Ok(()) => "finished",
        Err(err) if err.is_cancelled() => "canceled",
        Err(err) if err.is_panic() => "panicked",
        _ => "(unknown join error)",
    }
}

pub struct LxTaskWithNameFut<T>(LxTask<T>);

impl<T> LxTaskWithNameFut<T> {
    #[inline]
    pub fn name(&self) -> &'static str {
        self.0.name()
    }
}

impl<T> Future for LxTaskWithNameFut<T> {
    type Output = (Result<T, JoinError>, &'static str);

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        let result = match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };

        let name = self.name();

        let result = match result {
            Ok(val) => (Ok(val), name),
            Err(err) => (Err(err), name),
        };

        Poll::Ready(result)
    }
}
