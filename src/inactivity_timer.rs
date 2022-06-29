use std::time::Duration;

use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{self, Instant};

// TODO(max): Also count Lightning Network events as activity events
/// A simple actor that keeps track of an inactivity timer held in the stack of
/// its `start()` fn.
///
/// - If the command server sends an activity event, the timer resets to now +
///   `self.duration`.
/// - If an activity event is received via `activity_rx`, the timer is reset.
/// - If the timer reaches 0, a shutdown signal is sent via `shutdown_tx`.
/// - If a shutdown signal is received, the actor shuts down.
pub struct InactivityTimer {
    /// Whether to signal a shutdown if no activity was detected at the
    /// beginning of `start()`
    shutdown_after_sync_if_no_activity: bool,
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
        shutdown_after_sync_if_no_activity: bool,
        inactivity_timer_sec: u64,
        activity_rx: mpsc::Receiver<()>,
        shutdown_tx: broadcast::Sender<()>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        let duration = Duration::from_secs(inactivity_timer_sec);
        Self {
            shutdown_after_sync_if_no_activity,
            duration,
            activity_rx,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Starts the inactivity timer.
    pub async fn start(&mut self) {
        if self.shutdown_after_sync_if_no_activity {
            match self.activity_rx.try_recv() {
                Ok(()) => {
                    println!("Activity detected, starting shutdown timer")
                }
                Err(TryRecvError::Empty) => {
                    println!("No activity detected, initiating shutdown");
                    let _ = self.shutdown_tx.send(());
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    println!("Timer channel disconnected, initiating shutdown");
                    let _ = self.shutdown_tx.send(());
                    return;
                }
            }
        }

        // Initiate timer
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
                                "Inactivity timer received activity, resetting"
                            );
                            timer.as_mut().reset(Instant::now() + self.duration);
                        }
                        None => {
                            println!("All activity_tx dropped, shutting down");
                            break
                        },
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

#[cfg(test)]
mod tests {

    use tokio::sync::{broadcast, mpsc};
    use tokio::time::{self, Duration};

    use super::*;
    use crate::init::DEFAULT_CHANNEL_SIZE;

    /// A simple struct that holds all the materials required to test the
    /// InactivityTimer.
    struct TestMaterials {
        actor: InactivityTimer,
        activity_tx: mpsc::Sender<()>,
        shutdown_tx: broadcast::Sender<()>,
        shutdown_rx: broadcast::Receiver<()>,
    }

    fn get_test_materials(
        shutdown_after_sync_if_no_activity: bool,
        inactivity_timer_sec: u64,
    ) -> TestMaterials {
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (shutdown_tx, shutdown_rx) =
            broadcast::channel(DEFAULT_CHANNEL_SIZE);
        let actor_shutdown_rx = shutdown_tx.subscribe();
        let actor = InactivityTimer::new(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
            activity_rx,
            shutdown_tx.clone(),
            actor_shutdown_rx,
        );

        TestMaterials {
            actor,
            activity_tx,
            shutdown_tx,
            shutdown_rx,
        }
    }

    /// Case 1: shutdown_after_sync enabled, no activity
    #[tokio::test]
    async fn case_1() {
        let shutdown_after_sync_if_no_activity = true;
        let inactivity_timer_sec = 1;
        let mut mats = get_test_materials(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
        );
        let actor_fut = mats.actor.start();

        // Actor should finish instantly
        let upper_bound = time::sleep(Duration::from_millis(10));
        tokio::select! {
            () = actor_fut => {}
            () = upper_bound => panic!("Should've finished instantly"),
        }
        mats.shutdown_rx
            .try_recv()
            .expect("Should have received shutdown signal");
    }

    /// Case 2: shutdown_after_sync enabled, *with* activity
    #[tokio::test]
    async fn case_2() {
        let shutdown_after_sync_if_no_activity = true;
        let inactivity_timer_sec = 1;
        let mut mats = get_test_materials(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
        );
        let _ = mats.activity_tx.send(()).await;
        let actor_fut = mats.actor.start();

        // Actor should finish at about 1000ms (1 sec)
        let lower_bound = time::sleep(Duration::from_millis(900));
        let upper_bound = time::sleep(Duration::from_millis(1100));
        tokio::pin!(actor_fut);
        tokio::select! {
            () = &mut actor_fut => panic!("Actor finished too quickly"),
            () = lower_bound => {}
        }
        tokio::select! {
            () = &mut actor_fut => {}
            () = upper_bound => panic!("Took too long to finish"),
        }
        mats.shutdown_rx
            .try_recv()
            .expect("Should have received shutdown signal");
    }

    /// Case 3: shutdown_after_sync not enabled, no activity
    #[tokio::test]
    async fn case_3() {
        let shutdown_after_sync_if_no_activity = false;
        let inactivity_timer_sec = 1;
        let mut mats = get_test_materials(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
        );
        let actor_fut = mats.actor.start();

        // Actor should finish at about 1000ms (1 sec)
        let lower_bound = time::sleep(Duration::from_millis(900));
        let upper_bound = time::sleep(Duration::from_millis(1100));
        tokio::pin!(actor_fut);
        tokio::select! {
            () = &mut actor_fut => panic!("Actor finished too quickly"),
            () = lower_bound => {}
        }
        tokio::select! {
            () = &mut actor_fut => {}
            () = upper_bound => panic!("Took too long to finish"),
        }
        mats.shutdown_rx
            .try_recv()
            .expect("Should have received shutdown signal");
    }

    /// Case 4: shutdown_after_sync not enabled, *with* activity; i.e. the
    /// inactivity timer resets
    #[tokio::test]
    async fn case_4() {
        let shutdown_after_sync_if_no_activity = false;
        let inactivity_timer_sec = 1;
        let mut mats = get_test_materials(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
        );
        let actor_fut = mats.actor.start();

        // Spawn a task to generate an activity event 500ms in
        let activity_tx = mats.activity_tx.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_millis(500)).await;
            let _ = activity_tx.send(()).await;
        });

        // Actor should finish at about 1500ms
        let lower_bound = time::sleep(Duration::from_millis(1400));
        let upper_bound = time::sleep(Duration::from_millis(1600));
        tokio::pin!(actor_fut);
        tokio::select! {
            () = &mut actor_fut => panic!("Actor finished too quickly"),
            () = lower_bound => {}
        }
        tokio::select! {
            () = &mut actor_fut => {}
            () = upper_bound => panic!("Took too long to finish"),
        }
        mats.shutdown_rx
            .try_recv()
            .expect("Should have received shutdown signal");
    }

    /// Case 5: shutdown_after_sync not enabled, *with* activity, *with*
    /// shutdown signal. The shutdown signal should take precedence over the
    /// activity timer
    #[tokio::test]
    async fn case_5() {
        let shutdown_after_sync_if_no_activity = false;
        let inactivity_timer_sec = 1;
        let mut mats = get_test_materials(
            shutdown_after_sync_if_no_activity,
            inactivity_timer_sec,
        );
        let actor_fut = mats.actor.start();

        // Spawn a task to generate an activity event 500ms in and a shutdown
        // signal 750ms in
        let activity_tx = mats.activity_tx.clone();
        tokio::spawn(async move {
            time::sleep(Duration::from_millis(500)).await;
            let _ = activity_tx.send(()).await;
            time::sleep(Duration::from_millis(250)).await;
            let _ = mats.shutdown_tx.send(());
        });

        // Actor should finish at about 750ms despite receiving activity
        let lower_bound = time::sleep(Duration::from_millis(700));
        let upper_bound = time::sleep(Duration::from_millis(800));
        tokio::pin!(actor_fut);
        tokio::select! {
            () = &mut actor_fut => panic!("Actor finished too quickly"),
            () = lower_bound => {}
        }
        tokio::select! {
            () = &mut actor_fut => {}
            () = upper_bound => panic!("Took too long to finish"),
        }
        mats.shutdown_rx
            .try_recv()
            .expect("Should have received shutdown signal");
    }
}
