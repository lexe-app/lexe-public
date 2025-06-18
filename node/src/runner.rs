use std::collections::HashMap;

use common::{api::user::UserPk, cli::node::MegaArgs};
use futures::stream::FuturesUnordered;
use lexe_api::{
    error::MegaApiError, models::mega::RunUserRequest, types::ports::RunPorts,
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::sync::{mpsc, oneshot};
use tracing::info_span;

/// A [`RunUserRequest`] but includes a waiter with which to respond.
pub(crate) struct RunUserRequestWithTx {
    #[allow(dead_code)] // TODO(max): Remove
    pub inner: RunUserRequest,

    /// A channel with which to respond to the server API handler.
    #[allow(dead_code)] // TODO(max): Remove
    pub user_ready_tx: oneshot::Sender<Result<RunPorts, MegaApiError>>,
}

/// Runs user nodes upon request.
pub(crate) struct UserRunner {
    #[allow(dead_code)] // TODO(max): Remove
    mega_args: MegaArgs,

    #[allow(dead_code)] // TODO(max): Remove
    user_nodes: HashMap<UserPk, UserState>,

    #[allow(dead_code)] // TODO(max): Remove
    user_stream: FuturesUnordered<LxTask<UserPk>>,

    mega_shutdown: NotifyOnce,
    runner_rx: mpsc::Receiver<RunUserRequestWithTx>,
}

/// State related to a specific usernode.
struct UserState {
    // TODO(max): Implement
}

impl UserRunner {
    // TODO(max): Implement
    pub fn new(
        mega_args: MegaArgs,
        runner_rx: mpsc::Receiver<RunUserRequestWithTx>,
        mega_shutdown: NotifyOnce,
    ) -> Self {
        Self {
            mega_args,
            user_nodes: HashMap::new(),
            user_stream: FuturesUnordered::new(),
            mega_shutdown,
            runner_rx,
        }
    }

    pub fn spawn_into_task(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(user-runner)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            self.run().await
        })
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                Some(run_req) = self.runner_rx.recv() => {
                    // TODO(max): Implement
                    let _ = run_req;
                }

                () = self.mega_shutdown.recv() => return,
            }
        }
    }
}

mod helpers {
    use anyhow::Context;
    use common::{cli::node::RunArgs, rng::SysRng};
    use tracing::{error, info};

    use super::*;
    use crate::run::UserNode;

    #[allow(dead_code)] // TODO(max): Remove
    fn spawn_user_node(
        mega_args: &MegaArgs,
        run_req: &RunUserRequest,
    ) -> LxTask<UserPk> {
        let user_pk = run_req.user_pk;
        let run_args =
            build_run_args(mega_args, user_pk, run_req.shutdown_after_sync);

        // TODO(max): Pass in channels so the UserRunner can communicate with
        // the usernode.
        let usernode_span = build_usernode_span(&user_pk);
        LxTask::spawn_with_span(
            format!("Usernode {user_pk}"),
            usernode_span,
            async move {
                let try_future = async move {
                    let mut rng = SysRng::new();
                    let mut node = UserNode::init(&mut rng, run_args)
                        .await
                        .context("Error during run init")?;
                    node.sync().await.context("Error while syncing")?;
                    node.run().await.context("Error while running")
                };

                match try_future.await {
                    Ok(()) => info!(%user_pk, "Usernode finished successfully"),
                    Err(e) => error!(%user_pk, "Usernode errored: {e:#}"),
                }

                user_pk
            },
        )
    }

    fn build_run_args(
        mega_args: &MegaArgs,
        user_pk: UserPk,
        shutdown_after_sync: bool,
    ) -> RunArgs {
        let MegaArgs {
            mega_id: _,
            backend_url,
            runner_url,

            esplora_urls,
            inactivity_timer_sec,
            lsp,
            oauth: _,
            rust_backtrace,
            rust_log,
            untrusted_deploy_env,
            untrusted_network,
        } = mega_args;

        RunArgs {
            user_pk,
            shutdown_after_sync,
            inactivity_timer_sec: *inactivity_timer_sec,
            allow_mock: false,
            backend_url: Some(backend_url.clone()),
            runner_url: Some(runner_url.clone()),
            esplora_urls: esplora_urls.clone(),
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
}
