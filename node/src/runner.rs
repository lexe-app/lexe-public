use std::{
    collections::{HashMap, HashSet},
    mem,
    time::Duration,
};

use futures::{StreamExt, stream::FuturesUnordered};
use lexe_api::{
    cli::node::MegaArgs,
    error::{MegaApiError, MegaErrorKind},
    models::runner::{MegaNodeApiUserEvictRequest, MegaNodeApiUserRunRequest},
    types::{LeaseId, ports::RunPorts},
};
use lexe_common::{api::user::UserPk, constants::timeout, time::TimestampMs};
use lexe_enclave::allocator;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lru::LruCache;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinError,
    time::MissedTickBehavior,
};
use tracing::{debug, info, info_span, warn};

use crate::context::MegaContext;

#[cfg(test)]
mod fuzz;

/// How frequently the UserRunner logs meganode stats.
const STATS_INTERVAL: Duration = Duration::from_secs(60);

/// How frequently the UserRunner checks for inactivity.
/// - Inactive usernodes are evicted.
/// - If the meganode itself is inactive, it will shut itself down.
/// - (Optional) If the meganode itself has lived too long, it will shut itself
///   down.
const INACTIVITY_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// How frequently we send activity notifications to the megarunner.
/// If no notifications are queued, the notification is skipped.
const MEGARUNNER_NOTIFICATION_INTERVAL: Duration = Duration::from_secs(15);

/// Indicates a usernode has shutdown (or been evicted).
pub(crate) struct UserShutdown;

#[allow(clippy::enum_variant_names)]
pub(crate) enum UserRunnerCommand {
    UserRunRequest(UserRunnerUserRunRequest),
    UserEvictRequest(UserRunnerUserEvictRequest),
    /// Indicates that a usernode received some activity.
    UserActivity(UserPk),
}

/// A [`MegaNodeApiUserRunRequest`] but includes a waiter with which to respond.
pub(crate) struct UserRunnerUserRunRequest {
    pub inner: MegaNodeApiUserRunRequest,

    /// A channel with which to respond to the server API handler.
    pub user_ready_waiter: oneshot::Sender<Result<RunPorts, MegaApiError>>,
}

/// A [`MegaNodeApiUserEvictRequest`] but includes a waiter with which to
/// respond.
pub(crate) struct UserRunnerUserEvictRequest {
    pub inner: MegaNodeApiUserEvictRequest,

    /// A channel with which to respond to the server API handler.
    pub user_shutdown_waiter:
        oneshot::Sender<Result<UserShutdown, MegaApiError>>,
}

/// Runs user nodes upon request.
pub(crate) struct UserRunner {
    /// Heap usage before constructing the UserRunner, used to estimate shared
    /// resources (`NetworkGraph`, `ProbabilisticScorer`, provisioner, etc.).
    fixed_heap_bytes: usize,
    mega_args: MegaArgs,
    mega_ctxt: MegaContext,

    /// Shutdown channel for the meganode overall.
    mega_shutdown: NotifyOnce,
    /// A shutdown channel specifically used for the mega API server.
    /// This is a separate channel because the meganode needs to continue
    /// responding to liveness checks while the UserRunner shuts down.
    /// This is notified only once the UserRunner finishes running.
    mega_server_shutdown: NotifyOnce,

    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    runner_rx: mpsc::Receiver<UserRunnerCommand>,

    /// The last time any usernode on this meganode was active.
    mega_last_used: TimestampMs,
    /// The timestamp at which this meganode started.
    mega_started_at: TimestampMs,
    /// Recently active users that we haven't yet notified the megarunner of.
    megarunner_activity_queue: HashSet<UserPk>,
    /// Maximum observed per-usernode average heap usage across stat samples.
    max_avg_user_bytes: usize,
    /// Maximum number of usernodes ever running concurrently.
    max_users: usize,
    /// Total number of usernodes ever run.
    total_users: usize,

    user_nodes: HashMap<UserPk, UserHandle>,
    user_lru: LruCache<UserPk, TimestampMs>,
    user_evicting: HashSet<UserPk>,
    user_stream: FuturesUnordered<LxTask<UserPk>>,
}

/// A handle to a specific usernode.
struct UserHandle {
    /// The user node's lease ID. Subsequent run requests from the megarunner
    /// are expected to know this lease ID.
    lease_id: LeaseId,
    user_ready_waiter_tx:
        mpsc::Sender<oneshot::Sender<Result<RunPorts, MegaApiError>>>,
    user_shutdown: NotifyOnce,
    /// User shutdown waiters to be notified once the UserHandle is dropped.
    user_shutdown_waiters:
        Vec<oneshot::Sender<Result<UserShutdown, MegaApiError>>>,
}

impl UserHandle {
    /// Queues a readiness waiter to be answered by the usernode once ready.
    /// A waiter that can't be enqueued is answered with an error rather than
    /// dropped (which surfaces as an opaque "channel closed").
    fn enqueue_ready_waiter(
        &self,
        waiter: oneshot::Sender<Result<RunPorts, MegaApiError>>,
    ) {
        if let Err(send_err) = self.user_ready_waiter_tx.try_send(waiter) {
            use mpsc::error::TrySendError::{Closed, Full};
            let error = MegaApiError {
                kind: MegaErrorKind::RunnerUnreachable,
                msg: "Usernode is not accepting readiness waiters; \
                      its queue is full or it is exiting"
                    .to_string(),
                ..Default::default()
            };
            let waiter = match send_err {
                Full(waiter) | Closed(waiter) => waiter,
            };
            let _ = waiter.send(Err(error));
        }
    }
}

impl Drop for UserHandle {
    fn drop(&mut self) {
        // Notify all shutdown waiters that the user node has shut down.
        for waiter in self.user_shutdown_waiters.drain(..) {
            let _ = waiter.send(Ok(UserShutdown));
        }
    }
}

impl UserRunner {
    pub fn new(
        now: TimestampMs,
        fixed_heap_bytes: usize,
        mega_args: MegaArgs,
        mega_ctxt: MegaContext,
        mega_shutdown: NotifyOnce,
        mega_server_shutdown: NotifyOnce,
        runner_rx: mpsc::Receiver<UserRunnerCommand>,
        eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    ) -> Self {
        Self {
            fixed_heap_bytes,
            mega_args,
            mega_ctxt,
            mega_shutdown,
            mega_server_shutdown,

            eph_tasks_tx,
            runner_rx,

            mega_last_used: now,
            mega_started_at: now,
            megarunner_activity_queue: HashSet::new(),
            max_avg_user_bytes: 0,
            max_users: 0,
            total_users: 0,

            user_nodes: HashMap::new(),
            user_lru: LruCache::unbounded(),
            user_evicting: HashSet::new(),
            user_stream: FuturesUnordered::new(),
        }
    }

    pub fn spawn_into_task(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(user-runner)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            self.run().await
        })
    }

    async fn run(mut self) {
        let mut stats_interval = tokio::time::interval(STATS_INTERVAL);
        stats_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut inactivity_check_interval =
            tokio::time::interval(INACTIVITY_CHECK_INTERVAL);
        inactivity_check_interval.tick().await;

        let mut megarunner_notif_interval =
            tokio::time::interval(MEGARUNNER_NOTIFICATION_INTERVAL);
        megarunner_notif_interval.tick().await;

        // --- Regular operation --- //

        loop {
            let now = TimestampMs::now();

            tokio::select! {
                Some(cmd) = self.runner_rx.recv() => match cmd {
                    UserRunnerCommand::UserRunRequest(run_req) =>
                        self.handle_user_run_request(run_req, now),
                    UserRunnerCommand::UserEvictRequest(evict_req) =>
                        self.handle_user_evict_request(evict_req),
                    UserRunnerCommand::UserActivity(user_pk) =>
                        self.handle_user_activity(user_pk, now),
                },

                Some(join_result) = self.user_stream.next() =>
                    self.handle_finished_usernode(join_result),

                _ = stats_interval.tick() =>
                    self.log_stats("periodic"),

                _ = inactivity_check_interval.tick() => {
                    self.evict_any_inactive_usernodes(now);
                    self.shutdown_meganode_if_inactive(now);
                    self.shutdown_meganode_if_too_old(now);
                }

                _ = megarunner_notif_interval.tick() =>
                    self.maybe_notify_megarunner_user_activity(),

                () = self.mega_shutdown.recv() =>
                    break info!("Initiating shutdown of UserRunner"),
            }

            self.assert_invariants();
        }

        // --- Graceful shutdown --- //

        let shutdown_timeout =
            tokio::time::sleep(timeout::userrunner::SHUTDOWN_TIMEOUT);
        tokio::pin!(shutdown_timeout);

        // User shutdown waiters received during shutdown which will be notified
        // once runner shutdown completes.
        let mut user_shutdown_waiters = Vec::new();

        // Send shutdown signal to all running usernodes
        for (user_pk, handle) in self.user_nodes.iter() {
            debug!(%user_pk, "Sending shutdown signal to usernode");
            handle.user_shutdown.send();
        }

        loop {
            tokio::select! {
                // Continue to pop off commands and handle them during shutdown.
                // Immediately notify Run requests with error.
                // Save any user eviction requests to be notified later.
                Some(cmd) = self.runner_rx.recv() => {
                    match cmd {
                        UserRunnerCommand::UserRunRequest(req) => {
                            let error = MegaApiError {
                                kind: MegaErrorKind::RunnerUnreachable,
                                msg: "UserRunner is shutting down".to_string(),
                                ..Default::default()
                            };
                            let _ =
                                req.user_ready_waiter.send(Err(error));
                        }
                        UserRunnerCommand::UserEvictRequest(req) => {
                            user_shutdown_waiters
                                .push(req.user_shutdown_waiter);
                        }
                        // Ignore
                        UserRunnerCommand::UserActivity(_) => (),
                    }
                }

                // Continue to pop off user tasks from the stream, ideally
                // until all tasks have finished.
                maybe_join_result = self.user_stream.next() => {
                    match maybe_join_result {
                        Some(join_result) =>
                            self.handle_finished_usernode(join_result),
                        None => {
                            info!("All usernodes finished successfully.");

                            // Notify all user shutdown waiters of success
                            for waiter in user_shutdown_waiters {
                                let _ = waiter.send(Ok(UserShutdown));
                            }
                            break;
                        }
                    }
                }

                // If we hit the shutdown timeout, that means that at least one
                // usernode hung. Notify all user shutdown waiters of error.
                () = &mut shutdown_timeout => {
                    let error = MegaApiError {
                        kind: MegaErrorKind::RunnerUnreachable,
                        msg: "UserRunner shutdown timeout reached".to_string(),
                        ..Default::default()
                    };
                    for waiter in user_shutdown_waiters {
                        let _ = waiter.send(Err(error.clone()));
                    }

                    let num_hung = self.user_stream.len();
                    warn!(num_hung, "UserRunner shutdown timeout reached");

                    break;
                }
            }

            self.assert_invariants();
        }

        // We're done shutting down usernodes, so we can stop responding to
        // liveness checks. Trigger a shutdown of the meganode API server.
        self.mega_server_shutdown.send();

        self.log_stats("shutdown");
    }

    fn handle_user_run_request(
        &mut self,
        run_req: UserRunnerUserRunRequest,
        now: TimestampMs,
    ) {
        let UserRunnerUserRunRequest {
            inner: run_req,
            user_ready_waiter,
        } = run_req;
        let user_pk = run_req.user_pk;

        // If the user is running, just pass the waiter to the node and return.
        if let Some(user_handle) = self.user_nodes.get(&user_pk) {
            // Ensure the lease_id matches
            if user_handle.lease_id != run_req.lease_id {
                let _ = user_ready_waiter.send(Err(
                    MegaApiError::unknown_user(&user_pk, "Lease ID mismatch"),
                ));
                return;
            }

            // Pass the waiter to the node.
            user_handle.enqueue_ready_waiter(user_ready_waiter);

            // Mark the usernode and meganode as active.
            self.meganode_used_now(now);
            self.usernode_used_now(&user_pk, now);

            return;
        }
        // From here, we know the user is not already running.

        // If spawning this user would exceed our hard limit, then we'll
        // immediately reject the request.
        if self.current_memory() + self.mega_args.usernode_memory
            > self.hard_memory_limit()
        {
            let _ = user_ready_waiter.send(Err(MegaApiError::at_capacity(
                "Insufficient memory to schedule user node",
            )));
            return;
        }

        // Spawn the user node.
        #[cfg(not(test))]
        let (user_task, user_handle) = helpers::spawn_usernode(
            &self.mega_args,
            run_req,
            self.mega_ctxt.clone(),
        );

        #[cfg(test)]
        let (user_task, user_handle) = {
            let _ = &self.mega_args;
            let _ = &self.mega_ctxt;
            helpers::spawn_dummy_usernode(run_req)
        };

        // Immediately queue the `user_ready_waiter`.
        // It will live in the channel until the user node is ready.
        user_handle.enqueue_ready_waiter(user_ready_waiter);

        // Add to user state
        self.user_nodes.insert(user_pk, user_handle);
        self.total_users += 1;
        self.max_users = self.max_users.max(self.user_nodes.len());

        // Add to user stream
        self.user_stream.push(user_task);

        // Add to LRU queue
        self.user_lru.push(user_pk, now);

        // Mark meganode as active
        self.meganode_used_now(now);

        // We just spawned a node. Check for evictions.
        self.evict_usernodes_if_needed(now);
    }

    fn handle_user_activity(&mut self, user_pk: UserPk, now: TimestampMs) {
        debug!(%user_pk, "Received user activity, updating LRU");

        // Update the last_used value for the meganode itself.
        self.meganode_used_now(now);

        // Update the LRU queue for this user.
        self.usernode_used_now(&user_pk, now);

        // Add user to notification queue
        self.megarunner_activity_queue.insert(user_pk);
    }

    fn handle_user_evict_request(
        &mut self,
        evict_req: UserRunnerUserEvictRequest,
    ) {
        let UserRunnerUserEvictRequest {
            inner: evict_req,
            user_shutdown_waiter,
        } = evict_req;

        let handle = match self.user_nodes.get_mut(&evict_req.user_pk) {
            Some(h) => h,
            None => {
                // If the requested user node is not running, return an error.
                let _ =
                    user_shutdown_waiter.send(Err(MegaApiError::unknown_user(
                        &evict_req.user_pk,
                        "User not found for eviction",
                    )));
                return;
            }
        };

        // Send a shutdown signal to the user node.
        handle.user_shutdown.send();

        // Queue the waiter to be notified once the UserHandle is dropped.
        handle.user_shutdown_waiters.push(user_shutdown_waiter);
    }

    fn handle_finished_usernode(
        &mut self,
        join_result: Result<UserPk, JoinError>,
    ) {
        let user_pk = join_result.expect("User node task panicked");
        info!(%user_pk, "User node finished");

        // When this handle is dropped, all contained shutdown waiters are
        // notified that the usernode has been shut down.
        let handle = self
            .user_nodes
            .remove(&user_pk)
            .expect("Finished user node should exist in user_nodes");

        let was_in_lru = self.user_lru.pop(&user_pk).is_some();
        let was_evicting = self.user_evicting.remove(&user_pk);
        assert!(
            was_in_lru ^ was_evicting,
            "User node should be in LRU or evicting set, but not both",
        );

        // Notify the megarunner that the user has shut down,
        // so that the user's lease can be terminated.
        let task = helpers::spawn_user_finished_task(
            self.mega_ctxt.runner_api.clone(),
            user_pk,
            handle.lease_id,
            self.mega_args.mega_id,
        );
        let _ = self.eph_tasks_tx.try_send(task);
    }

    /// Checks for and evicts any usernodes which have been inactive for at
    /// least `user_inactivity_duration`.
    fn evict_any_inactive_usernodes(&mut self, now: TimestampMs) {
        // If a user is inactive for at least this long, we'll shut them down.
        let user_inactivity_duration =
            Duration::from_secs(self.mega_args.user_inactivity_secs);

        // Cutoff timestamp: users with last_used before this are inactive
        let inactive_ts = now.saturating_sub(user_inactivity_duration);

        // Collect users to evict (can't mutate while iterating)
        let inactive_users = self
            .user_lru
            // NOTE: `LruCache::iter` returns entries in MRU order.
            .iter()
            .rev()
            // Find all users with a `last_used` ts older than `inactive_ts`.
            // Stops upon finding a user that is still active.
            // Leverages invariant that timestamps are monotonically increasing.
            .take_while(|(_, last_used)| **last_used < inactive_ts)
            .map(|(user_pk, _)| *user_pk)
            .collect::<Vec<UserPk>>();

        // Evict each inactive user
        for user_pk in inactive_users {
            let evicted = self.evict_usernode(user_pk, now);
            assert!(evicted, "User was just in LRU");
        }
    }

    /// Checks if the meganode has been inactive and initiates shutdown if so.
    fn shutdown_meganode_if_inactive(&mut self, now: TimestampMs) {
        let max_inactive_secs = match self.mega_args.mega_inactivity_secs {
            Some(s) => s,
            // No `mega_inactivity_secs` configured; let the parent MegaRunner
            // shut us down if we are inactive.
            None => return,
        };
        let inactive_secs =
            now.saturating_duration_since(self.mega_last_used).as_secs();

        if inactive_secs > max_inactive_secs {
            info!(inactive_secs, "Meganode inactive, initiating shutdown");
            self.mega_shutdown.send();
        }
    }

    /// Checks if the meganode lifetime has exceeded its max lifetime config
    /// and initiates shutdown if so.
    fn shutdown_meganode_if_too_old(&mut self, now: TimestampMs) {
        let max_lifetime_secs = match self.mega_args.mega_max_lifetime_secs {
            Some(s) => s,
            None => return,
        };
        let lifetime_secs = now
            .saturating_duration_since(self.mega_started_at)
            .as_secs();

        if lifetime_secs > max_lifetime_secs {
            info!(lifetime_secs, "Meganode too old, initiating shutdown");
            self.mega_shutdown.send();
        }
    }

    fn evict_usernodes_if_needed(&mut self, now: TimestampMs) {
        // Available memory for usernodes after accounting for overhead
        let hard_memory_limit = self.hard_memory_limit();

        // Target memory to maintain usernode buffer slots
        let target_buffer_memory = self.target_buffer_memory();

        // The limit over which we'll evict usernodes.
        let soft_memory_limit =
            hard_memory_limit.saturating_sub(target_buffer_memory);

        while self.current_memory() - self.evicting_memory() > soft_memory_limit
        {
            let evicted = self.evict_lru_usernode(now);
            assert!(evicted, "Unevicted memory over limit => can evict");
        }
    }

    /// Evicts the least recently used usernode.
    /// Returns whether a usernode was evicted.
    fn evict_lru_usernode(&mut self, now: TimestampMs) -> bool {
        if let Some((user_pk, lru)) = self.user_lru.peek_lru() {
            helpers::log_eviction(*lru, now, "Evicting LRU usernode.");

            let evicted = self.evict_usernode(*user_pk, now);
            assert!(evicted, "LRU usernode should be evictable");

            true
        } else {
            false
        }
    }

    /// Evicts the given usernode.
    /// Returns whether the usernode was evicted (was in the LRU queue).
    fn evict_usernode(&mut self, user_pk: UserPk, now: TimestampMs) -> bool {
        // Pop from LRU and mark as evicting
        match self.user_lru.pop(&user_pk) {
            Some(last_used) =>
                helpers::log_eviction(last_used, now, "Evicted usernode."),
            None => return false,
        }
        self.user_evicting.insert(user_pk);

        // Send a shutdown signal to the user node.
        let usernode = self
            .user_nodes
            .get(&user_pk)
            .expect("LRU user_pk should exist in user_nodes");
        usernode.user_shutdown.send();

        true
    }

    /// Marks the meganode as active by updating the `mega_last_used` timestamp.
    fn meganode_used_now(&mut self, now: TimestampMs) {
        self.mega_last_used = now;
    }

    /// Marks a usernode as the most recently used node and updates its
    /// `last_used` timestamp.
    fn usernode_used_now(&mut self, user_pk: &UserPk, now: TimestampMs) {
        // NOTE: `get_mut` promotes the entry to the head (MRU) of the queue.
        if let Some(last_used) = self.user_lru.get_mut(user_pk) {
            *last_used = now;
        }
    }

    /// Amount of memory used by currently running user nodes. User nodes are
    /// counted regardless of whether they are booting, running, or evicting.
    fn current_memory(&self) -> u64 {
        (self.user_nodes.len() as u64) * self.mega_args.usernode_memory
    }

    /// Log meganode stats like heap usage and per-user estimated heap usage.
    /// Logs a warning if observed usage exceeds the estimates passed in via
    /// CLI args.
    fn log_stats(&mut self, reason: &'static str) {
        let heap_stats = allocator::stats();
        let heap_bytes = heap_stats.current_bytes;
        let heap_size_bytes = heap_stats.heap_size.unwrap_or_default();
        let users = self.user_nodes.len();

        // Roughly attribute everything above the fixed snapshot to usernodes.
        let users_bytes = heap_bytes.saturating_sub(self.fixed_heap_bytes);
        let avg_user_bytes = users_bytes.checked_div(users).unwrap_or_default();
        self.max_avg_user_bytes = self.max_avg_user_bytes.max(avg_user_bytes);

        let heap_pct = if heap_size_bytes == 0 {
            0.0
        } else {
            100.0 * heap_bytes as f64 / heap_size_bytes as f64
        };

        let to_mib = |bytes: usize| bytes as f64 / (1 << 20) as f64;
        let heap_mib = to_mib(heap_bytes);
        let heap_size_mib = to_mib(heap_size_bytes);
        let max_heap_mib = to_mib(heap_stats.max_bytes);
        let fixed_heap_mib = to_mib(self.fixed_heap_bytes);
        let avg_user_mib = to_mib(avg_user_bytes);
        let max_avg_user_mib = to_mib(self.max_avg_user_bytes);

        info!(
            reason,
            heap_mib = %format_args!("{heap_mib:.1}"),
            heap_pct = %format_args!("{heap_pct:.1}"),
            heap_size_mib = %format_args!("{heap_size_mib:.1}"),
            max_heap_mib = %format_args!("{max_heap_mib:.1}"),
            fixed_heap_mib = %format_args!("{fixed_heap_mib:.1}"),
            avg_user_mib = %format_args!("{avg_user_mib:.1}"),
            max_avg_user_mib = %format_args!("{max_avg_user_mib:.1}"),
            users,
            max_users = self.max_users,
            total_users = self.total_users,
            "Meganode stats",
        );

        // Log WARN if observed usage exceeds the estimates passed in by the
        // megarunner, which uses them to decide how many nodes fit in memory.
        let usernode_memory = self.mega_args.usernode_memory as usize;
        if avg_user_bytes > usernode_memory {
            let estimate_mib = to_mib(usernode_memory);
            warn!(
                "Avg usernode heap usage ({avg_user_mib:.1} MiB) exceeds \
                 the per-usernode memory estimate ({estimate_mib:.1} MiB)"
            );
        }
        let memory_overhead = self.mega_args.memory_overhead as usize;
        if self.fixed_heap_bytes > memory_overhead {
            let overhead_mib = to_mib(memory_overhead);
            warn!(
                "Fixed heap usage ({fixed_heap_mib:.1} MiB) exceeds \
                 the meganode memory overhead estimate ({overhead_mib:.1} MiB)"
            );
        }
    }

    /// Amount of memory usage by currently evicting user nodes.
    /// Is always <= [`Self::current_memory`].
    fn evicting_memory(&self) -> u64 {
        (self.user_evicting.len() as u64) * self.mega_args.usernode_memory
    }

    /// Hard memory limit available for usernodes after accounting for overhead.
    /// Calculated as `sgx_heap_size - memory_overhead`.
    fn hard_memory_limit(&self) -> u64 {
        self.mega_args
            .sgx_heap_size
            .saturating_sub(self.mega_args.memory_overhead)
    }

    /// Target buffer memory to maintain capacity for additional usernode slots.
    /// Calculated as `usernode_buffer_slots * usernode_memory`.
    fn target_buffer_memory(&self) -> u64 {
        self.mega_args.usernode_buffer_slots as u64
            * self.mega_args.usernode_memory
    }

    /// Sends any queued activity notifications to the megarunner.
    fn maybe_notify_megarunner_user_activity(&mut self) {
        if self.megarunner_activity_queue.is_empty() {
            return;
        }

        if cfg!(test) {
            self.megarunner_activity_queue.clear();
            return;
        }

        let user_pks = mem::take(&mut self.megarunner_activity_queue);

        helpers::notify_megarunner_user_activity(
            &self.eph_tasks_tx,
            self.mega_ctxt.runner_api.clone(),
            user_pks,
        );
    }

    /// Asserts invariants, to be called after every state transition. Since it
    /// only executes logic in debug mode, operations within can be arbitrarily
    /// expensive. Invariants which have a negligible runtime cost can be
    /// asserted directly in the places where they are most easily checked.
    fn assert_invariants(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        // Every user should be in user_lru XOR user_evicting.
        for user_pk in self.user_nodes.keys() {
            let in_lru = self.user_lru.contains(user_pk);
            let in_evicting = self.user_evicting.contains(user_pk);
            assert!(
                in_lru ^ in_evicting,
                "User {user_pk} should be in exactly one of \
                 `user_lru` or `user_evicting`"
            );
        }

        // Reverse: Everything in `user_lru` is in `user_nodes`.
        for (user_pk, _) in self.user_lru.iter() {
            assert!(
                self.user_nodes.contains_key(user_pk),
                "LRU user {user_pk} not in user_nodes"
            );
        }

        // Reverse: Everything in `user_evicting` is in `user_nodes`.
        for user_pk in self.user_evicting.iter() {
            assert!(
                self.user_nodes.contains_key(user_pk),
                "Evicting user {user_pk} not in user_nodes"
            );
        }

        // `user_lru` timestamps are in LRU order.
        // NOTE: `LruCache::iter` returns items in MRU order.
        let mut prev_ts = None;
        for (_, timestamp) in self.user_lru.iter().rev() {
            if let Some(prev) = prev_ts {
                assert!(
                    timestamp >= &prev,
                    "User LRU timestamps are not in LRU order"
                );
            }
            prev_ts = Some(*timestamp);
        }

        // `user_nodes.len() == user_stream.len()`
        let user_state_len = self.user_nodes.len();
        let user_stream_len = self.user_stream.len();
        assert_eq!(
            user_state_len, user_stream_len,
            "Usernode state length ({user_state_len}) does not match \
             user stream length ({user_stream_len})"
        );

        assert!(self.max_users >= user_state_len);
        assert!(self.total_users >= self.max_users);
    }
}

mod helpers {
    use std::sync::Arc;

    use anyhow::Context;
    use lexe_api::{
        def::MegaRunnerApi, models::runner::UserFinishedRequest, types::LeaseId,
    };
    use lexe_common::api::MegaId;
    use lexe_crypto::rng::SysRng;
    use tracing::{error, info, warn};

    use super::*;
    use crate::{
        client::RunnerClient,
        context::UserContext,
        run::{RunArgs, UserNode},
    };

    #[cfg_attr(test, allow(dead_code))]
    pub(super) fn spawn_usernode(
        mega_args: &MegaArgs,
        run_req: MegaNodeApiUserRunRequest,
        mega_ctxt: MegaContext,
    ) -> (LxTask<UserPk>, UserHandle) {
        let user_pk = run_req.user_pk;
        let run_args =
            build_run_args(mega_args, user_pk, run_req.shutdown_after_sync);

        let (user_ready_waiter_tx, user_ready_waiter_rx) =
            mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);
        let user_shutdown = NotifyOnce::new();
        let user_context = UserContext {
            lease_id: run_req.lease_id,
            user_shutdown: user_shutdown.clone(),
        };

        let handle = UserHandle {
            lease_id: run_req.lease_id,
            user_ready_waiter_tx,
            user_shutdown,
            user_shutdown_waiters: Vec::new(),
        };

        let usernode_span = build_usernode_span(&user_pk);
        let task = LxTask::spawn_with_span(
            format!("Usernode {user_pk}"),
            usernode_span,
            async move {
                // We hold this here instead of in `try_future` so that if the
                // usernode fails during init() or sync(), we can notify the
                // queued readiness waiters of the actual startup error, instead
                // of just dropping the rx which returns "channel closed".
                let mut maybe_ready_rx = Some(user_ready_waiter_rx);

                let try_future = async {
                    let mut rng = SysRng::new();
                    // Box: keep large one-time init future state out of
                    // long-lived task.
                    let mut node = Box::pin(UserNode::init(
                        &mut rng,
                        run_args,
                        mega_ctxt,
                        user_context,
                    ))
                    .await
                    .context("Error during run init")?;
                    node.sync().await.context("Error while syncing")?;

                    // We're ready; hand the readiness waiter channel to the
                    // node's ports responder.
                    let ready_rx = maybe_ready_rx
                        .take()
                        .expect("Present until we take it here");
                    node.spawn_ports_responder(ready_rx);

                    node.run().await.context("Error while running")
                };
                let result = try_future.await;

                match result {
                    Ok(()) => info!(%user_pk, "Usernode finished successfully"),
                    Err(e) => {
                        error!(%user_pk, "Usernode errored: {e:#}");

                        // This is Some iff startup failed. In this case, notify
                        // all queued readiness waiters with the actual error
                        // from `init()` or `sync()`.
                        if let Some(mut ready_rx) = maybe_ready_rx.take() {
                            let error = MegaApiError {
                                kind: MegaErrorKind::UserStartupFailure,
                                msg: format!("{e:#}"),
                                ..Default::default()
                            };
                            ready_rx.close();
                            while let Ok(waiter) = ready_rx.try_recv() {
                                let _ = waiter.send(Err(error.clone()));
                            }
                        }
                    }
                }

                user_pk
            },
        );

        (task, handle)
    }

    #[cfg(test)]
    pub(super) fn spawn_dummy_usernode(
        run_req: MegaNodeApiUserRunRequest,
    ) -> (LxTask<UserPk>, UserHandle) {
        let user_pk = run_req.user_pk;

        let (user_ready_waiter_tx, user_ready_waiter_rx) =
            mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);
        let mut user_shutdown = NotifyOnce::new();

        let handle = UserHandle {
            lease_id: run_req.lease_id,
            user_ready_waiter_tx,
            user_shutdown: user_shutdown.clone(),
            user_shutdown_waiters: Vec::new(),
        };

        let usernode_span = build_usernode_span(&user_pk);
        let task = LxTask::spawn_with_span(
            format!("Dummy usernode {user_pk}"),
            usernode_span,
            async move {
                let run_ports = RunPorts {
                    user_pk,
                    user_port: user_pk.to_u64() as u16,
                    lexe_port: (user_pk.to_u64().wrapping_add(1)) as u16,
                };

                let mut ready_rx = user_ready_waiter_rx;

                loop {
                    tokio::select! {
                        Some(waiter) = ready_rx.recv() => {
                            // Immediately respond with Ok(ports)
                            let _ = waiter.send(Ok(run_ports));
                        }
                        () = user_shutdown.recv() =>
                            break info!(%user_pk, "Dummy user shutting down"),
                    }
                }

                user_pk
            },
        );

        (task, handle)
    }

    fn build_run_args(
        mega_args: &MegaArgs,
        user_pk: UserPk,
        shutdown_after_sync: bool,
    ) -> RunArgs {
        let MegaArgs {
            mega_id: _,
            backend_url,

            lease_lifetime_secs,
            lease_renewal_interval_secs,
            lsp,
            lsp_url: _,
            mega_inactivity_secs: _,
            mega_max_lifetime_secs: _,
            memory_overhead: _,
            oauth: _,
            runner_url,
            rust_backtrace: _,
            rust_log: _,
            sgx_heap_size: _,
            usernode_sync_timeout_secs: _,
            untrusted_deploy_env,
            untrusted_esplora_urls: esplora_urls,
            untrusted_network,
            user_inactivity_secs,
            usernode_buffer_slots: _,
            usernode_memory: _,
            partners: _,
        } = mega_args;

        RunArgs {
            backend_url: backend_url.clone(),
            lease_lifetime_secs: *lease_lifetime_secs,
            lease_renewal_interval_secs: *lease_renewal_interval_secs,
            lsp: lsp.clone(),
            runner_url: runner_url.clone(),
            shutdown_after_sync,
            untrusted_deploy_env: *untrusted_deploy_env,
            untrusted_esplora_urls: esplora_urls.clone(),
            untrusted_network: *untrusted_network,
            user_inactivity_secs: *user_inactivity_secs,
            user_pk,
        }
    }

    fn build_usernode_span(user_pk: &UserPk) -> tracing::Span {
        let span = info_span!(
            parent: None,
            "(user)",
            user_pk = %user_pk.short(),
            user_idx = tracing::field::Empty
        );

        // Try to detect if this user is using a test RootSeed. If so, annotate
        // all logs with the index for easier integration test debugging.
        #[cfg(feature = "test-utils")]
        for user_idx in 0..10 {
            let seed = lexe_common::root_seed::RootSeed::from_u64(user_idx);
            let derived_user_pk = seed.derive_user_pk();
            if user_pk == &derived_user_pk {
                span.record("user_idx", user_idx);
                break;
            }
        }

        span
    }

    /// Spawns a task to notify the runner that a user has finished.
    pub(super) fn spawn_user_finished_task(
        runner_api: Arc<RunnerClient>,
        user_pk: UserPk,
        lease_id: LeaseId,
        mega_id: MegaId,
    ) -> LxTask<()> {
        const SPAN_NAME: &str = "(notify-user-finished)";
        LxTask::spawn_with_span(
            SPAN_NAME,
            info_span!(SPAN_NAME, %user_pk, %lease_id, %mega_id),
            async move {
                let req = UserFinishedRequest {
                    user_pk,
                    lease_id,
                    mega_id,
                };

                match runner_api.user_finished(&req).await {
                    Ok(_) => info!(
                        "Successfully notified megarunner of user shutdown"
                    ),
                    Err(e) => error!(
                        "Couldn't notify megarunner of user shutdown: {e:#}"
                    ),
                }
            },
        )
    }

    /// Spawns a task to notify the megarunner of user activity.
    pub(super) fn notify_megarunner_user_activity(
        eph_tasks_tx: &mpsc::Sender<LxTask<()>>,
        runner_api: Arc<RunnerClient>,
        user_pks: HashSet<UserPk>,
    ) {
        const SPAN_NAME: &str = "(megarunner-activity)";
        let span = info_span!(SPAN_NAME);
        let task = LxTask::spawn_unlogged_with_span(SPAN_NAME, span, {
            async move {
                if let Err(e) = runner_api.activity(user_pks).await {
                    warn!("Couldn't notify megarunner of activity: {e:#}");
                }
            }
        });
        let _ = eph_tasks_tx.try_send(task);
    }

    /// Logs an eviction with the time since last use.
    pub(super) fn log_eviction(
        last_used: TimestampMs,
        now: TimestampMs,
        msg: &str,
    ) {
        let inactive_secs = now.saturating_duration_since(last_used).as_secs();
        info!(inactive_secs, "{msg}");
    }
}
