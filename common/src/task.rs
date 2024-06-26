use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::task::{JoinError, JoinHandle};
use tracing::{error, info, warn, Instrument, Span};

/// A thin wrapper around [`tokio::task::JoinHandle`] that adds the
/// `#[must_use]` lint to ensure that all spawned tasks are joined or explictly
/// annotated that no joining is required. Use [`LxTask::detach`] to make it
/// clear that the spawned task should be detached from the handle. Once
/// detached, a task can't be joined.
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
/// `LxTask` also includes an optional task name for improved debuggability.
/// [`LxTask::with_name`] will return a Future of the task result
/// alongside the task name.
///
/// [Structured Concurrency]: https://www.wikiwand.com/en/Structured_concurrency
#[must_use]
pub struct LxTask<T> {
    task: JoinHandle<T>,
    name: String,
}

impl<T> LxTask<T> {
    /// Spawns a task without a name. Use this primarily for trivial tasks where
    /// you don't care about joining later (e.g. a task that makes an API call)
    #[inline]
    pub fn spawn<F>(future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self::spawn_named(String::new(), future)
    }

    /// Spawns a named task which inherits the current span.
    /// This is generally what you want to use.
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn_named() {
    /// use common::task::LxTask;
    /// use tracing::{info, instrument};
    ///
    /// // Typical library code.
    /// #[instrument(name = "(my-span)")]
    /// async fn my_library_function() {
    ///     info!("This log msg is prefixed with (my-span)");
    ///
    ///     let task = LxTask::spawn_named(
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
    pub fn spawn_named<F>(
        name: impl Into<String>,
        future: F,
    ) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        // Instrument the future so that the current tracing span propagates
        // past spawn boundaries.
        Self {
            task: tokio::spawn(future.in_current_span()),
            name: name.into(),
        }
    }

    /// Spawns an unnnamed task which does NOT inherit the current span.
    ///
    /// Useful for quickly preventing multiple span labels `(span1) (span2)`
    /// from showing in logs.
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn_no_inherit() {
    /// use common::task::LxTask;
    /// use tracing::{info, instrument};
    ///
    /// // Typical library code.
    /// #[instrument(name = "(my-span)")]
    /// fn my_library_function() {
    ///     info!("This is prefixed by (my-span) but may have others too");
    /// }
    ///
    /// // Typical orchestration code.
    /// #[instrument(name = "(orchestrator)")]
    /// async fn orchestrate() {
    ///     info!("This is prefixed by (orchestrator)");
    ///
    ///     let task = LxTask::spawn_no_inherit(
    ///         async move {
    ///             info!("This log msg does NOT have a span prefix");
    ///             // This prints a log msg with (my-span) only
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
    pub fn spawn_no_inherit<F>(future: F) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self::spawn_named_no_inherit(String::new(), future)
    }

    /// Spawns a named task which does NOT inherit the current span.
    ///
    /// Prevents multiple span labels `(span1) (span2)` from showing in logs.
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn_named_no_inherit() {
    /// use common::task::LxTask;
    /// use tracing::{info, instrument};
    ///
    /// // Typical library code.
    /// #[instrument(name = "(my-span)")]
    /// fn my_library_function() {
    ///     info!("This is prefixed by (my-span) but may have others too");
    /// }
    ///
    /// // Typical orchestration code.
    /// #[instrument(name = "(orchestrator)")]
    /// async fn orchestrate() {
    ///     info!("This is prefixed by (orchestrator)");
    ///
    ///     let task = LxTask::spawn_named_no_inherit(
    ///         "my task name",
    ///         async move {
    ///             info!("This log msg does NOT have a span prefix");
    ///             // This prints a log msg with (my-span) only
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
    pub fn spawn_named_no_inherit<F>(
        name: impl Into<String>,
        future: F,
    ) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        Self {
            task: tokio::spawn(future),
            name: name.into(),
        }
    }

    /// Spawns a named task with a custom span.
    ///
    /// Note that the [`Span`]s generated by the `span!` macros inherit from the
    /// current span by default; include `parent: None` in the macro invocation
    /// to disable this.
    ///
    /// ```
    /// # use tracing::info_span;
    /// let span = info_span!(parent: None, "(my-span)");
    /// ```
    ///
    /// It is generally preferred to add spans with
    /// [`macro@tracing::instrument`], using [`LxTask::spawn_no_inherit`] or
    /// [`LxTask::spawn_named_no_inherit`] in orchestration code when necessary,
    /// since the span labels will be outputted in logs regardless whether the
    /// crate is run as a process or a task. However, sometimes this is not
    /// possible, e.g. when defining API routes which are packaged into a
    /// service [`Future`] elsewhere. [`LxTask::spawn_named_with_span`] is
    /// useful for this case.
    ///
    /// ```
    /// # #[tokio::test]
    /// # async fn test_spawn_named_with_span() {
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
    ///     let task = LxTask::spawn_named_with_span(
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
    pub fn spawn_named_with_span<F>(
        name: impl Into<String>,
        span: Span,
        future: F,
    ) -> LxTask<F::Output>
    where
        F: Future<Output = T> + Send + 'static,
        F::Output: Send + 'static,
    {
        // Instrument the future with the given tracing span.
        Self {
            task: tokio::spawn(future.instrument(span)),
            name: name.into(),
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

    /// Make await'ing on an `LxTask` return the name along with the result:
    /// `(Result<T, JoinError>, name)`
    #[inline]
    pub fn with_name(self) -> LxTaskWithName<T> {
        LxTaskWithName(self)
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

/// Helper to log the output of a finished [`LxTaskWithName<()>`]
///
/// Pass `ed = true` if the task finished prematurely.
pub fn log_finished_task(output: &(Result<(), JoinError>, String), ed: bool) {
    let (join_res, name) = output;
    let join_label = join_result_label(join_res);

    // "Task '<name>' <finished|cancelled|panicked> [prematurely]: [<error>]"
    let mut msg = format!("Task '{name}' {join_label}");
    if ed {
        msg.push_str(" prematurely");
    }
    if let Err(e) = join_res {
        msg.push_str(": ");
        msg.push_str(&format!("{e:#}"));
    }

    if ed || join_res.is_err() {
        warn!("{msg}");
    } else {
        info!("{msg}");
    }
}

/// A small helper that gives a human-readable label for a joined task's
/// resulting output.
pub fn join_result_label(join_res: &Result<(), JoinError>) -> &'static str {
    match join_res {
        Ok(()) => "finished",
        Err(err) if err.is_cancelled() => "cancelled",
        Err(err) if err.is_panic() => "panicked",
        _ => "(unknown join error)",
    }
}

/// A small wrapper `Future` for `LxTask` that returns the task name alongside
/// the task output.
pub struct LxTaskWithName<T>(LxTask<T>);

impl<T> LxTaskWithName<T> {
    #[inline]
    pub fn name(&self) -> &str {
        self.0.name()
    }

    /// Calls [`is_finished`] on the underlying [`LxTask`].
    ///
    /// [`is_finished`]: LxTask::is_finished
    #[inline]
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }
}

impl<T> Future for LxTaskWithName<T> {
    type Output = (Result<T, JoinError>, String);

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        let result = match Pin::new(&mut self.0).poll(cx) {
            Poll::Ready(result) => result,
            Poll::Pending => return Poll::Pending,
        };

        let name = self.name().to_string();

        let result = match result {
            Ok(val) => (Ok(val), name),
            Err(err) => (Err(err), name),
        };

        Poll::Ready(result)
    }
}
