use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, ensure, Context};
use arc_swap::ArcSwap;
use common::{
    cli::LspInfo, enclave, env::DeployEnv, ln::network::LxNetwork, rng::Crng,
};
use lexe_api::{
    def::NodeLspApi,
    error::MegaApiError,
    types::{ports::RunPorts, LeaseId},
};
use lexe_ln::{
    alias::{
        EsploraSyncClientType, NetworkGraphType, P2PGossipSyncType,
        ProbabilisticScorerType,
    },
    esplora::{self, FeeEstimates, LexeEsplora},
    logger::LexeTracingLogger,
};
use lexe_std::Apply;
use lexe_tls::attestation::NodeMode;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use lightning::{
    routing::gossip::P2PGossipSync,
    util::{config::UserConfig, ser::ReadableArgs},
};
use lightning_transaction_sync::EsploraSyncClient;
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::{
    api::{self, NodeBackendApiClient, RunnerApiClient},
    DEV_VERSION, SEMVER_VERSION,
};

/// Usernode-specific context.
/// May be initialized by the meganode or by the usernode itself.
// TODO(max): Eventually, this will only be initialized by the meganode.
pub(crate) struct UserContext {
    /// The lease ID for this user node, if any.
    // TODO(claude): Remove the Option once we remove the run variant.
    pub lease_id: Option<LeaseId>,
    /// A channel for requests to get the [`RunPorts`] of this user node.
    pub user_ready_waiter_rx:
        mpsc::Receiver<oneshot::Sender<Result<RunPorts, MegaApiError>>>,
    /// Notifies this specific usernode that it should shut down.
    pub user_shutdown: NotifyOnce,
}

// TODO(max): This can be removed once `run` is removed.
impl Default for UserContext {
    fn default() -> Self {
        Self {
            lease_id: None,
            user_shutdown: NotifyOnce::new(),
            user_ready_waiter_rx: mpsc::channel(
                lexe_tokio::DEFAULT_CHANNEL_SIZE,
            )
            .1,
        }
    }
}

/// Run context shared between all usernodes running on this meganode.
#[derive(Clone)]
pub(crate) struct MegaContext {
    /// The backend API client for user nodes.
    pub backend_api: Arc<dyn NodeBackendApiClient + Send + Sync>,
    /// The channel config for user nodes.
    pub config: Arc<ArcSwap<UserConfig>>,
    /// The Esplora client for blockchain data.
    pub esplora: Arc<LexeEsplora>,
    /// On-chain fee estimates, periodically updated by [`LexeEsplora`].
    pub fee_estimates: Arc<FeeEstimates>,
    /// The P2P gossip sync for network graph updates.
    pub gossip_sync: Arc<P2PGossipSyncType>,
    /// The LDK transaction sync client for blockchain synchronization.
    pub ldk_sync_client: Arc<EsploraSyncClientType>,
    /// The logger for user nodes.
    pub logger: LexeTracingLogger,
    /// The LSP API client for user nodes.
    pub lsp_api: Arc<dyn NodeLspApi + Send + Sync>,
    /// The machine ID of the enclave.
    pub machine_id: enclave::MachineId,
    /// The measurement of the enclave.
    pub measurement: enclave::Measurement,
    /// The Lightning Network graph for routing.
    pub network_graph: Arc<NetworkGraphType>,
    /// The runner API client for user nodes.
    pub runner_api: Arc<dyn RunnerApiClient + Send + Sync>,
    /// The probabilistic scorer for pathfinding.
    pub scorer: Arc<Mutex<ProbabilisticScorerType>>,
    /// The untrusted deploy environment.
    pub untrusted_deploy_env: DeployEnv,
    /// The untrusted network.
    pub untrusted_network: LxNetwork,
    /// The semantic version of the node.
    pub version: semver::Version,
}

impl MegaContext {
    const NODE_MODE: NodeMode = NodeMode::Run;

    /// Creates a [`MegaContext`]; also returns spawned `static_tasks`.
    ///
    /// TODO(max): The returned `static_tasks` are expected to shutdown after
    /// `user_or_mega_shutdown` is notified. Once we are meganode only, it no
    /// longer needs to be passed into `UserNode::init` for the user node to
    /// await on; they will be awaited on by the meganode itself.
    pub async fn init(
        rng: &mut impl Crng,
        // TODO(max): This can be removed once the `run` command is removed.
        allow_mock: bool,
        backend_url: Option<String>,
        lsp: LspInfo,
        runner_url: Option<String>,
        untrusted_deploy_env: DeployEnv,
        untrusted_esplora_urls: Vec<String>,
        untrusted_network: LxNetwork,
        // - If this MegaContext was created for the entire meganode, this
        //   should contain the shutdown channel for the meganode.
        // - If this MegaContext was created just for a usernode, this should
        //   contain the shutdown channel for that usernode.
        // TODO(max): This can be removed once the `run` command is removed;
        // at that point, it would exclusively use `mega_shutdown`.
        user_or_mega_shutdown: NotifyOnce,
    ) -> anyhow::Result<(Self, Vec<LxTask<()>>)> {
        let logger = LexeTracingLogger::new();

        let machine_id = enclave::machine_id();
        let measurement = enclave::measurement();
        // TODO(phlip9): Compare this with current cpusvn
        let _min_cpusvn = enclave::MinCpusvn::CURRENT;

        let config = crate::channel_manager::get_config();

        let backend_api = api::new_backend_api(
            rng,
            allow_mock,
            untrusted_deploy_env,
            Self::NODE_MODE,
            backend_url,
        )?;
        let runner_api = api::new_runner_api(
            rng,
            allow_mock,
            untrusted_deploy_env,
            Self::NODE_MODE,
            runner_url,
        )?;
        let lsp_api = api::new_lsp_api(
            rng,
            allow_mock,
            untrusted_deploy_env,
            untrusted_network,
            Self::NODE_MODE,
            lsp.node_api_url.clone(),
            logger.clone(),
        )?;

        let mut static_tasks = Vec::with_capacity(20);

        // Only accept esplora urls whitelisted in the given `network`.
        let esplora_urls = untrusted_esplora_urls
            .iter()
            .filter(|url| esplora::url_is_whitelisted(url, untrusted_network))
            .cloned()
            .collect::<Vec<String>>();
        ensure!(
            !esplora_urls.is_empty(),
            "None of the provided esplora urls were in whitelist: {urls:?}",
            urls = &untrusted_esplora_urls,
        );

        // Version
        let version = DEV_VERSION
            .unwrap_or(SEMVER_VERSION)
            .apply(semver::Version::parse)
            .expect("Checked in tests");

        // Initialize esplora, network graph, and scorer concurrently
        #[rustfmt::skip] // Does not respect 80 char line width
        let (try_esplora_init, try_network_graph_bytes, try_scorer_bytes) =
            tokio::join!(
                LexeEsplora::init_any(
                    api::USER_AGENT_EXTERNAL,
                    rng,
                    esplora_urls,
                    user_or_mega_shutdown.clone(),
                ),
                lsp_api.get_network_graph(),
                lsp_api.get_prob_scorer(),
            );

        // Handle esplora initialization result
        let (esplora, fee_estimates, refresh_fees_task, esplora_url) =
            try_esplora_init.context("Failed to init esplora")?;
        info!(%esplora_url);
        static_tasks.push(refresh_fees_task);

        // Init LDK transaction sync client; share LexeEsplora's connection pool
        let ldk_sync_client = Arc::new(EsploraSyncClient::from_client(
            esplora.client().clone(),
            logger.clone(),
        ));

        // Initialize network graph
        let network_graph = {
            let network_graph_bytes = try_network_graph_bytes
                .context("Could not fetch serialized network graph")?;
            let mut reader = Cursor::new(&network_graph_bytes);
            let read_args = logger.clone();
            NetworkGraphType::read(&mut reader, read_args)
                .map(Arc::new)
                .map_err(|e| anyhow!("Couldn't deser network graph: {e:#}"))?
        };

        // Initialize scorer
        let scorer = {
            let scorer_bytes = try_scorer_bytes
                .context("Could not fetch serialized scorer")?;
            let decay_params = lexe_ln::constants::LEXE_SCORER_PARAMS;
            let read_args =
                (decay_params, network_graph.clone(), logger.clone());
            let mut reader = Cursor::new(&scorer_bytes);
            ProbabilisticScorerType::read(&mut reader, read_args)
                .map(Mutex::new)
                .map(Arc::new)
                .map_err(|e| anyhow!("Couldn't deser prob scorer: {e:#}"))?
        };

        // Initialize gossip sync
        // TODO(phlip9): does node even need gossip sync anymore?
        let utxo_lookup = None;
        let gossip_sync = Arc::new(P2PGossipSync::new(
            network_graph.clone(),
            utxo_lookup,
            logger.clone(),
        ));

        let context = Self {
            backend_api,
            config,
            esplora,
            fee_estimates,
            gossip_sync,
            ldk_sync_client,
            logger,
            lsp_api,
            machine_id,
            measurement,
            network_graph,
            runner_api,
            scorer,
            untrusted_deploy_env,
            untrusted_network,
            version,
        };

        Ok((context, static_tasks))
    }
}
