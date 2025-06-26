use std::time::Duration;

use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::{
    sync::mpsc,
    time::{self, Instant},
};
use tracing::{debug, info, info_span};

/// A simple actor that keeps track of an inactivity timer held in the stack of
/// its `start()` fn.
///
/// - If the command server sends an activity event, the timer resets to now +
///   `self.duration`.
/// - If an activity event is received via `activity_rx`, the timer is reset.
/// - If the timer reaches 0, a shutdown signal is sent via `shutdown`.
/// - If a shutdown signal is received, the actor shuts down.
pub struct InactivityTimer {
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
        inactivity_timer_sec: u64,
        activity_rx: mpsc::Receiver<()>,
        shutdown: NotifyOnce,
    ) -> Self {
        let duration = Duration::from_secs(inactivity_timer_sec);
        Self {
            duration,
            activity_rx,
            shutdown,
        }
    }

    pub fn spawn_into_task(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(inactivity-timer)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            self.run().await
        })
    }

    /// Starts the inactivity timer.
    pub async fn run(mut self) {
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
                            debug!("Received activity event");
                            timer.as_mut().reset(Instant::now() + self.duration);
                        }
                        None => {
                            info!("All activity_tx dropped, shutting down");
                            break
                        },
                    }
                }
                () = self.shutdown.recv() => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use lexe_tokio::{task::LxTask, DEFAULT_CHANNEL_SIZE};

    use super::*;

    /// A simple struct that holds all the context required to test the
    /// InactivityTimer.
    struct TestContext {
        actor: InactivityTimer,
        activity_tx: mpsc::Sender<()>,
        shutdown: NotifyOnce,
    }

    fn get_test_context(inactivity_timer_sec: u64) -> TestContext {
        let (activity_tx, activity_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let shutdown = NotifyOnce::new();
        let actor_shutdown = shutdown.clone();
        let actor = InactivityTimer::new(
            inactivity_timer_sec,
            activity_rx,
            actor_shutdown,
        );

        TestContext {
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

    /// Case 1: *with* activity only at start
    #[test]
    fn case_1() {
        do_test_paused_time(async {
            let inactivity_timer_sec = 1;
            let ctxt = get_test_context(inactivity_timer_sec);
            let _ = ctxt.activity_tx.send(()).await;
            let actor_fut = ctxt.actor.run();

            // Actor should finish at 1000ms (1 sec)
            bound_finish(actor_fut, ctxt.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 2: no activity at all
    #[test]
    fn case_2() {
        do_test_paused_time(async {
            let inactivity_timer_sec = 1;
            let ctxt = get_test_context(inactivity_timer_sec);
            let actor_fut = ctxt.actor.run();

            // Actor should finish at about 1000ms (1 sec)
            bound_finish(actor_fut, ctxt.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 3: *with* activity 500ms in
    #[test]
    fn case_3() {
        do_test_paused_time(async {
            let inactivity_timer_sec = 1;
            let ctxt = get_test_context(inactivity_timer_sec);
            let actor_fut = ctxt.actor.run();

            // Spawn a task to generate an activity event 500ms in
            let activity_tx = ctxt.activity_tx.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                let _ = activity_tx.send(()).await;
            });

            // Actor should finish at about 1500ms
            bound_finish(actor_fut, ctxt.shutdown, Some(1499), Some(1501))
                .await;
            activity_task.await.unwrap();
        });
    }

    /// Case 4: *with* activity, *with* shutdown signal.
    /// The shutdown signal should take precedence over the activity timer
    #[test]
    fn case_4() {
        do_test_paused_time(async {
            let inactivity_timer_sec = 1;
            let ctxt = get_test_context(inactivity_timer_sec);
            let actor_fut = ctxt.actor.run();

            // Spawn a task to generate an activity event 500ms in and a
            // shutdown signal 750ms in
            let activity_tx = ctxt.activity_tx.clone();
            let shutdown = ctxt.shutdown.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                let _ = activity_tx.send(()).await;
                time::sleep(Duration::from_millis(250)).await;
                shutdown.send();
            });

            // Actor should finish at about 750ms despite receiving activity
            bound_finish(actor_fut, ctxt.shutdown, Some(749), Some(751)).await;
            activity_task.await.unwrap();
        });
    }
}
