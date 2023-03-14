use std::sync::Arc;

use tokio::sync::Semaphore;

/// A synchronization utility designed for sending / receiving shutdown signals.
///
/// Features:
///
/// - Multi-producer and multi-consumer - simply clone to get another handle.
/// - Every clone observes shutdown signals at-most-once. If the shutdown has
///   already been sent, new clones can still observe it once.
/// - Consumers can receive shutdown signals that were sent prior to
///   'subscribing' to the channel (unlike [`tokio::sync::broadcast`]);
/// - It is safe to send a shutdown signal multiple times (e.g. by accident).
///
/// The underlying implementation (ab)uses the fact that calling [`acquire`] on
/// a [`Semaphore`] with 0 permits only returns once the [`Semaphore`] has been
/// closed. Closing the [`Semaphore`] is equivalent to sending a shutdown
/// signal, and receiving an [`AcquireError`] (indicating the [`Semaphore`] has
/// been closed) from a call to [`acquire`] is equivalent to receiving one.
/// [`ShutdownChannel`]'s methods abstract over these details, of course.
///
/// [`acquire`]: Semaphore::acquire
/// [`AcquireError`]: tokio::sync::AcquireError
#[derive(Debug)]
pub struct ShutdownChannel {
    inner: Arc<Semaphore>,
    have_recved: bool,
}

impl ShutdownChannel {
    /// Construct a new [`ShutdownChannel`].
    /// This function should only be called *once* in the lifetime of a program.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let inner = Arc::new(Semaphore::new(0));
        Self {
            inner,
            have_recved: false,
        }
    }

    /// Send a shutdown signal, causing all actors waiting on this channel to
    /// complete their call to [`recv`].
    ///
    /// [`recv`]: ShutdownChannel::recv
    pub fn send(&self) {
        self.inner.close();
    }

    /// Wait for a shutdown signal.
    ///
    /// If this `ShutdownChannel` has already observed a shutdown, _this future
    /// will never return!_
    pub async fn recv(&mut self) {
        if self.have_recved {
            // TODO(phlip9): seems not great, but it works with what we have
            // THIS FUTURE WILL NEVER RESOLVE
            std::future::pending().await
        } else {
            // wait for a shutdown
            self.inner
                .acquire()
                .await
                .map_err(|_| ())
                .expect_err("Shouldn't've been able to acquire a permit");
            // we've seen a shutdown; if this method gets called again, it
            // won't yield.
            self.have_recved = true;
        }
    }

    pub async fn recv_owned(mut self) {
        self.recv().await
    }

    /// Immediately returns whether a shutdown signal has been sent.
    pub fn try_recv(&self) -> bool {
        self.inner.is_closed()
    }
}

impl Clone for ShutdownChannel {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            // Every clone gets a chance to see the shutdown, even if the clonee
            // handle has already seen it.
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
        let shutdown = ShutdownChannel::new();
        shutdown.send();
        shutdown.send();
        shutdown.send();
    }

    #[test]
    fn only_yields_shutdown_once() {
        let shutdown1 = ShutdownChannel::new();
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
        let shutdown1 = ShutdownChannel::new();
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
