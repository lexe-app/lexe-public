//! Implements fuzz tests for the UserRunner.
//!
//! NOTE that as the UserRunner is fuzzed, real tasks are spawned.

use std::{env, str::FromStr, task::Poll, time::Duration};

use common::{
    rng::{FastRng, Rng, RngExt, SysRng},
    time::TimestampMs,
};
use futures::StreamExt;
use lexe_api::models::runner::{
    MegaNodeApiUserEvictRequest, MegaNodeApiUserRunRequest,
};
use tokio::sync::oneshot;

use super::{
    RunnerCommand, UserRunner, UserRunnerUserEvictRequest,
    UserRunnerUserRunRequest,
};

struct UserRunnerFuzzer {
    runner: UserRunner,
    rng: FastRng,
    now: TimestampMs,
}

impl UserRunnerFuzzer {
    fn from_seed(seed: u64) -> Self {
        let runner = helpers::new_userrunner_with_seed(seed);

        helpers::print_mega_args(&runner.mega_args);

        let mut rng = FastRng::from_u64(seed);
        let now = TimestampMs::from_secs_u32(rng.gen_u32());

        Self { runner, rng, now }
    }

    /// Executes a command and asserts invariants are upheld.
    fn execute_command(&mut self, command: RunnerCommand) {
        match command {
            RunnerCommand::UserRunRequest(user_run_request) => self
                .runner
                .handle_user_run_request(user_run_request, self.now),
            RunnerCommand::UserEvictRequest(user_evict_request) =>
                self.runner.handle_user_evict_request(user_evict_request),
            RunnerCommand::UserActivity(user_pk) =>
                self.runner.handle_user_activity(user_pk, self.now),
        }

        self.runner.assert_invariants();
    }

    /// Poll `mega_stream` until all completed tasks are removed from the queue.
    /// The purpose of this to prevent memory leaks.
    fn ensure_user_stream_polled(&mut self) {
        let waker = futures::task::noop_waker();
        let mut cx = futures::task::Context::from_waker(&waker);
        while let Poll::Ready(Some(join_result)) =
            self.runner.user_stream.poll_next_unpin(&mut cx)
        {
            self.runner.handle_finished_usernode(join_result);
            self.runner.assert_invariants();
        }
    }

    fn generate_command(&mut self) -> RunnerCommand {
        let p = self.rng.gen_f64();

        if p < 0.6 {
            // -- UserRunRequest -- // (60% probability)

            if helpers::generate_valid_command(&mut self.rng) {
                // Generate a valid UserRunRequest
                let user_pk = helpers::sample_user_pk(&mut self.rng);
                let lease_id = self.rng.gen_u32();
                let mega_id = self.runner.mega_args.mega_id;
                let (user_ready_tx, _user_ready_rx) = oneshot::channel();

                let inner = MegaNodeApiUserRunRequest {
                    user_pk,
                    lease_id,
                    mega_id,
                    shutdown_after_sync: self.rng.gen_boolean(),
                };

                let user_run_request = UserRunnerUserRunRequest {
                    inner,
                    user_ready_waiter: user_ready_tx,
                };

                RunnerCommand::UserRunRequest(user_run_request)
            } else {
                // Generate a bogus UserRunRequest with wrong mega_id
                let user_pk = helpers::sample_user_pk(&mut self.rng);
                let lease_id = self.rng.gen_u32();
                // Use random mega_id to trigger error
                let mega_id = self.rng.gen_u16();
                let (user_ready_tx, _user_ready_rx) = oneshot::channel();

                let inner = MegaNodeApiUserRunRequest {
                    user_pk,
                    lease_id,
                    mega_id,
                    shutdown_after_sync: false,
                };

                let user_run_request = UserRunnerUserRunRequest {
                    inner,
                    user_ready_waiter: user_ready_tx,
                };

                RunnerCommand::UserRunRequest(user_run_request)
            }
        } else if p < 0.8 {
            // -- UserEvictRequest -- // (20% probability)

            if helpers::generate_valid_command(&mut self.rng)
                && !self.runner.user_lru.is_empty()
            {
                // Generate a valid UserEvictRequest for an existing user
                // Pick a random user from the LRU cache
                let users = self
                    .runner
                    .user_lru
                    .iter()
                    .map(|(user_pk, _)| *user_pk)
                    .collect::<Vec<_>>();
                let user_idx = (self.rng.gen_u64() as usize) % users.len();
                let user_pk = users[user_idx];

                let (shutdown_tx, _shutdown_rx) = oneshot::channel();
                let inner = MegaNodeApiUserEvictRequest {
                    user_pk,
                    mega_id: self.runner.mega_args.mega_id,
                };
                let user_evict_request = UserRunnerUserEvictRequest {
                    inner,
                    user_shutdown_waiter: shutdown_tx,
                };

                RunnerCommand::UserEvictRequest(user_evict_request)
            } else {
                // Generate a bogus UserEvictRequest for a non-existent user
                let user_pk = helpers::sample_user_pk(&mut self.rng);
                let (shutdown_tx, _shutdown_rx) = oneshot::channel();
                let inner = MegaNodeApiUserEvictRequest {
                    user_pk,
                    mega_id: self.runner.mega_args.mega_id,
                };
                let user_evict_request = UserRunnerUserEvictRequest {
                    inner,
                    user_shutdown_waiter: shutdown_tx,
                };

                RunnerCommand::UserEvictRequest(user_evict_request)
            }
        } else {
            // -- UserActivity -- // (20% probability)

            if !self.runner.user_nodes.is_empty() {
                // Pick a random user from active user nodes
                let users =
                    self.runner.user_nodes.keys().copied().collect::<Vec<_>>();
                let user_idx = (self.rng.gen_u64() as usize) % users.len();
                let user_pk = users[user_idx];

                RunnerCommand::UserActivity(user_pk)
            } else {
                // No active users, generate activity for a random user
                let user_pk = helpers::sample_user_pk(&mut self.rng);
                RunnerCommand::UserActivity(user_pk)
            }
        }
    }

    fn print_summary(&self) {
        println!("--- UserRunner State Summary ---");
        println!("Time: {}", self.now);

        let user_nodes = self.runner.user_nodes.len();
        let user_lru = self.runner.user_lru.len();
        let user_evicting = self.runner.user_evicting.len();
        println!(
            "User nodes: {user_nodes} \
             (LRU: {user_lru}, Evicting: {user_evicting})"
        );

        let activity_queue = self.runner.megarunner_activity_queue.len();
        println!("Activity queue: {activity_queue}");

        // Print memory usage
        let current_memory = self.runner.current_memory();
        let hard_limit = self.runner.hard_memory_limit();
        let soft_limit = self
            .runner
            .hard_memory_limit()
            .saturating_sub(self.runner.target_buffer_memory());
        println!(
            "Memory: {current_memory} / {hard_limit} (soft: {soft_limit})"
        );

        println!("--------------------------------");
    }

    fn maybe_advance_time(&mut self) {
        // 50% probability to advance time
        // Fully qualified otherwise gen_bool is ambiguous
        if self.rng.gen_boolean() {
            // Generate a random duration to add (up to 1s)
            let advance_ms = self.rng.gen_range(0..1_000);
            let duration = Duration::from_millis(advance_ms);

            // Try to add the duration to current time
            match self.now.checked_add(duration) {
                Some(new_time) => self.now = new_time,
                None => self.now = TimestampMs::MIN,
            }
        }
    }

    /// Checks for inactive users and meganode shutdown, asserting invariants.
    fn do_inactivity_check(&mut self) {
        self.runner.evict_any_inactive_usernodes(self.now);
        self.runner.shutdown_meganode_if_inactive(self.now);
        self.runner.assert_invariants();
    }
}

mod helpers {
    use common::{
        api::user::UserPk,
        cli::{node::MegaArgs, LspInfo},
        env::DeployEnv,
        ln::network::LxNetwork,
        rng::{FastRng, Rng, RngExt},
        time::TimestampMs,
    };
    use lexe_tokio::{notify_once::NotifyOnce, DEFAULT_CHANNEL_SIZE};
    use tokio::sync::mpsc;

    use super::UserRunner;
    use crate::context::MegaContext;

    /// Create a new UserRunner suitable for fuzzing with the given seed.
    pub(super) fn new_userrunner_with_seed(seed: u64) -> UserRunner {
        let mut rng = FastRng::from_u64(seed);
        let now = TimestampMs::from_secs_u32(rng.gen_u32());

        // Create MegaArgs with reasonable defaults for testing
        // Values copied from runner/src/config.rs constants
        let mega_args = MegaArgs {
            mega_id: rng.gen_u16(),
            backend_url: String::new(),
            // DEFAULT_USER_LEASE_LIFETIME
            lease_lifetime_secs: 60,
            // DEFAULT_USER_LEASE_RENEWAL_INTERVAL
            lease_renewal_interval_secs: 30,
            lsp: LspInfo::dummy(),
            lsp_url: String::new(),
            mega_inactivity_secs: 7200, // 2 hours (from dummy config)
            // 200 MiB DEFAULT_MEGANODE_MEMORY_OVERHEAD
            memory_overhead: 200 * (1 << 20),
            oauth: None,
            runner_url: String::new(),
            rust_backtrace: None,
            rust_log: None,
            untrusted_deploy_env: DeployEnv::Dev,
            untrusted_esplora_urls: vec![],
            untrusted_network: LxNetwork::Regtest,
            sgx_heap_size: 0x8000_0000, // 2 GB
            user_inactivity_secs: 3600, // 1 hour (from dummy config)
            // DEFAULT_USERNODE_BUFFER_SLOTS
            usernode_buffer_slots: 2,
            // 64 MiB DEFAULT_USERNODE_MEMORY_ESTIMATE
            usernode_memory: 64 * (1 << 20),
        };

        let mega_ctxt = MegaContext::dummy();

        let mega_shutdown = NotifyOnce::new();
        let mega_server_shutdown = NotifyOnce::new();
        let (_runner_tx, runner_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);
        let (eph_tasks_tx, _eph_tasks_rx) = mpsc::channel(DEFAULT_CHANNEL_SIZE);

        UserRunner::new(
            now,
            mega_args,
            mega_ctxt,
            mega_shutdown,
            mega_server_shutdown,
            runner_rx,
            eph_tasks_tx,
        )
    }

    /// Whether to generate a valid runner command.
    /// Otherwise, the caller should generate a bogus command.
    pub(super) fn generate_valid_command(rng: &mut impl Rng) -> bool {
        rng.gen_f64() < 0.9
    }

    /// Sample a UserPk from a limited set of 256 possible values.
    /// This reduces the sample space to increase the likelihood of
    /// interesting interactions between commands targeting the same user.
    pub(super) fn sample_user_pk(rng: &mut impl RngExt) -> UserPk {
        let user_idx = rng.gen_u8();
        UserPk::from_u64(u64::from(user_idx))
    }

    /// Print detailed information about mega args.
    pub(super) fn print_mega_args(mega_args: &MegaArgs) {
        let mega_id = mega_args.mega_id;
        let heap_size = mega_args.sgx_heap_size;
        let overhead = mega_args.memory_overhead;
        let usernode_mem = mega_args.usernode_memory;
        let buffer_slots = mega_args.usernode_buffer_slots;
        let user_inactive = mega_args.user_inactivity_secs;
        let mega_inactive = mega_args.mega_inactivity_secs;

        println!("MegaArgs configuration:");
        println!("  mega_id: {mega_id}");
        println!("  sgx_heap_size: 0x{heap_size:x} ({heap_size} bytes)");
        println!("  memory_overhead: {overhead} bytes");
        println!("  usernode_memory: {usernode_mem} bytes");
        println!("  usernode_buffer_slots: {buffer_slots}");

        let hard_limit = heap_size.saturating_sub(overhead);
        let hard_limit_gib = hard_limit as f64 / (1 << 30) as f64;
        println!(
            "  hard_memory_limit: {hard_limit} bytes ({hard_limit_gib:.2} GiB)"
        );

        let max_users = hard_limit / usernode_mem;
        println!("  max_users (at hard limit): {max_users}");

        let buffer_memory = buffer_slots as u64 * usernode_mem;
        let soft_limit = hard_limit.saturating_sub(buffer_memory);
        let soft_limit_gib = soft_limit as f64 / (1 << 30) as f64;
        println!(
            "  soft_memory_limit: {soft_limit} bytes ({soft_limit_gib:.2} GiB)"
        );

        let soft_max_users = soft_limit / usernode_mem;
        println!("  max_users (at soft limit): {soft_max_users}");

        println!("  user_inactivity_secs: {user_inactive}");
        println!("  mega_inactivity_secs: {mega_inactive}");
    }
}

#[tokio::test(start_paused = true)]
async fn test_random_commands() {
    let seed = match env::var("USERRUNNER_SEED") {
        Ok(seed_str) => u64::from_str(&seed_str).expect("Invalid seed"),
        Err(_) => SysRng::new().gen_u64(),
    };
    println!("RNG seed: {seed}");
    println!("To rerun this seed, set:");
    println!("$ export USERRUNNER_SEED={seed}");

    let iters = match env::var("USERRUNNER_ITERS") {
        Ok(iters_str) => usize::from_str(&iters_str).expect("Invalid value"),
        Err(_) => 10000,
    };
    println!("Running {iters} fuzz iterations");

    let mut harness = UserRunnerFuzzer::from_seed(seed);

    // Execute random commands
    for i in 0..iters {
        // Print periodic status updates every 10000 iterations
        if i % 10000 == 0 {
            println!("\n=== Iteration {i} ===");
            harness.print_summary();
        }

        let command = harness.generate_command();

        // println!("Executing command: {command:?}");

        harness.execute_command(command);

        // Give dummy task a chance to respond
        tokio::time::sleep(Duration::from_nanos(1)).await;

        harness.ensure_user_stream_polled();

        // Maybe advance time
        harness.maybe_advance_time();

        // Check for inactive users
        harness.do_inactivity_check();
    }
}
