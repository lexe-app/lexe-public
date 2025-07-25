use anyhow::Context;
use common::{cli::node::MegaArgs, constants, rng::Crng, time::TimestampMs};
use lexe_api::{def::MegaRunnerApi, types::ports::MegaPorts};
use lexe_tls::attestation::NodeMode;
use lexe_tokio::{notify_once::NotifyOnce, task};
use tokio::sync::mpsc;

use crate::{
    client::RunnerClient,
    context::MegaContext,
    provision::{ProvisionArgs, ProvisionInstance},
    runner::UserRunner,
};

pub async fn run(rng: &mut impl Crng, args: MegaArgs) -> anyhow::Result<()> {
    let (runner_tx, runner_rx) =
        mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);
    let (eph_tasks_tx, eph_tasks_rx) =
        mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);

    // Create mega context for user nodes
    let mega_shutdown = NotifyOnce::new();
    let (mega_ctxt, mut static_tasks) = MegaContext::init(
        rng,
        args.backend_url.clone(),
        args.lsp_url.clone(),
        args.runner_url.clone(),
        args.untrusted_deploy_env,
        args.untrusted_esplora_urls.clone(),
        args.untrusted_network,
        runner_tx.clone(),
        mega_shutdown.clone(),
    )
    .await
    .context("Couldn't init MegaContext")?;

    // Init the provision service. Since it's a static service that should
    // live as long as the mega node itself, we can reuse the mega_shutdown.
    let provision_args = ProvisionArgs::from(&args);
    let provision = ProvisionInstance::init(
        rng,
        provision_args,
        mega_ctxt.clone(),
        mega_shutdown.clone(),
    )
    .await
    .context("Couldn't init ProvisionInstance")?;
    let provision_ports = provision.ports();
    static_tasks.push(provision.spawn_into_task());

    // Start the usernode runner.
    let now = TimestampMs::now();
    let measurement = mega_ctxt.measurement;
    let mega_server_shutdown = NotifyOnce::new();
    let user_runner = UserRunner::new(
        now,
        args.clone(),
        mega_ctxt,
        mega_shutdown.clone(),
        mega_server_shutdown.clone(),
        runner_rx,
        eph_tasks_tx,
    );
    static_tasks.push(user_runner.spawn_into_task());

    // Spawn the mega server task.
    let mega_id = args.mega_id;
    let mega_state = mega_server::MegaRouterState {
        mega_id,
        runner_tx,
        mega_shutdown: mega_shutdown.clone(),
    };
    let (mega_task, lexe_mega_port, _mega_url) =
        mega_server::spawn_server_task(mega_state, mega_server_shutdown)
            .context("Failed to spawn mega server task")?;
    static_tasks.push(mega_task);

    // Init runner client.
    let mr_short = measurement.short();
    let runner_client = RunnerClient::new(
        rng,
        // TODO(max): This deploy env doesn't secure anything dangerous, but we
        // should still find a way to validate the untrusted deploy env.
        args.untrusted_deploy_env,
        NodeMode::Mega { mr_short },
        args.runner_url.clone(),
    )
    .context("Couldn't init runner client")?;

    // Let the runner know that the mega node is ready to load user nodes.
    let ports = MegaPorts {
        mega_id,
        measurement,
        app_provision_port: provision_ports.app_port,
        lexe_mega_port,
    };
    runner_client
        .mega_ready(&ports)
        .await
        .context("Mega ready callback failed")?;

    // We're now ready. Poll for panics and prematurely finished tasks.

    task::try_join_tasks_and_shutdown(
        static_tasks,
        eph_tasks_rx,
        mega_shutdown,
        constants::USER_NODE_SHUTDOWN_TIMEOUT,
    )
    .await
    .context("Error awaiting tasks")?;

    Ok(())
}

mod mega_server {
    use std::{borrow::Cow, net::TcpListener};

    use anyhow::Context;
    use axum::{
        routing::{get, post},
        Router,
    };
    use common::{api::MegaId, net};
    use lexe_api::{server::LayerConfig, types::ports::Port};
    use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
    use tokio::sync::mpsc;
    use tracing::info_span;

    use super::handlers;
    use crate::runner::UserRunnerCommand;

    /// Spawns the Lexe mega server task; returns the task, port, and url.
    pub(super) fn spawn_server_task(
        state: MegaRouterState,
        // A shutdown channel specifically used for the mega API server.
        // Not to be confused with `mega_shutdown`.
        // This is a separate channel because the meganode needs to continue
        // responding to liveness checks while the UserRunner shuts down.
        mega_server_shutdown: NotifyOnce,
    ) -> anyhow::Result<(LxTask<()>, Port, String)> {
        const SERVER_SPAN_NAME: &str = "(mega-server)";
        let lexe_mega_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind mega listener")?;
        let lexe_mega_port = lexe_mega_listener
            .local_addr()
            .context("Couldn't get mega addr")?
            .port();
        let tls_and_dns = None;
        let (task, lexe_mega_url) =
            lexe_api::server::spawn_server_task_with_listener(
                lexe_mega_listener,
                mega_router(state),
                LayerConfig::default(),
                tls_and_dns,
                Cow::from(SERVER_SPAN_NAME),
                info_span!(SERVER_SPAN_NAME),
                mega_server_shutdown,
            )
            .context("Failed to spawn Lexe mega server task")?;

        Ok((task, lexe_mega_port, lexe_mega_url))
    }

    #[derive(Clone)]
    pub(super) struct MegaRouterState {
        pub mega_id: MegaId,
        pub runner_tx: mpsc::Sender<UserRunnerCommand>,
        /// Shutdown channel for the meganode overall.
        /// Not to be confused with `mega_server_shutdown`.
        pub mega_shutdown: NotifyOnce,
    }

    /// Implements [`LexeMegaApi`] - only callable by the Lexe operators.
    ///
    /// [`LexeMegaApi`]: lexe_api::def::LexeMegaApi
    fn mega_router(state: MegaRouterState) -> Router<()> {
        Router::new()
            .route("/lexe/run_user", post(handlers::run_user))
            .route("/lexe/evict_user", post(handlers::evict_user))
            .route("/lexe/status", get(handlers::status))
            .route("/lexe/shutdown", post(handlers::shutdown))
            .with_state(state)
    }
}

/// API handlers.
mod handlers {
    use axum::extract::State;
    use common::{
        api::{models::Status, MegaIdStruct},
        time::TimestampMs,
    };
    use lexe_api::{
        error::{MegaApiError, MegaErrorKind},
        models::runner::{
            MegaNodeApiUserEvictRequest, MegaNodeApiUserRunRequest,
            MegaNodeApiUserRunResponse,
        },
        server::{extract::LxQuery, LxJson},
        types::Empty,
    };
    use tokio::sync::oneshot;

    use super::mega_server::MegaRouterState;
    use crate::runner::{
        UserRunnerCommand, UserRunnerUserEvictRequest, UserRunnerUserRunRequest,
    };

    pub(super) async fn run_user(
        State(state): State<MegaRouterState>,
        LxJson(req): LxJson<MegaNodeApiUserRunRequest>,
    ) -> Result<LxJson<MegaNodeApiUserRunResponse>, MegaApiError> {
        // Sanity check
        if req.mega_id != state.mega_id {
            return Err(MegaApiError::wrong_mega_id(
                &req.mega_id,
                &state.mega_id,
            ));
        }

        let (user_ready_tx, user_ready_rx) = oneshot::channel();
        let req_with_tx = UserRunnerUserRunRequest {
            inner: req,
            user_ready_waiter: user_ready_tx,
        };
        let cmd = UserRunnerCommand::UserRunRequest(req_with_tx);
        state.runner_tx.try_send(cmd).map_err(|e| MegaApiError {
            kind: MegaErrorKind::RunnerUnreachable,
            msg: e.to_string(),
            ..Default::default()
        })?;

        let run_ports = user_ready_rx.await.map_err(|e| MegaApiError {
            kind: MegaErrorKind::RunnerUnreachable,
            msg: e.to_string(),
            ..Default::default()
        })??;

        Ok(LxJson(MegaNodeApiUserRunResponse { run_ports }))
    }

    pub(super) async fn status(
        State(state): State<MegaRouterState>,
        LxQuery(req): LxQuery<MegaIdStruct>,
    ) -> Result<LxJson<Status>, MegaApiError> {
        // Sanity check
        if req.mega_id != state.mega_id {
            return Err(MegaApiError::wrong_mega_id(
                &req.mega_id,
                &state.mega_id,
            ));
        }

        Ok(LxJson(Status {
            timestamp: TimestampMs::now(),
        }))
    }

    pub(super) async fn shutdown(
        State(state): State<MegaRouterState>,
    ) -> LxJson<Empty> {
        state.mega_shutdown.send();
        LxJson(Empty {})
    }

    pub(super) async fn evict_user(
        State(state): State<MegaRouterState>,
        LxJson(req): LxJson<MegaNodeApiUserEvictRequest>,
    ) -> Result<LxJson<Empty>, MegaApiError> {
        // Sanity check
        if req.mega_id != state.mega_id {
            return Err(MegaApiError::wrong_mega_id(
                &req.mega_id,
                &state.mega_id,
            ));
        }

        let (user_shutdown_tx, user_shutdown_rx) = oneshot::channel();
        let req_with_tx = UserRunnerUserEvictRequest {
            inner: req,
            user_shutdown_waiter: user_shutdown_tx,
        };
        let cmd = UserRunnerCommand::UserEvictRequest(req_with_tx);
        state.runner_tx.try_send(cmd).map_err(|e| MegaApiError {
            kind: MegaErrorKind::RunnerUnreachable,
            msg: e.to_string(),
            ..Default::default()
        })?;

        let _user_shutdown =
            user_shutdown_rx.await.map_err(|e| MegaApiError {
                kind: MegaErrorKind::RunnerUnreachable,
                msg: e.to_string(),
                ..Default::default()
            })??;

        Ok(LxJson(Empty {}))
    }
}
