use std::sync::Arc;

use tokio::sync::Semaphore;

/// A synchronization utility designed for sending / receiving shutdown signals.
///
/// Features:
///
/// - Multi-producer and multi-consumer - simply clone to get another handle
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
#[derive(Clone, Debug)]
pub struct ShutdownChannel {
    inner: Arc<Semaphore>,
}

impl ShutdownChannel {
    /// Construct a new [`ShutdownChannel`].
    /// This function should only be called *once* in the lifetime of a program.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let inner = Arc::new(Semaphore::new(0));
        Self { inner }
    }

    /// Send a shutdown signal, causing all actors waiting on this channel to
    /// complete their call to [`recv`].
    ///
    /// [`recv`]: ShutdownChannel::recv
    pub fn send(&self) {
        self.inner.close()
    }

    /// Wait for a shutdown signal.
    /// If a shutdown signal was already sent, this fn returns immediately.
    pub async fn recv(&self) {
        self.inner
            .acquire()
            .await
            .map_err(|_| ())
            .expect_err("Shouldn't've been able to acquire a permit")
    }

    /// Immediately returns whether a shutdown signal has been sent.
    pub fn try_recv(&self) -> bool {
        self.inner.is_closed()
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use tokio::time;

    use super::*;

    #[test]
    fn multiple_sends_doesnt_panic() {
        let shutdown = ShutdownChannel::new();
        shutdown.send();
        shutdown.send();
        shutdown.send();
    }

    #[tokio::test(start_paused = true)]
    async fn subscribe_after_close_is_ok() {
        // Basic test: subscribe, wait, shutdown
        let shutdown1 = ShutdownChannel::new();
        let shutdown2 = shutdown1.clone();
        time::sleep(Duration::from_secs(1)).await;
        shutdown1.send();
        time::timeout(Duration::from_nanos(1), shutdown2.recv())
            .await
            .expect("Did not finish immediately");

        // 'Subscribing' after close should immediately finish
        let shutdown3 = shutdown2.clone();
        assert!(shutdown3.try_recv());
        time::timeout(Duration::from_nanos(1), shutdown3.recv())
            .await
            .expect("Did not finish immediately");
    }
}
