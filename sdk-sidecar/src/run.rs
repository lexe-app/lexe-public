use std::{
    borrow::Cow,
    env,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, anyhow};
use common::{env::DeployEnv, ln::network::LxNetwork, rng::SysRng};
use lexe_api::server::LayerConfig;
use lexe_tokio::{
    notify_once::NotifyOnce,
    task::{self, LxTask},
};
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::Credentials,
};
use tracing::{info, info_span, instrument};

use crate::{cli::SidecarArgs, server};

/// `127.0.0.1:5393` We use IPv4 because it's more approachable to newbie devs.
/// The docs note that IPv6 is still supported.
const DEFAULT_LISTEN_ADDR: SocketAddr =
    SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5393));

pub struct Sidecar {
    deploy_env: DeployEnv,
    network: LxNetwork,
    sidecar_url: String,
    static_tasks: Vec<LxTask<()>>,
    shutdown: NotifyOnce,
}

impl Sidecar {
    /// Initialize the [`Sidecar`]
    #[instrument(skip_all, name = "(sidecar)")]
    pub fn init(mut args: SidecarArgs) -> anyhow::Result<Self> {
        // Load credentials from files into args if necessary.
        args.load()?;

        let mut static_tasks = Vec::with_capacity(1);

        // Check user-provided default credentials
        let maybe_credentials = match (args.root_seed, args.client_credentials)
        {
            (Some(root_seed), None) => Some(Credentials::from(root_seed)),
            (None, Some(client_credentials)) =>
                Some(Credentials::from(client_credentials)),
            (Some(_), Some(_)) =>
                return Err(anyhow!(
                    "Can only provide one of: `--root-seed` or `--client-credentials`"
                )),
            // TODO(phlip9): mention root seed options here when we unhide them
            (None, None) => {
                info!(
                    "No credentials configured. \
                     Set credentials via env (LEXE_CLIENT_CREDENTIALS) \
                     or per-request via the Authorization header."
                );
                None
            }
        };

        let listen_addr = args.listen_addr.unwrap_or(DEFAULT_LISTEN_ADDR);
        let deploy_env = args.deploy_env.unwrap_or(DeployEnv::Prod);
        let network = args.network.unwrap_or(LxNetwork::Mainnet);
        info!(%deploy_env, %network);

        let dev_gateway_url = env::var("DEV_GATEWAY_URL").ok().map(Cow::Owned);
        let gateway_url = deploy_env.gateway_url(dev_gateway_url);

        // Create the default node client if default credentials were provided
        let default_client = match maybe_credentials {
            Some(credentials) => {
                let gateway_client = GatewayClient::new(
                    deploy_env,
                    gateway_url.clone(),
                    crate::USER_AGENT,
                )
                .context("Failed to create gateway client")?;

                // does nothing b/c we don't provision
                let use_sgx = true;
                let node_client = NodeClient::new(
                    &mut SysRng::new(),
                    use_sgx,
                    deploy_env,
                    gateway_client,
                    credentials.as_ref(),
                )
                .context("Failed to create node client")?;

                Some(node_client)
            }
            None => None,
        };

        // Spawn HTTP server
        let router_state = Arc::new(server::RouterState::new(
            default_client,
            deploy_env,
            gateway_url,
        ));
        let maybe_tls_and_dns = None;
        const SERVER_SPAN_NAME: &str = "(server)";
        let shutdown = NotifyOnce::new();
        let (server_task, sidecar_url) = lexe_api::server::spawn_server_task(
            listen_addr,
            server::router(router_state),
            LayerConfig::default(),
            maybe_tls_and_dns,
            SERVER_SPAN_NAME.into(),
            info_span!(SERVER_SPAN_NAME),
            shutdown.clone(),
        )
        .context("Failed to spawn server task")?;
        static_tasks.push(server_task);

        Ok(Self {
            deploy_env,
            network,
            sidecar_url,
            static_tasks,
            shutdown,
        })
    }

    /// Get the url of the [`Sidecar`] webserver, e.g. "http://127.0.0.1:5393".
    pub fn url(&self) -> String {
        self.sidecar_url.clone()
    }

    pub fn deploy_env(&self) -> DeployEnv {
        self.deploy_env
    }

    pub fn network(&self) -> LxNetwork {
        self.network
    }

    /// Get a clone of the shutdown channel which can be used to shut down the
    /// [`Sidecar`]. Simply call [`NotifyOnce::send`] on the returned channel.
    pub fn shutdown_channel(&self) -> NotifyOnce {
        self.shutdown.clone()
    }

    /// Runs the [`Sidecar`] until a shutdown signal is received.
    ///
    /// - Set `spawn_ctrlc_handler` to `true` if you'd like the sidecar to
    ///   listen for a Ctrl+C signal to initiate a shutdown.
    ///
    /// Generally, you want to `.await` on this function until it's complete,
    /// but it's also OK to spawn this function call into a task.
    #[instrument(skip_all, name = "(sidecar)")]
    pub async fn run(self, spawn_ctrlc_handler: bool) -> anyhow::Result<()> {
        // Shutdown on CTRL+C
        if spawn_ctrlc_handler {
            LxTask::spawn("ctrlc-handler", {
                let shutdown = self.shutdown.clone();
                async move {
                    use tokio::signal::ctrl_c;

                    info!("Ctrl+C handler ready, press Ctrl+C to shut down.");
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
        }

        // Wait for graceful shutdown (with time limit)
        const SHUTDOWN_TIME_LIMIT: Duration = Duration::from_secs(10);
        let (_eph_tasks_tx, eph_tasks_rx) = tokio::sync::mpsc::channel(1);
        task::try_join_tasks_and_shutdown(
            self.static_tasks,
            eph_tasks_rx,
            self.shutdown,
            SHUTDOWN_TIME_LIMIT,
        )
        .await
        .context("Error awaiting tasks")?;

        Ok(())
    }
}
