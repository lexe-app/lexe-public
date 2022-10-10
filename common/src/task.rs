use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use once_cell::sync::Lazy;
use tokio::sync::{mpsc, oneshot};
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
    #[inline]
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

    #[inline]
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

    /// Make await'ing on an `LxTask` return the name along with the result:
    /// `(Result<T, JoinError>, name)`
    #[inline]
    pub fn result_with_name(self) -> LxTaskWithName<T> {
        LxTaskWithName(self)
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

/// A small helper that gives a human-readable label for a joined task's
/// resulting output.
pub fn joined_task_state_label(
    join_res: Result<(), JoinError>,
) -> &'static str {
    match join_res {
        Ok(()) => "finished",
        Err(err) if err.is_cancelled() => "canceled",
        Err(err) if err.is_panic() => "panicked",
        _ => "(unknown join error)",
    }
}

/// A small wrapper `Future` for `LxTask` that returns the task name alongside
/// the task output.
pub struct LxTaskWithName<T>(LxTask<T>);

impl<T> LxTaskWithName<T> {
    #[inline]
    pub fn name(&self) -> &'static str {
        self.0.name()
    }
}

impl<T> Future for LxTaskWithName<T> {
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

type BoxFut = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;
type BoxFutWithTx = (BoxFut, oneshot::Sender<()>);

/// A lazily spawned thread for running async tasks from inside a sync context
/// (that is itself inside an async context... don't ask...). Please don't use
/// this unless you have a very good reason. : )
///
/// XXX: remove when LDK `EventHandler` trait is made properly async.
pub struct LazyBlockingTaskRt(Lazy<mpsc::Sender<BoxFutWithTx>>);

impl LazyBlockingTaskRt {
    pub const fn new() -> Self {
        Self(Lazy::new(|| {
            // Only run one task at a time.
            let (task_tx, mut task_rx) = mpsc::channel::<BoxFutWithTx>(1);

            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();

                rt.block_on(async move {
                    // Only run one task at a time.
                    while let Some((task, res_tx)) = task_rx.recv().await {
                        task.await;
                        let _ = res_tx.send(());
                    }
                });
            });

            task_tx
        }))
    }

    /// Block on `fut` until it runs to completion. The only difference b/w this
    /// and [`tokio::task::block_in_place`] is that this "technically" works
    /// inside a current-thread runtime (though it's certainly not recommended).
    pub fn block_on(&self, fut: impl Future<Output = ()> + Send + 'static) {
        self.block_on_boxed(Box::pin(fut));
    }

    pub fn block_on_boxed(&self, task: BoxFut) {
        let blocking_task_tx = &*self.0;
        let (res_tx, res_rx) = oneshot::channel();

        // NOTE: _must_ be `futures::executor::block_on`, as
        // `Handle::current().block_on()` will (sensibly) panic if used in an
        // async current-thread runtime, since it will block all tasks in the
        // rt.
        futures::executor::block_on(async {
            if blocking_task_tx.send((task, res_tx)).await.is_err() {
                panic!("event handler runtime task channel closed");
            }
            res_rx
                .await
                .expect("event handler runtime panicked while running task?");
        });
    }
}

impl Default for LazyBlockingTaskRt {
    fn default() -> Self {
        Self::new()
    }
}
