use anyhow::Context;
use common::{
    cli::node::{MegaArgs, ProvisionArgs},
    constants,
    rng::Crng,
};
use lexe_tokio::{notify_once::NotifyOnce, task};
use tokio::sync::mpsc;

use crate::provision::ProvisionInstance;

pub async fn run(rng: &mut impl Crng, args: MegaArgs) -> anyhow::Result<()> {
    let mut static_tasks = Vec::with_capacity(5);

    let (eph_tasks_tx, eph_tasks_rx) =
        mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);

    // TODO(max): User node tasks should be spawned into this.
    let _ = eph_tasks_tx;

    // Shutdown channel for the entire mega instance.
    let mega_shutdown = NotifyOnce::new();

    // Init the provision service. Since it's a static service that should
    // live as long as the mega node itself, we can reuse the mega_shutdown.
    let provision_args = ProvisionArgs::from(&args);
    let provision =
        ProvisionInstance::init(rng, provision_args, mega_shutdown.clone())
            .await?;
    static_tasks.push(provision.spawn_into_task());

    // Spawn the mega server task.
    let mega_state = mega_server::MegaRouterState {
        mega_shutdown: mega_shutdown.clone(),
    };
    let (mega_task, mega_port, mega_url) =
        mega_server::spawn_server_task(mega_state)
            .context("Failed to spawn mega server task")?;
    static_tasks.push(mega_task);

    // TODO(max): Send the port to the runner
    let _ = mega_port;
    let _ = mega_url;

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
    use axum::{routing::post, Router};
    use common::net;
    use lexe_api::{server::LayerConfig, types::ports::Port};
    use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
    use tracing::info_span;

    use super::handlers;

    /// Spawns the Lexe mega server task; returns the task, port, and url.
    pub(super) fn spawn_server_task(
        state: MegaRouterState,
    ) -> anyhow::Result<(LxTask<()>, Port, String)> {
        let mega_shutdown = state.mega_shutdown.clone();

        const SERVER_SPAN_NAME: &str = "(mega-server)";
        let mega_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind mega listener")?;
        let mega_port = mega_listener
            .local_addr()
            .context("Couldn't get mega addr")?
            .port();
        let tls_and_dns = None;
        let (task, mega_url) =
            lexe_api::server::spawn_server_task_with_listener(
                mega_listener,
                mega_router(state),
                LayerConfig::default(),
                tls_and_dns,
                Cow::from(SERVER_SPAN_NAME),
                info_span!(SERVER_SPAN_NAME),
                mega_shutdown,
            )
            .context("Failed to spawn Lexe mega server task")?;

        Ok((task, mega_port, mega_url))
    }

    #[derive(Clone)]
    pub(super) struct MegaRouterState {
        pub mega_shutdown: NotifyOnce,
    }

    /// Implements [`LexeMegaApi`] - only callable by the Lexe operators.
    ///
    /// [`LexeMegaApi`]: lexe_api::def::LexeMegaApi
    fn mega_router(state: MegaRouterState) -> Router<()> {
        Router::new()
            .route("/lexe/shutdown", post(handlers::shutdown))
            .with_state(state)
    }
}

/// API handlers.
mod handlers {
    use axum::extract::State;
    use lexe_api::{server::LxJson, types::Empty};

    use super::mega_server::MegaRouterState;

    pub(super) async fn shutdown(
        State(state): State<MegaRouterState>,
    ) -> LxJson<Empty> {
        state.mega_shutdown.send();
        LxJson(Empty {})
    }
}
