//! # `notify` channel
//!
//! This small module implements a simple notification channel which wraps
//! [`tokio::sync::mpsc`] to provide the additional property that if multiple
//! notifications are sent before the receiver calls [`Receiver::recv`], the
//! receiver will only be notified once, preventing the receiver from
//! accidentally doing duplicate work.
//!
//! Everything else is sugar:
//!
//! - Just do `tx.send()` instead of `let _ = tx.try_send(())` to send a
//!   notification without caring about if the channel was full or if the
//!   receiver was dropped.
//! - Just do `rx.recv()` instead of `if let Some(()) = rx.recv() {}` to check
//!   that the future didn't complete simple because the sender was dropped. If
//!   all senders have been dropped, this future will never resolve.
//! - Just do `rx.clear()` instead of `while self.rx.try_recv().is_ok() {}` to
//!   clear out pending notifications on the channel.
//!
//! This can also be used as a [`oneshot::channel::<()>()`]
//!
//! [`Receiver::recv`]: crate::notify::Receiver::recv
//! [`oneshot::channel::<()>()`]: tokio::sync::oneshot::channel

use tokio::sync::mpsc;

/// Create a new `notify` channel returning a [`Sender`] (cloneable) and
/// [`Receiver`] (not cloneable), analogous to `mpsc::channel(1)`.
pub fn channel() -> (Sender, Receiver) {
    let (tx, rx) = mpsc::channel(1);
    (Sender(tx), Receiver(rx))
}

/// `notify` sender, analogous to `mpsc::Sender<()>`.
#[derive(Clone)]
pub struct Sender(mpsc::Sender<()>);

/// `notify` receiver, analogous to `mpsc::Receiver<()>`.
pub struct Receiver(mpsc::Receiver<()>);

impl Sender {
    /// Sends a notification to the [`Receiver`].
    pub fn send(&self) {
        let _ = self.0.try_send(());
    }
}

impl Receiver {
    /// Waits until a notification is received over the channel. Completes
    /// immediately if a notification has already been sent. NOTE: If all
    /// [`Sender`]s have been dropped, this future never completes!
    pub async fn recv(&mut self) {
        match self.0.recv().await {
            Some(()) => (),
            None => std::future::pending().await,
        }
    }

    /// Immediately returns whether a notification has been sent.
    #[must_use]
    pub fn try_recv(&mut self) -> bool {
        self.0.try_recv().is_ok()
    }

    /// Clears out any pending notifications in the channel.
    pub fn clear(&mut self) {
        while self.0.try_recv().is_ok() {}
    }
}
