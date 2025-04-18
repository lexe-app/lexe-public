use std::{
    env,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use app_rs::client::{GatewayClient, NodeClient};
use common::{
    api::auth::BearerAuthenticator,
    env::DeployEnv,
    ln::network::LxNetwork,
    notify_once::NotifyOnce,
    rng::SysRng,
    root_seed::RootSeed,
    task::{self, LxTask},
};
use lexe_api::server::LayerConfig;
use tracing::{info, info_span, instrument};

use crate::cli::SidecarArgs;

/// The user agent string for internal requests.
static USER_AGENT_INTERNAL: &str = lexe_api::user_agent_internal!();

/// `[::1]:5393`
const DEFAULT_LISTEN_ADDR: SocketAddr =
    SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 5393, 0, 0));

pub struct Sidecar {
    root_seed: RootSeed,
    listen_addr: SocketAddr,
    deploy_env: DeployEnv,
    gateway_url: String,
    _network: LxNetwork,
    shutdown: NotifyOnce,
}

impl Sidecar {
    #[instrument(skip_all, name = "(sdk)")]
    pub fn new(args: SidecarArgs) -> anyhow::Result<Self> {
        let root_seed = args
            .root_seed
            .context("Missing --root_seed / `$ROOT_SEED`")?;

        let listen_addr = args.listen_addr.unwrap_or(DEFAULT_LISTEN_ADDR);
        let deploy_env = args.deploy_env.unwrap_or(DeployEnv::Prod);
        let network = args.network.unwrap_or(LxNetwork::Mainnet);

        // Keep in sync with `app/lib/cfg.dart`.
        // TODO(phlip9): extract
        let gateway_url = match deploy_env {
            DeployEnv::Dev => env::var("DEV_GATEWAY_URL")
                .unwrap_or_else(|_| "https://localhost:4040".to_owned()),
            DeployEnv::Staging =>
                "https://lexe-staging-sgx.uswest2.staging.lexe.app".to_owned(),
            DeployEnv::Prod =>
                "https://lexe-prod.uswest2.prod.lexe.app".to_owned(),
        };

        let shutdown = NotifyOnce::new();

        Ok(Self {
            root_seed,
            listen_addr,
            deploy_env,
            gateway_url,
            _network: network,
            shutdown,
        })
    }

    #[instrument(skip_all, name = "(sdk)")]
    pub async fn run(self) -> anyhow::Result<()> {
        // Shutdown on CTRL+C
        LxTask::spawn("ctrlc-handler", {
            let shutdown = self.shutdown.clone();
            async move {
                use tokio::signal::ctrl_c;
                ctrl_c().await.expect("Error receiving first CTRL+C");
                info!(
                    "CTRL+C received, starting graceful shutdown. \
                 Hit CTRL+C again to quit immediately."
                );
                shutdown.send();
                ctrl_c().await.expect("Error receiving second CTRL+C");
                std::process::exit(1);
            }
        })
        .detach();

        let mut static_tasks = Vec::with_capacity(1);

        // TODO(phlip9): split NodeClient into NodeRunClient and
        // NodeProvisionClient to support root-seed-less sdk.
        let root_seed = self.root_seed;
        let user_key_pair = root_seed.derive_user_key_pair();
        let maybe_auth_token = None;
        let authenticator =
            Arc::new(BearerAuthenticator::new(user_key_pair, maybe_auth_token));

        let gateway_client = GatewayClient::new(
            self.deploy_env,
            self.gateway_url,
            USER_AGENT_INTERNAL,
        )
        .context("Failed to create gateway client")?;

        // does nothing b/c we don't provision
        let use_sgx = true;
        let node_client = NodeClient::new(
            &mut SysRng::new(),
            use_sgx,
            &root_seed,
            self.deploy_env,
            authenticator,
            gateway_client,
        )
        .context("Failed to create node client")?;
        drop(root_seed);

        // Spawn HTTP server
        let router_state = Arc::new(crate::server::RouterState { node_client });
        let maybe_tls_and_dns = None;
        const SERVER_SPAN_NAME: &str = "(server)";
        let (server_task, _server_url) = lexe_api::server::spawn_server_task(
            self.listen_addr,
            crate::server::router(router_state),
            LayerConfig::default(),
            maybe_tls_and_dns,
            SERVER_SPAN_NAME,
            info_span!(SERVER_SPAN_NAME),
            self.shutdown.clone(),
        )
        .context("Failed to spawn server task")?;
        static_tasks.push(server_task);

        // Wait for graceful shutdown (with time limit)
        const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(10);
        let (_eph_tasks_tx, eph_tasks_rx) = tokio::sync::mpsc::channel(1);
        task::try_join_tasks_and_shutdown(
            static_tasks,
            eph_tasks_rx,
            self.shutdown,
            SHUTDOWN_TIME_LIMIT,
        )
        .await
        .context("Error awaiting tasks")?;

        Ok(())
    }
}
