use std::time::Duration;

use tokio::sync::{broadcast, mpsc};
use tokio::time::{self, Instant};

/// A simple actor that keeps track of an inactivity timer held in the stack of
/// its `start()` fn.
///
/// - If the command server sends an activity event, the timer resets to now +
///   `self.duration`.
/// - If an activity event is received via `activity_rx`, the timer is reset.
/// - If the timer reaches 0, a shutdown signal is sent via `shutdown_tx`.
/// - If a shutdown signal is received, the actor shuts down.
pub struct InactivityTimer {
    /// The duration that the inactivity timer will reset to whenever it
    /// receives an activity event.
    duration: Duration,
    /// Used to receive activity events from the command server.
    activity_rx: mpsc::Receiver<()>,
    /// Used to signal the rest of the program to shut down.
    shutdown_tx: broadcast::Sender<()>,
    /// Used to receive a shutdown signal from the /lexe/shutdown endpoint.
    shutdown_rx: broadcast::Receiver<()>,
}

impl InactivityTimer {
    pub fn new(
        inactivity_timer_sec: u64,
        activity_rx: mpsc::Receiver<()>,
        shutdown_tx: broadcast::Sender<()>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let duration = Duration::from_secs(inactivity_timer_sec);
        Self {
            duration,
            activity_rx,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Starts the inactivity timer.
    pub async fn start(&mut self) {
        let timer = time::sleep(self.duration);

        // Pin the timer on the stack so it can be polled without being consumed
        tokio::pin!(timer);

        loop {
            tokio::select! {
                () = &mut timer => {
                    println!("Inactivity timer hit 0, sending shutdown signal");
                    let _ = self.shutdown_tx.send(());
                    break;
                }
                activity_opt = self.activity_rx.recv() => {
                    match activity_opt {
                        Some(()) => {
                            println!(
                                "Inactivity timer received activity,
                                resetting timer"
                            );
                            timer.as_mut().reset(Instant::now() + self.duration);
                        }
                        // All senders dropped, just shut down
                        None => break,
                    }
                }
                _ = self.shutdown_rx.recv() => {
                    println!("Inactivity timer received shutdown signal");
                    break;
                }
            }
        }
        println!("Inactivity timer complete.");
    }
}
