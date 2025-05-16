use std::sync::Arc;

use tokio::sync::Semaphore;

/// Synchronization utility which sends a notification to all consumers *once*,
/// most commonly used for shutdown signals.
///
/// Features:
///
/// - Multi-producer and multi-consumer - simply clone to get another handle.
/// - Every clone observes a signal at-most-once. If the signal has already been
///   sent, new clones can still observe it once.
/// - Consumers can receive signals that were sent prior to 'subscribing' to the
///   channel (unlike [`tokio::sync::broadcast`]);
/// - It is safe to send a signal multiple times (e.g. by accident).
///
/// The underlying implementation (ab)uses the fact that calling [`acquire`] on
/// a [`Semaphore`] with 0 permits only returns once the [`Semaphore`] has been
/// closed. Closing the [`Semaphore`] is equivalent to sending a signal, and
/// receiving an [`AcquireError`] (indicating the [`Semaphore`] has been closed)
/// from a call to [`acquire`] is equivalent to receiving one. [`NotifyOnce`]'s
/// methods abstract over these details, of course.
///
/// [`acquire`]: Semaphore::acquire
/// [`AcquireError`]: tokio::sync::AcquireError
#[derive(Debug)]
pub struct NotifyOnce {
    inner: Arc<Semaphore>,
    have_recved: bool,
}

impl NotifyOnce {
    /// Construct a new [`NotifyOnce`].
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let inner = Arc::new(Semaphore::new(0));
        Self {
            inner,
            have_recved: false,
        }
    }

    /// Send a signal, causing all actors waiting on this channel to complete
    /// their call to [`recv`].
    ///
    /// [`recv`]: NotifyOnce::recv
    pub fn send(&self) {
        self.inner.close();
    }

    /// Wait for a signal.
    ///
    /// NOTE: If this `NotifyOnce` has already observed a signal,
    /// _this future will never return!_
    pub async fn recv(&mut self) {
        if self.have_recved {
            // TODO(phlip9): seems not great, but it works with what we have
            // THIS FUTURE WILL NEVER RESOLVE
            std::future::pending().await
        } else {
            // wait for a signal
            self.inner
                .acquire()
                .await
                .map_err(|_| ())
                .expect_err("Shouldn't've been able to acquire a permit");
            // we've seen a signal;
            // if this method gets called again, it won't yield.
            self.have_recved = true;
        }
    }

    /// Waits for a signal, taking ownership of the handle. Useful for graceful
    /// shutdown APIs which require `impl Future<Output = ()> + 'static`.
    pub async fn recv_owned(mut self) {
        self.recv().await
    }

    /// Immediately returns whether a signal has been sent.
    /// This bypasses the at-most-once logic; calling this function will NOT
    /// consume the signal for a later call to [`recv`](Self::recv).
    #[must_use]
    pub fn try_recv(&self) -> bool {
        self.inner.is_closed()
    }
}

impl Clone for NotifyOnce {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            // Every clone gets a chance to see the signal, even if the original
            // has already seen it.
            have_recved: false,
        }
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use tokio::time;
    use tokio_test::{assert_pending, assert_ready};

    use super::*;

    #[test]
    fn multiple_sends_doesnt_panic() {
        let shutdown = NotifyOnce::new();
        shutdown.send();
        shutdown.send();
        shutdown.send();
    }

    #[test]
    fn only_yields_shutdown_once() {
        let shutdown1 = NotifyOnce::new();
        let mut shutdown2 = shutdown1.clone();

        // a normal task that recv's from a shutdown handle should see the event
        let mut recv_task2_1 = tokio_test::task::spawn(shutdown2.recv());
        assert_pending!(recv_task2_1.poll());

        shutdown1.send();

        assert!(recv_task2_1.is_woken());
        assert_ready!(recv_task2_1.poll());
        drop(recv_task2_1);

        // trying to recv from the same handle more than once will always return
        // pending
        let mut recv_task2_2 = tokio_test::task::spawn(shutdown2.recv());
        assert_pending!(recv_task2_2.poll());
        assert_pending!(recv_task2_2.poll());

        shutdown1.send();

        // still pending!
        assert_pending!(recv_task2_2.poll());
        assert_pending!(recv_task2_2.poll());
        drop(recv_task2_2);

        // but a new handle will get a new chance to see the shutdown event
        let mut shutdown3 = shutdown2.clone();
        let mut recv_task3 = tokio_test::task::spawn(shutdown3.recv());
        assert_ready!(recv_task3.poll());
    }

    #[tokio::test(start_paused = true)]
    async fn subscribe_after_close_is_ok() {
        // Basic test: subscribe, wait, shutdown
        let shutdown1 = NotifyOnce::new();
        let mut shutdown2 = shutdown1.clone();
        time::sleep(Duration::from_secs(1)).await;
        shutdown1.send();
        time::timeout(Duration::from_nanos(1), shutdown2.recv())
            .await
            .expect("Did not finish immediately");

        // 'Subscribing' after close should immediately finish
        let mut shutdown3 = shutdown2.clone();
        assert!(shutdown3.try_recv());
        time::timeout(Duration::from_nanos(1), shutdown3.recv())
            .await
            .expect("Did not finish immediately");
    }
}
