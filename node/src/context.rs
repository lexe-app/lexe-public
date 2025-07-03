use std::{
    io::Cursor,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, ensure, Context};
use arc_swap::ArcSwap;
use common::{
    api::user::UserPk, cli::LspInfo, enclave, env::DeployEnv,
    ln::network::LxNetwork, rng::Crng,
};
use lexe_api::{
    def::NodeLspApi,
    error::MegaApiError,
    types::{ports::RunPorts, LeaseId},
};
use lexe_ln::{
    alias::{NetworkGraphType, P2PGossipSyncType, ProbabilisticScorerType},
    esplora::{self, FeeEstimates, LexeEsplora},
    logger::LexeTracingLogger,
};
use lexe_tls::attestation::NodeMode;
use lexe_tokio::{
    events_bus::EventsBus, notify_once::NotifyOnce, task::LxTask,
};
use lightning::{
    routing::gossip::P2PGossipSync,
    util::{config::UserConfig, ser::ReadableArgs},
};
use tokio::sync::{mpsc, oneshot};
use tracing::info;

use crate::{
    api::{self, BackendApiClient, RunnerApiClient},
    channel_manager,
};

/// Usernode-specific context initialized by the meganode.
pub(crate) struct UserContext {
    /// The lease ID for this user node.
    pub lease_id: LeaseId,
    /// A channel for requests to get the [`RunPorts`] of this user node.
    pub user_ready_waiter_rx:
        mpsc::Receiver<oneshot::Sender<Result<RunPorts, MegaApiError>>>,
    /// Notifies this specific usernode that it should shut down.
    pub user_shutdown: NotifyOnce,
}

/// Run context shared between all usernodes running on this meganode.
#[derive(Clone)]
pub(crate) struct MegaContext {
    /// The backend API client for user nodes.
    pub backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    /// The channel config for user nodes.
    pub config: Arc<ArcSwap<UserConfig>>,
    /// The Esplora client for blockchain data.
    /// NOTE: LexeEsplora can be shared but EsploraSyncClient can't because
    /// EsploraSyncClient holds state internally.
    pub esplora: Arc<LexeEsplora>,
    /// On-chain fee estimates, periodically updated by [`LexeEsplora`].
    pub fee_estimates: Arc<FeeEstimates>,
    /// The P2P gossip sync for network graph updates.
    pub gossip_sync: Arc<P2PGossipSyncType>,
    /// The logger for user nodes.
    pub logger: LexeTracingLogger,
    /// The LSP API client for user nodes.
    pub lsp_api: Arc<dyn NodeLspApi + Send + Sync>,
    /// The machine ID of the enclave.
    pub machine_id: enclave::MachineId,
    /// The measurement of the enclave.
    pub measurement: enclave::Measurement,
    /// Notifies the meganode's runner and inactivity timer of user activity.
    pub mega_activity_bus: EventsBus<UserPk>,
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
    /// The returned `static_tasks` are expected to shutdown after
    /// `mega_shutdown` is notified. They are awaited on by the meganode itself.
    pub async fn init(
        rng: &mut impl Crng,
        backend_url: String,
        lsp: LspInfo,
        runner_url: String,
        untrusted_deploy_env: DeployEnv,
        untrusted_esplora_urls: Vec<String>,
        untrusted_network: LxNetwork,
        mega_shutdown: NotifyOnce,
    ) -> anyhow::Result<(Self, Vec<LxTask<()>>)> {
        let logger = LexeTracingLogger::new();

        let version = crate::version();
        let machine_id = enclave::machine_id();
        let measurement = enclave::measurement();
        // TODO(phlip9): Compare this with current cpusvn
        let _min_cpusvn = enclave::MinCpusvn::CURRENT;

        let config = channel_manager::get_config();

        let backend_api = api::new_backend_api(
            rng,
            untrusted_deploy_env,
            Self::NODE_MODE,
            backend_url,
        )?;
        let runner_api = api::new_runner_api(
            rng,
            untrusted_deploy_env,
            Self::NODE_MODE,
            runner_url,
        )?;
        let lsp_api = api::new_lsp_api(
            rng,
            untrusted_deploy_env,
            Self::NODE_MODE,
            lsp.node_api_url.clone(),
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

        // Initialize esplora, network graph, and scorer concurrently
        #[rustfmt::skip] // Does not respect 80 char line width
        let (try_esplora_init, try_network_graph_bytes, try_scorer_bytes) =
            tokio::join!(
                LexeEsplora::init_any(
                    api::USER_AGENT_EXTERNAL,
                    rng,
                    esplora_urls,
                    mega_shutdown.clone(),
                ),
                lsp_api.get_network_graph(),
                lsp_api.get_prob_scorer(),
            );

        // Handle esplora initialization result
        let (esplora, fee_estimates, refresh_fees_task, esplora_url) =
            try_esplora_init.context("Failed to init esplora")?;
        info!(%esplora_url);
        static_tasks.push(refresh_fees_task);

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

        // Meganode activity channel
        let mega_activity_bus = EventsBus::new();

        let context = Self {
            backend_api,
            config,
            esplora,
            fee_estimates,
            gossip_sync,
            logger,
            lsp_api,
            machine_id,
            measurement,
            mega_activity_bus,
            network_graph,
            runner_api,
            scorer,
            untrusted_deploy_env,
            untrusted_network,
            version,
        };

        Ok((context, static_tasks))
    }

    /// Create a dummy MegaContext for testing purposes.
    /// This creates a minimal context suitable for unit tests that don't need
    /// actual network connectivity. Uses real clients with fake URLs.
    #[cfg(test)]
    #[allow(dead_code)] // TODO(claude): Remove when used in tests
    pub fn dummy() -> Self {
        use std::sync::Mutex;

        use common::{env::DeployEnv, ln::network::LxNetwork, rng::SysRng};
        use lexe_ln::{esplora::LexeEsplora, logger::LexeTracingLogger};
        use lightning::routing::{
            gossip::{NetworkGraph, P2PGossipSync},
            scoring::ProbabilisticScorer,
        };

        let logger = LexeTracingLogger::new();
        let network = LxNetwork::Regtest;
        let deploy_env = DeployEnv::Dev;

        let mut rng = SysRng::new();
        let fake_backend_url = String::new();
        let fake_runner_url = String::new();
        let fake_lsp_url = String::new();

        let backend_api = api::new_backend_api(
            &mut rng,
            deploy_env,
            Self::NODE_MODE,
            fake_backend_url,
        )
        .expect("Should create backend API with fake URL");

        let runner_api = api::new_runner_api(
            &mut rng,
            deploy_env,
            Self::NODE_MODE,
            fake_runner_url,
        )
        .expect("Should create runner API with fake URL");

        let lsp_api = api::new_lsp_api(
            &mut rng,
            deploy_env,
            Self::NODE_MODE,
            fake_lsp_url,
        )
        .expect("Should create LSP API with fake URL");

        // Create dummy esplora and fee estimates
        let esplora = LexeEsplora::dummy();
        let fee_estimates = esplora.fee_estimates();

        // Create empty network graph
        let network_graph =
            Arc::new(NetworkGraph::new(network.to_bitcoin(), logger.clone()));

        // Create empty scorer
        let decay_params = lexe_ln::constants::LEXE_SCORER_PARAMS;
        let scorer = Arc::new(Mutex::new(ProbabilisticScorer::new(
            decay_params,
            network_graph.clone(),
            logger.clone(),
        )));

        // Create gossip sync
        let utxo_lookup = None;
        let gossip_sync = Arc::new(P2PGossipSync::new(
            network_graph.clone(),
            utxo_lookup,
            logger.clone(),
        ));

        // Create other required fields
        let config = channel_manager::get_config();
        let version = crate::version();
        let machine_id = enclave::machine_id();
        let measurement = enclave::measurement();
        let mega_activity_bus = EventsBus::new();

        Self {
            backend_api,
            config,
            esplora,
            fee_estimates,
            gossip_sync,
            logger,
            lsp_api,
            machine_id,
            measurement,
            mega_activity_bus,
            network_graph,
            runner_api,
            scorer,
            untrusted_deploy_env: deploy_env,
            untrusted_network: network,
            version,
        }
    }
}
