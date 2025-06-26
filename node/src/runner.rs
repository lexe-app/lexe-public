use std::collections::{HashMap, HashSet};

use common::{api::user::UserPk, cli::node::MegaArgs, time::TimestampMs};
use futures::{stream::FuturesUnordered, StreamExt};
use lexe_api::{
    error::MegaApiError,
    models::runner::{MegaNodeUserEvictionRequest, MegaNodeUserRunRequest},
    types::{ports::RunPorts, LeaseId},
};
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lru::LruCache;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinError,
};
use tracing::{debug, info, info_span};

use crate::context::MegaContext;

// TODO(max): Add inactivity timers for meganode, usernode

/// Indicates a usernode has shutdown (or been evicted).
pub(crate) struct UserShutdown;

pub(crate) enum RunnerCommand {
    UserRunRequest(UserRunnerUserRunRequest),
    UserEvictionRequest(UserRunnerUserEvictionRequest),
}

/// A [`MegaNodeUserRunRequest`] but includes a waiter with which to respond.
pub(crate) struct UserRunnerUserRunRequest {
    pub inner: MegaNodeUserRunRequest,

    /// A channel with which to respond to the server API handler.
    pub user_ready_waiter: oneshot::Sender<Result<RunPorts, MegaApiError>>,
}

/// A [`MegaNodeUserEvictionRequest`] but includes a waiter with which to
/// respond.
pub(crate) struct UserRunnerUserEvictionRequest {
    pub inner: MegaNodeUserEvictionRequest,

    /// A channel with which to respond to the server API handler.
    pub user_shutdown_waiter:
        oneshot::Sender<Result<UserShutdown, MegaApiError>>,
}

/// Runs user nodes upon request.
pub(crate) struct UserRunner {
    mega_args: MegaArgs,
    mega_ctxt: MegaContext,
    mega_activity_bus: EventsBus<UserPk>,
    mega_shutdown: NotifyOnce,

    eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    runner_rx: mpsc::Receiver<RunnerCommand>,

    user_nodes: HashMap<UserPk, UserHandle>,
    user_lru: LruCache<UserPk, TimestampMs>,
    user_evicting: HashSet<UserPk>,
    user_stream: FuturesUnordered<LxTask<(UserPk, LeaseId)>>,
}

/// A handle to a specific usernode.
struct UserHandle {
    user_ready_waiter_tx:
        mpsc::Sender<oneshot::Sender<Result<RunPorts, MegaApiError>>>,
    user_shutdown: NotifyOnce,
    /// User shutdown waiters to be notified once the UserHandle is dropped.
    user_shutdown_waiters:
        Vec<oneshot::Sender<Result<UserShutdown, MegaApiError>>>,
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
        mega_args: MegaArgs,
        mega_ctxt: MegaContext,
        mega_shutdown: NotifyOnce,
        runner_rx: mpsc::Receiver<RunnerCommand>,
        eph_tasks_tx: mpsc::Sender<LxTask<()>>,
    ) -> Self {
        let mega_activity_bus = mega_ctxt.mega_activity_bus.clone();
        Self {
            mega_args,
            mega_ctxt,
            mega_activity_bus,
            mega_shutdown,

            eph_tasks_tx,
            runner_rx,

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

    pub async fn run(mut self) {
        let mega_activity_bus = self.mega_activity_bus.clone();
        let mut mega_activity_rx = mega_activity_bus.subscribe();

        loop {
            tokio::select! {
                Some(cmd) = self.runner_rx.recv() => match cmd {
                    RunnerCommand::UserRunRequest(run_req) =>
                        self.handle_user_run_request(run_req),
                    RunnerCommand::UserEvictionRequest(evict_req) =>
                        self.handle_user_eviction_request(evict_req),
                },

                Some(join_result) = self.user_stream.next() =>
                    self.handle_finished_user_node(join_result),

                user_pk = mega_activity_rx.recv() =>
                    self.handle_user_activity(user_pk),

                () = self.mega_shutdown.recv() => return,
            }
        }
    }

    fn handle_user_run_request(&mut self, run_req: UserRunnerUserRunRequest) {
        let UserRunnerUserRunRequest {
            inner: run_req,
            user_ready_waiter,
        } = run_req;
        let user_pk = run_req.user_pk;

        // If the user is running, just pass the waiter to the node and return.
        if let Some(user_handle) = self.user_nodes.get(&user_pk) {
            let _ =
                user_handle.user_ready_waiter_tx.try_send(user_ready_waiter);
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
        let (user_task, user_handle) = helpers::spawn_user_node(
            &self.mega_args,
            run_req,
            self.mega_ctxt.clone(),
        );

        // Immediately queue the `user_ready_waiter`.
        // It will live in the channel until the user node is ready.
        user_handle
            .user_ready_waiter_tx
            .try_send(user_ready_waiter)
            .expect("Rx is currently on the stack");

        // Add to user state
        self.user_nodes.insert(user_pk, user_handle);

        // Add to user stream
        self.user_stream.push(user_task);

        // Add to LRU queue
        self.user_lru.push(user_pk, TimestampMs::now());

        // We just spawned a node. Check for evictions.
        self.evict_usernodes_if_needed();
    }

    fn handle_user_activity(&mut self, user_pk: UserPk) {
        debug!(%user_pk, "Received user activity, updating LRU");
        self.usernode_used_now(&user_pk);
    }

    fn handle_user_eviction_request(
        &mut self,
        evict_req: UserRunnerUserEvictionRequest,
    ) {
        let UserRunnerUserEvictionRequest {
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

    fn handle_finished_user_node(
        &mut self,
        join_result: Result<(UserPk, LeaseId), JoinError>,
    ) {
        let (user_pk, lease_id) = join_result.expect("User node task panicked");
        info!(%user_pk, %lease_id, "User node finished");

        // When this handle is dropped, all contained shutdown waiters are
        // notified that the usernode has been shut down.
        let _handle = self
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
            lease_id,
            self.mega_args.mega_id,
        );
        let _ = self.eph_tasks_tx.try_send(task);
    }

    fn evict_usernodes_if_needed(&mut self) {
        // Available memory for usernodes after accounting for overhead
        let hard_memory_limit = self.hard_memory_limit();

        // Target memory to maintain usernode buffer slots
        let target_buffer_memory = self.target_buffer_memory();

        // The limit over which we'll evict usernodes.
        let soft_memory_limit =
            hard_memory_limit.saturating_sub(target_buffer_memory);

        while self.current_memory() - self.evicting_memory() > soft_memory_limit
        {
            let evicted = self.evict_usernode();
            assert!(evicted, "Unevicted memory over limit => can evict");
        }
    }

    /// Evicts the least recently used usernode.
    /// Returns whether a usernode was evicted.
    fn evict_usernode(&mut self) -> bool {
        if let Some((user_pk, lru)) = self.user_lru.pop_lru() {
            let now = TimestampMs::now();
            let inactive_secs = lru.absolute_diff(now).as_secs();
            info!(%inactive_secs, "Evicted usernode.");

            self.user_evicting.insert(user_pk);

            // Send a shutdown signal to the user node.
            let usernode = self
                .user_nodes
                .get(&user_pk)
                .expect("LRU user_pk should exist in user_nodes");
            usernode.user_shutdown.send();

            true
        } else {
            false
        }
    }

    /// Marks a usernode as the most recently used node and updates its
    /// `last_used` timestamp.
    fn usernode_used_now(&mut self, user_pk: &UserPk) {
        // NOTE: `get_mut` promotes the entry to the head (MRU) of the queue.
        if let Some(last_used) = self.user_lru.get_mut(user_pk) {
            *last_used = TimestampMs::now();
        }
    }

    /// Amount of memory used by currently running user nodes. User nodes are
    /// counted regardless of whether they are booting, running, or evicting.
    fn current_memory(&self) -> u64 {
        (self.user_nodes.len() as u64) * self.mega_args.usernode_memory
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
}

mod helpers {
    use std::sync::Arc;

    use anyhow::Context;
    use common::{api::MegaId, cli::node::RunArgs, rng::SysRng};
    use lexe_api::{models::runner::UserFinishedRequest, types::LeaseId};
    use tracing::{error, info};

    use super::*;
    use crate::{api::RunnerApiClient, context::UserContext, run::UserNode};

    pub(super) fn spawn_user_node(
        mega_args: &MegaArgs,
        run_req: MegaNodeUserRunRequest,
        mega_ctxt: MegaContext,
    ) -> (LxTask<(UserPk, LeaseId)>, UserHandle) {
        let user_pk = run_req.user_pk;
        let run_args =
            build_run_args(mega_args, user_pk, run_req.shutdown_after_sync);

        let (user_ready_waiter_tx, user_ready_waiter_rx) =
            mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);
        let user_shutdown = NotifyOnce::new();
        let user_context = UserContext {
            lease_id: Some(run_req.lease_id),
            user_shutdown: user_shutdown.clone(),
            user_ready_waiter_rx,
        };

        let handle = UserHandle {
            user_ready_waiter_tx,
            user_shutdown,
            user_shutdown_waiters: Vec::new(),
        };

        let usernode_span = build_usernode_span(&user_pk);
        let task = LxTask::spawn_with_span(
            format!("Usernode {user_pk}"),
            usernode_span,
            async move {
                let try_future = async move {
                    let mut rng = SysRng::new();
                    let static_tasks = Vec::new();
                    let mut node = UserNode::init(
                        &mut rng,
                        run_args,
                        mega_ctxt,
                        user_context,
                        static_tasks,
                    )
                    .await
                    .context("Error during run init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                };

                match try_future.await {
                    Ok(()) => info!(%user_pk, "Usernode finished successfully"),
                    Err(e) => error!(%user_pk, "Usernode errored: {e:#}"),
                }

                (user_pk, run_req.lease_id)
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

            inactivity_timer_sec,
            lease_lifetime_secs,
            lease_renewal_interval_secs,
            lsp,
            memory_overhead: _,
            oauth: _,
            runner_url,
            rust_backtrace,
            rust_log,
            untrusted_deploy_env,
            untrusted_esplora_urls: esplora_urls,
            untrusted_network,
            sgx_heap_size: _,
            usernode_buffer_slots: _,
            usernode_memory: _,
        } = mega_args;

        RunArgs {
            user_pk,
            shutdown_after_sync,
            inactivity_timer_sec: *inactivity_timer_sec,
            lease_lifetime_secs: *lease_lifetime_secs,
            lease_renewal_interval_secs: *lease_renewal_interval_secs,
            allow_mock: false,
            backend_url: Some(backend_url.clone()),
            runner_url: Some(runner_url.clone()),
            untrusted_esplora_urls: esplora_urls.clone(),
            lsp: lsp.clone(),
            untrusted_deploy_env: *untrusted_deploy_env,
            untrusted_network: *untrusted_network,
            rust_backtrace: rust_backtrace.clone(),
            rust_log: rust_log.clone(),
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
            let seed = common::root_seed::RootSeed::from_u64(user_idx);
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
        runner_api: Arc<dyn RunnerApiClient + Send + Sync>,
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
}
