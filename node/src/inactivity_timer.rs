use std::time::Duration;

use lexe_tokio::notify_once::NotifyOnce;
use tokio::{
    sync::{mpsc, mpsc::error::TryRecvError},
    time::{self, Instant},
};
use tracing::{debug, info, trace};

// TODO(max): Rewrite as a `fn spawn_inactivity_timer() -> LxTask<()>` which is
// a lot more concise.

// TODO(max): Also count Lightning Network events as activity events
/// A simple actor that keeps track of an inactivity timer held in the stack of
/// its `start()` fn.
///
/// - If the command server sends an activity event, the timer resets to now +
///   `self.duration`.
/// - If an activity event is received via `activity_rx`, the timer is reset.
/// - If the timer reaches 0, a shutdown signal is sent via `shutdown`.
/// - If a shutdown signal is received, the actor shuts down.
pub struct InactivityTimer {
    /// Whether to signal a shutdown if no activity was detected at the
    /// beginning of `start()`
    shutdown_after_sync: bool,
    /// The duration that the inactivity timer will reset to whenever it
    /// receives an activity event.
    duration: Duration,
    /// Used to receive activity events from the command server.
    activity_rx: mpsc::Receiver<()>,
    /// Used to signal the rest of the program to shut down.
    shutdown: NotifyOnce,
}

impl InactivityTimer {
    pub fn new(
        shutdown_after_sync: bool,
        inactivity_timer_sec: u64,
        activity_rx: mpsc::Receiver<()>,
        shutdown: NotifyOnce,
    ) -> Self {
        let duration = Duration::from_secs(inactivity_timer_sec);
        Self {
            shutdown_after_sync,
            duration,
            activity_rx,
            shutdown,
        }
    }

    /// Starts the inactivity timer.
    pub async fn start(&mut self) {
        if self.shutdown_after_sync {
            match self.activity_rx.try_recv() {
                Ok(()) => {
                    trace!("Activity detected, starting shutdown timer");
                }
                Err(TryRecvError::Empty) => {
                    info!("No activity detected, initiating shutdown");
                    self.shutdown.send();
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    info!("Timer channel disconnected, initiating shutdown");
                    self.shutdown.send();
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
                    info!("Inactivity timer hit 0, sending shutdown signal");
                    self.shutdown.send();
                    break;
                }
                activity_opt = self.activity_rx.recv() => {
                    match activity_opt {
                        Some(()) => {
                            debug!(
                                "Received activity event, resetting"
                            );
                            timer.as_mut().reset(Instant::now() + self.duration);
                        }
                        None => {
                            info!("All activity_tx dropped, shutting down");
                            break
                        },
                    }
                }
                () = self.shutdown.recv() => {
                    info!("Inactivity timer received shutdown signal");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use lexe_tokio::{task::LxTask, DEFAULT_CHANNEL_SIZE};

    use super::*;

    /// A simple struct that holds all the materials required to test the
    /// InactivityTimer.
    struct TestMaterials {
        actor: InactivityTimer,
        activity_tx: mpsc::Sender<()>,
        shutdown: NotifyOnce,
    }

    fn get_test_materials(
        shutdown_after_sync: bool,
        inactivity_timer_sec: u64,
    ) -> TestMaterials {
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = NotifyOnce::new();
        let actor_shutdown = shutdown.clone();
        let actor = InactivityTimer::new(
            shutdown_after_sync,
            inactivity_timer_sec,
            activity_rx,
            actor_shutdown,
        );

        TestMaterials {
            actor,
            activity_tx,
            shutdown,
        }
    }

    /// Tests that a given `InactivityTimer::start()` Future finishes within
    /// given time bounds. Also tests that it sends a shutdown signal.
    async fn bound_finish(
        actor_fut: impl Future<Output = ()>,
        shutdown: NotifyOnce,
        lower_bound_ms: Option<u64>,
        upper_bound_ms: Option<u64>,
    ) {
        let lower_bound =
            lower_bound_ms.map(|l| time::sleep(Duration::from_millis(l)));
        let upper_bound =
            upper_bound_ms.map(|u| time::sleep(Duration::from_millis(u)));
        tokio::pin!(actor_fut);
        if let Some(lower) = lower_bound {
            tokio::select! {
                () = &mut actor_fut => panic!("Actor finished too quickly"),
                () = lower => {}
            }
        }
        if let Some(upper) = upper_bound {
            tokio::select! {
                () = &mut actor_fut => {}
                () = upper => panic!("Took too long to finish"),
            }
        }
        assert!(shutdown.try_recv());
    }

    // A tokio runtime w/ time paused.
    fn test_rt_paused_time() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            // Only `enable_time()` to avoid spawning a new thread.
            //
            // In SGX, `node` is configured w/ only 2 threads, so `enable_all()`
            // in the default `#[tokio::test]` spawns one too many threads
            // (test harness, test thread, tokio io driver -> async-usercalls)
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap()
    }

    fn do_test_paused_time(fut: impl Future<Output = ()>) {
        test_rt_paused_time().block_on(fut)
    }

    /// Case 1: shutdown_after_sync enabled, no activity
    #[test]
    fn case_1() {
        do_test_paused_time(async {
            let shutdown_after_sync = true;
            let inactivity_timer_sec = 1;
            let mut mats =
                get_test_materials(shutdown_after_sync, inactivity_timer_sec);
            let actor_fut = mats.actor.start();

            // Actor should finish instantly
            bound_finish(actor_fut, mats.shutdown, None, Some(1)).await;
        });
    }

    /// Case 2: shutdown_after_sync enabled, *with* activity
    #[test]
    fn case_2() {
        do_test_paused_time(async {
            let shutdown_after_sync = true;
            let inactivity_timer_sec = 1;
            let mut mats =
                get_test_materials(shutdown_after_sync, inactivity_timer_sec);
            let _ = mats.activity_tx.send(()).await;
            let actor_fut = mats.actor.start();

            // Actor should finish at 1000ms (1 sec)
            bound_finish(actor_fut, mats.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 3: shutdown_after_sync not enabled, no activity
    #[test]
    fn case_3() {
        do_test_paused_time(async {
            let shutdown_after_sync = false;
            let inactivity_timer_sec = 1;
            let mut mats =
                get_test_materials(shutdown_after_sync, inactivity_timer_sec);
            let actor_fut = mats.actor.start();

            // Actor should finish at about 1000ms (1 sec)
            bound_finish(actor_fut, mats.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 4: shutdown_after_sync not enabled, *with* activity; i.e. the
    /// inactivity timer resets
    #[test]
    fn case_4() {
        do_test_paused_time(async {
            let shutdown_after_sync = false;
            let inactivity_timer_sec = 1;
            let mut mats =
                get_test_materials(shutdown_after_sync, inactivity_timer_sec);
            let actor_fut = mats.actor.start();

            // Spawn a task to generate an activity event 500ms in
            let activity_tx = mats.activity_tx.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                let _ = activity_tx.send(()).await;
            });

            // Actor should finish at about 1500ms
            bound_finish(actor_fut, mats.shutdown, Some(1499), Some(1501))
                .await;
            activity_task.await.unwrap();
        });
    }

    /// Case 5: shutdown_after_sync not enabled, *with* activity, *with*
    /// shutdown signal. The shutdown signal should take precedence over the
    /// activity timer
    #[test]
    fn case_5() {
        do_test_paused_time(async {
            let shutdown_after_sync = false;
            let inactivity_timer_sec = 1;
            let mut mats =
                get_test_materials(shutdown_after_sync, inactivity_timer_sec);
            let actor_fut = mats.actor.start();

            // Spawn a task to generate an activity event 500ms in and a
            // shutdown signal 750ms in
            let activity_tx = mats.activity_tx.clone();
            let shutdown = mats.shutdown.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                let _ = activity_tx.send(()).await;
                time::sleep(Duration::from_millis(250)).await;
                shutdown.send();
            });

            // Actor should finish at about 750ms despite receiving activity
            bound_finish(actor_fut, mats.shutdown, Some(749), Some(751)).await;
            activity_task.await.unwrap();
        });
    }
}
