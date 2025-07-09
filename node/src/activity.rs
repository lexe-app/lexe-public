use std::{sync::Arc, time::Duration};

use common::api::user::UserPk;
use lexe_api::def::NodeRunnerApi;
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use tokio::{
    sync::mpsc,
    time::{self, Instant},
};
use tracing::{debug, info, info_span, warn};

use crate::client::RunnerClient;

/// Notifies various listeners of user or node activity.
///
/// 1) Resets the usernode's inactivity timer.
/// 2) Resets the meganode's inactivity timer.
/// 3) Notifies the UserRunner of user activity.
/// 4) Notifies the MegaRunner of user activity.
pub(crate) fn notify_listeners(
    user_pk: UserPk,
    mega_activity_bus: &EventsBus<UserPk>,
    user_activity_bus: &EventsBus<()>,
    runner_api: Arc<RunnerClient>,
    eph_tasks_tx: &mpsc::Sender<LxTask<()>>,
) {
    debug!("Notifying listeners of activity");

    // 1) Reset the usernode inactivity timer.
    user_activity_bus.send(());

    // Both the UserRunner and meganode inactivity timer listen on this:
    // 2) Reset the meganode's inactivity timer.
    // 3) Notify the UserRunner of user activity.
    mega_activity_bus.send(user_pk);

    // 4) Spawn a task to notify the MegaRunner of user activity.
    const SPAN_NAME: &str = "(megarunner-activity-notif)";
    let task = LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), {
        let runner_api = runner_api.clone();
        async move {
            if let Err(e) = runner_api.activity(user_pk).await {
                warn!("Couldn't notify runner (active): {e:#}");
            }
        }
    });
    let _ = eph_tasks_tx.try_send(task);
}

/// A simple actor that keeps track of an inactivity timer held in the stack of
/// its `start()` fn.
///
/// - If an activity event is received via `activity_bus`, the timer is reset to
///   now + `self.duration`.
/// - If the timer reaches 0, a shutdown signal is sent via `shutdown`.
/// - If a shutdown signal is received, the actor shuts down.
pub struct InactivityTimer<T> {
    /// The duration that the inactivity timer will reset to whenever it
    /// receives an activity event.
    duration: Duration,
    /// Used to receive activity events from the command server.
    activity_bus: EventsBus<T>,
    /// Used to signal the rest of the program to shut down.
    shutdown: NotifyOnce,
}

impl<T: Clone + Send + 'static> InactivityTimer<T> {
    pub fn new(
        inactivity_secs: u64,
        activity_bus: EventsBus<T>,
        shutdown: NotifyOnce,
    ) -> Self {
        let duration = Duration::from_secs(inactivity_secs);
        Self {
            duration,
            activity_bus,
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
        let timer = time::sleep(self.duration);

        // Pin the timer on the stack so it can be polled without being consumed
        tokio::pin!(timer);

        let mut activity_rx = self.activity_bus.subscribe();

        loop {
            tokio::select! {
                () = &mut timer => {
                    info!("Inactivity timer hit 0, sending shutdown signal");
                    self.shutdown.send();
                    break;
                }
                _ = activity_rx.recv() => {
                    debug!("Received activity event");
                    timer.as_mut().reset(Instant::now() + self.duration);
                }
                () = self.shutdown.recv() => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;

    use lexe_tokio::task::LxTask;

    use super::*;

    /// A simple struct that holds all the context required to test the
    /// InactivityTimer.
    struct TestContext {
        actor: InactivityTimer<()>,
        activity_bus: EventsBus<()>,
        shutdown: NotifyOnce,
    }

    fn get_test_context(inactivity_secs: u64) -> TestContext {
        let activity_bus = EventsBus::new();
        let shutdown = NotifyOnce::new();
        let actor_shutdown = shutdown.clone();
        let actor = InactivityTimer::new(
            inactivity_secs,
            activity_bus.clone(),
            actor_shutdown,
        );

        TestContext {
            actor,
            activity_bus,
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
            let inactivity_secs = 1;
            let ctxt = get_test_context(inactivity_secs);
            ctxt.activity_bus.send(());
            let actor_fut = ctxt.actor.run();

            // Actor should finish at 1000ms (1 sec)
            bound_finish(actor_fut, ctxt.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 2: no activity at all
    #[test]
    fn case_2() {
        do_test_paused_time(async {
            let inactivity_secs = 1;
            let ctxt = get_test_context(inactivity_secs);
            let actor_fut = ctxt.actor.run();

            // Actor should finish at about 1000ms (1 sec)
            bound_finish(actor_fut, ctxt.shutdown, Some(999), Some(1001)).await;
        });
    }

    /// Case 3: *with* activity 500ms in
    #[test]
    fn case_3() {
        do_test_paused_time(async {
            let inactivity_secs = 1;
            let ctxt = get_test_context(inactivity_secs);
            let actor_fut = ctxt.actor.run();

            // Spawn a task to generate an activity event 500ms in
            let activity_bus = ctxt.activity_bus.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                activity_bus.send(());
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
            let inactivity_secs = 1;
            let ctxt = get_test_context(inactivity_secs);
            let actor_fut = ctxt.actor.run();

            // Spawn a task to generate an activity event 500ms in and a
            // shutdown signal 750ms in
            let activity_bus = ctxt.activity_bus.clone();
            let shutdown = ctxt.shutdown.clone();
            let activity_task = LxTask::spawn_unnamed(async move {
                time::sleep(Duration::from_millis(500)).await;
                activity_bus.send(());
                time::sleep(Duration::from_millis(250)).await;
                shutdown.send();
            });

            // Actor should finish at about 750ms despite receiving activity
            bound_finish(actor_fut, ctxt.shutdown, Some(749), Some(751)).await;
            activity_task.await.unwrap();
        });
    }
}
