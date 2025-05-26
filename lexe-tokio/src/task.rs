use std::{
    borrow::Cow,
    fmt::{self, Display},
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{stream::FuturesUnordered, StreamExt};
use thiserror::Error;
use tokio::{
    sync::mpsc,
    task::{JoinError, JoinHandle},
};
use tracing::{debug, error, info, warn, Instrument};

use crate::notify_once::NotifyOnce;

/// Errors that can occur when joining [`LxTask`]s.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Static task finished prematurely: {name}")]
    PrematureFinish { name: Cow<'static, str> },
    #[error("Some tasks failed to finish on time: {hung_tasks:?}")]
    Hung { hung_tasks: Vec<String> },
}

/// Lexe's 'standard' way of handling structured task concurrency and shutdown.
///
/// - "static" tasks are intended to run until the end of the program lifetime.
///   To prevent partial failures, this helper triggers a shutdown if any static
///   task finishes prematurely.
/// - "ephemeral" tasks are intended to finish without causing the overall
///   program to exit. These can be sent over the `eph_tasks_rx` channel.
/// - All task handles are polled to ensure that any panics are propagated.
/// - After a shutdown signal is received, this helper waits for all remaining
///   tasks to complete (both static and ephemeral), up to a `shutdown_timeout`.
///
/// # Errors
///
/// - If a task finishes prematurely, an error is returned.
/// - If some tasks hang after the shutdown signal, an error is returned.
///
/// NOTE: To propagate panics beyond this function, the callsite must
/// still poll the future returned here, and so on up to the top-level future!
pub async fn try_join_tasks_and_shutdown(
    static_tasks: Vec<LxTask<()>>,
    // A channel over which handles to ephemeral tasks can be sent.
    mut eph_tasks_rx: mpsc::Receiver<LxTask<()>>,
    mut shutdown: NotifyOnce,
    shutdown_timeout: Duration,
) -> Result<(), Error> {
    // The behavior is the same without this block, but just to be clear:
    // We want to return only after the shutdown signal is complete so outer
    // layers don't assume that we finished prematurely (cringe!)
    if static_tasks.is_empty() {
        shutdown.recv().await;
        return Ok(());
    }

    let mut static_tasks = static_tasks
        .into_iter()
        .map(LxTask::logged)
        .collect::<FuturesUnordered<_>>();
    let mut ephemeral_tasks = FuturesUnordered::new();

    let mut result = Ok(());

    // Wait for a shutdown signal and poll all tasks
    loop {
        tokio::select! {
            // Mitigate possible select! race after a shutdown signal is sent
            biased;
            () = shutdown.recv() => break,
            Some(task) = eph_tasks_rx.recv() => {
                debug!("Received ephemeral task: {name}", name = task.name());
                ephemeral_tasks.push(task.logged());
            }
            Some(name) = ephemeral_tasks.next() => {
                debug!("Ephemeral task finished: {name}");
            }
            Some(name) = static_tasks.next() => {
                // A static task finished prematurely. Set our result to an
                // error, initiate a shutdown, and wait on the remaining tasks.
                result = Err(Error::PrematureFinish { name });
                break shutdown.send();
            }
        }
    }

    let mut all_tasks = static_tasks
        .into_iter()
        .chain(ephemeral_tasks.into_iter())
        .collect::<FuturesUnordered<_>>();

    let shutdown_timeout_fut = tokio::time::sleep(shutdown_timeout);
    tokio::pin!(shutdown_timeout_fut);

    while !all_tasks.is_empty() {
        tokio::select! {
            Some(_name) = all_tasks.next() => (),
            () = &mut shutdown_timeout_fut => {
                // TODO(phlip9): How to get a backtrace of a hung task?
                let hung_tasks = all_tasks
                    .iter()
                    .map(|task| task.name().to_owned())
                    .collect::<Vec<_>>();

                return Err(Error::Hung { hung_tasks });
            }
        }
    }

    result
}

/// Shorthand to call [`try_join_tasks_and_shutdown`] and log any errors,
/// useful when the callsite needs a `Future<Output = ()> + Send + 'static`.
/// (Otherwise the callsite needs a bunch of `async move { ... }` junk)
pub async fn join_tasks_and_shutdown(
    name: &str,
    static_tasks: Vec<LxTask<()>>,
    eph_tasks_rx: mpsc::Receiver<LxTask<()>>,
    shutdown: NotifyOnce,
    max_shutdown_delta: Duration,
) {
    let result = try_join_tasks_and_shutdown(
        static_tasks,
        eph_tasks_rx,
        shutdown,
        max_shutdown_delta,
    )
    .await;

    match result {
        Ok(()) => info!("{name} tasks finished."),
        Err(e) => error!("{name} tasks errored: {e:#}"),
    }
}

/// Adds `#[must_use]` to ensure [`Option<LxTask<()>>`]s are used (or detached).
#[must_use]
pub struct MaybeLxTask<T>(pub Option<LxTask<T>>);

impl<T> MaybeLxTask<T> {
    pub fn detach(self) {
        if let Some(task) = self.0 {
            task.detach();
        }
    }
}

/// A thin wrapper around [`tokio::task::JoinHandle`] that:
///
/// (1) propagates panics instead of catching them
/// (2) adds the `#[must_use]` lint to ensure that all spawned tasks are joined
///     or explictly annotated that no joining is required. Use
///     [`LxTask::detach`] to make it clear that the spawned task should be
///     detached from the handle. Once detached, a task can't be joined.
///
/// The main goal with `LxTask` is to encourage [Structured Concurrency] by
/// joining all spawned tasks. This design pattern often leads to:
///
/// 1. saner control flow
/// 2. reduces resource leakage from orphaned or zombie spawned tasks
/// 3. helps propagate errors from panics in spawned tasks
///
/// Consequently, [`LxTask::detach`] should be used sparingly.
///
/// [`LxTask`] also includes an optional task name for improved debuggability.
/// - Use [`LxTask::name`] to get the name of a running task.
/// - Use [`LxTask::logged`] to instrument the task so it logs its name and
///   status when it finishes.
///
/// [Structured Concurrency]: https://www.wikiwand.com/en/Structured_concurrency
#[must_use]
pub struct LxTask<T> {
    task: JoinHandle<T>,
    name: Cow<'static, str>,
}

/// A [`Future`] that wraps [`LxTask`] so its result is logged when it finishes.
/// The inner `T` is discarded and the [`Future::Output`] is mapped to its name.
pub struct LoggedLxTask<T>(LxTask<T>);

// Provides a [`Display`] impl for the result of a finished task.
struct TaskOutputDisplay<'a> {
    name: &'a str,
    // Convert a task output to this using `result.as_ref().map(|_| ())`.
    // Avoids some code bloat by removing the generic `T` in `LxTask<T>`.
    result: Result<(), &'a tokio::task::JoinError>,
}

// --- impl LxTask --- //

impl<T> LxTask<T> {
    /// Constructs a [`LxTask`] from an existing [`tokio::task::JoinHandle`].
    pub fn from_tokio(
        handle: JoinHandle<T>,
        name: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            task: handle,
            name: name.into(),
        }
    }

    /// Spawns a named task which inherits from the current span.
    /// This is generally what you want to use.
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn() {
    /// use common::task::LxTask;
    /// use tracing::{info, instrument};
    ///
    /// // Typical library code.
    /// #[instrument(name = "(my-span)")]
    /// async fn my_library_function() {
    ///     info!("This log msg is prefixed with (my-span)");
    ///
    ///     let task = LxTask::spawn(
    ///         "my task name",
    ///         async move {
    ///             info!("This log msg is also prefixed with (my-span)");
    ///         }
    ///     );
    ///     task.await;
    /// }
    ///
    /// # my_library_function().await;
    /// # }
    /// ```
    #[inline]
    #[allow(clippy::disallowed_methods)]
    pub fn spawn<F>(
        name: impl Into<Cow<'static, str>>,
        future: F,
    ) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        // Instrument the future so that the current tracing span propagates
        // past spawn boundaries.
        let span = tracing::Span::current();
        Self::spawn_with_span(name, span, future)
    }

    /// Spawns a task without a name. Use this primarily for trivial tasks where
    /// you don't care about joining later (e.g. a task that makes an API call)
    #[inline]
    pub fn spawn_unnamed<F>(future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        let name = String::new();
        let span = tracing::Span::current();
        Self::spawn_with_span(name, span, future)
    }

    /// Spawns a named task with a custom span. This is the most versatile API.
    ///
    /// Note that the [`tracing::Span`]s generated by the `span!` macros inherit
    /// from the current span by default. If it is desired to prevent the span
    /// from inheriting from the current span, include `parent: None`.
    ///
    /// ```
    /// # use tracing::info_span;
    /// let span = info_span!(parent: None, "(my-span)");
    /// ```
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn_with_span() {
    /// use common::task::LxTask;
    /// use tracing::{info, instrument};
    ///
    /// // Typical library code.
    /// fn my_library_function() {
    ///     info!("This is prefixed by (my-span) when called inside the task");
    /// }
    ///
    /// // Typical orchestration code.
    /// #[instrument(name = "(orchestrator)")]
    /// async fn orchestrate() {
    ///     info!("This is prefixed by (orchestrator)");
    ///
    ///     let task = LxTask::spawn_with_span(
    ///         "my task name",
    ///         info_span!(parent: None, "(my-span)"),
    ///         async move {
    ///             // This logs a message with (my-span) but not (orchestrator)
    ///             my_library_function();
    ///         }
    ///     );
    ///     task.await;
    /// }
    ///
    /// # orchestrate().await;
    /// # }
    /// ```
    #[inline]
    #[allow(clippy::disallowed_methods)]
    pub fn spawn_with_span<F>(
        name: impl Into<Cow<'static, str>>,
        span: tracing::Span,
        future: F,
    ) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        let name = name.into();
        debug!("Spawning task: {name}");
        Self {
            task: tokio::spawn(future.instrument(span)),
            name,
        }
    }

    /// Drop the task handle, detaching it so it continues running the
    /// background. Without a handle, you can no longer `.await` the task itself
    /// to get the output.
    ///
    /// We consider it an anti-pattern to spawn tasks without some handle to get
    /// the results (or potential panics) from the completed task.
    #[inline]
    pub fn detach(self) {
        std::mem::drop(self)
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Calls [`is_finished`] on the underlying [`JoinHandle`].
    ///
    /// [`is_finished`]: tokio::task::JoinHandle::is_finished
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    /// Instrument a [`LxTask`] so that its result is logged when it finishes.
    /// The [`LxTask`]'s [`Future::Output`] is also mapped to the task name.
    #[inline]
    pub fn logged(self) -> LoggedLxTask<T> {
        LoggedLxTask(self)
    }

    #[inline]
    pub fn abort(&self) {
        self.task.abort();
    }
}

impl<T> Future for LxTask<T> {
    type Output = Result<T, JoinError>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        use std::io::Write;

        let result = match Pin::new(&mut self.task).poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };

        let result = match result {
            Ok(val) => Ok(val),
            Err(join_err) => {
                // HACK: Try to flush the error before propagating.
                // This is bc backtraces are getting swallowed by SGX.
                {
                    let name = self.name();
                    println!("FATAL TASK ERROR: {join_err:#} {name}");
                    eprintln!("FATAL TASK ERROR: {join_err:#} {name}");
                    tracing::error!(%name, "FATAL TASK ERROR: {join_err:#}");
                    if let Err(e) = std::io::stdout().flush() {
                        eprintln!("Toilet clogged! {e:#}");
                    }
                    if let Err(e) = std::io::stderr().flush() {
                        println!("Toilet clogged! {e:#}");
                    }
                }

                match join_err.try_into_panic() {
                    // If the inner spawned task panicked, then propagate the
                    // panic to the `LxTask` poller.
                    Ok(panic_reason) => {
                        error!("Task '{name}' panicked!", name = self.name());
                        std::panic::resume_unwind(panic_reason)
                    }
                    Err(join_err) => Err(join_err),
                }
            }
        };

        Poll::Ready(result)
    }
}

// --- impl LoggedLxTask --- //

impl<T> LoggedLxTask<T> {
    #[inline]
    pub fn name(&self) -> &str {
        self.0.name()
    }

    #[inline]
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
}

impl<T> Future for LoggedLxTask<T> {
    type Output = Cow<'static, str>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx).map(|result| {
            let mut log_error = false;
            let mut log_warn = false;

            match &result {
                Ok(_) => (),
                Err(e) if e.is_cancelled() => log_warn = true,
                Err(e) if e.is_panic() => log_error = true,
                _ => log_warn = true,
            };

            let msg = TaskOutputDisplay {
                name: self.name(),
                result: result.as_ref().map(|_| ()),
            };

            if log_error {
                error!("{msg}")
            } else if log_warn {
                warn!("{msg}")
            } else {
                info!("{msg}")
            }

            self.0.name.clone()
        })
    }
}

// --- impl TaskOutputDisplay --- //

impl Display for TaskOutputDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let join_label = match &self.result {
            Ok(_) => "finished",
            Err(e) if e.is_cancelled() => "cancelled",
            Err(e) if e.is_panic() => "panicked",
            _ => "(unknown join error)",
        };

        // "Task '<name>' <finished|cancelled|panicked>: [<error>]"
        let name = self.name;
        write!(f, "Task '{name}' {join_label}")?;

        if let Err(e) = self.result {
            write!(f, ": {e:#}")?;
        }

        Ok(())
    }
}
