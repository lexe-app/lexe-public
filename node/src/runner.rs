use std::collections::HashMap;

use common::api::user::UserPk;
use futures::stream::FuturesUnordered;
use lexe_api::{error::MegaApiError, types::ports::RunPorts};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tokio::sync::{mpsc, oneshot};
use tracing::info_span;

pub(crate) struct RunUserRequestWithTx {
    /// The user to run.
    #[allow(dead_code)] // TODO(max): Remove
    pub user_pk: UserPk,
    /// A channel with which to respond to the server API handler.
    #[allow(dead_code)] // TODO(max): Remove
    pub user_ready_tx: oneshot::Sender<Result<RunPorts, MegaApiError>>,
}

/// Runs user nodes upon request.
pub(crate) struct UserRunner {
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
        runner_rx: mpsc::Receiver<RunUserRequestWithTx>,
        mega_shutdown: NotifyOnce,
    ) -> Self {
        Self {
            user_nodes: HashMap::new(),
            user_stream: FuturesUnordered::new(),
            mega_shutdown,
            runner_rx,
        }
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

    pub fn spawn_into_task(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(user-runner)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            self.run().await
        })
    }
}
