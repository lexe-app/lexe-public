//! # Provisioning Lexe user nodes
//!
//! This module is responsible for running the node provisioning process for new
//! users and for existing users upgrading to new enclave versions.
//!
//! The intention of the provisioning process is for users to transfer their
//! secure secrets into a trusted enclave version without the operator (Lexe)
//! learning their secrets. These secrets include sensitive data like wallet
//! private keys or mTLS certificates.
//!
//! A node enclave must also convince the user that the software is a version
//! that they trust and the software is running inside an up-to-date secure
//! enclave. We do this using a variant of RA-TLS (Remote Attestation TLS),
//! where the enclave platform endorsements and enclave measurements are bundled
//! into a self-signed TLS certificate, which users must verify when connecting
//! to the provisioning endpoint.

use std::{net::TcpListener, sync::Arc, time::SystemTime};

use anyhow::Context;
use axum::{Router, routing::post};
use common::{
    api::provision::NodeProvisionRequest,
    cli::{OAuthConfig, node::MegaArgs},
    constants, enclave,
    env::DeployEnv,
    ln::network::LxNetwork,
    net,
    rng::{Crng, SysRng},
};
use lexe_api::{
    auth::BearerAuthenticator,
    def::NodeBackendApi,
    error::NodeApiError,
    server::{self, LayerConfig},
    types::{Empty, ports::ProvisionPorts, sealed_seed::SealedSeed},
};
use lexe_tls::attestation::{self, NodeMode};
use lexe_tokio::{
    notify_once::NotifyOnce,
    task::{self, LxTask},
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span};

use crate::{
    client::NodeBackendClient, context::MegaContext, gdrive_provision,
    persister,
};

/// Args needed by the [`ProvisionInstance`].
/// These are built from [`MegaArgs`] which is passed in via CLI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProvisionArgs {
    /// protocol://host:port of the backend.
    pub backend_url: String,

    /// protocol://host:port of the runner.
    pub runner_url: String,

    /// configuration info for Google OAuth2.
    /// Required only if running in staging / prod.
    pub oauth: Option<OAuthConfig>,

    /// The current deploy environment passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_deploy_env: DeployEnv,

    /// The current deploy network passed to us by Lexe (or someone in
    /// Lexe's cloud). This input should be treated as untrusted.
    pub untrusted_network: LxNetwork,
}

impl From<&MegaArgs> for ProvisionArgs {
    fn from(args: &MegaArgs) -> Self {
        Self {
            backend_url: args.backend_url.clone(),
            runner_url: args.runner_url.clone(),
            oauth: args.oauth.clone(),
            untrusted_deploy_env: args.untrusted_deploy_env,
            untrusted_network: args.untrusted_network,
        }
    }
}

pub(crate) struct ProvisionInstance {
    static_tasks: Vec<LxTask<()>>,
    #[allow(dead_code)]
    ports: ProvisionPorts,
    shutdown: NotifyOnce,
}

impl ProvisionInstance {
    pub async fn init(
        rng: &mut impl Crng,
        args: ProvisionArgs,
        mega_ctx: MegaContext,
        shutdown: NotifyOnce,
    ) -> anyhow::Result<Self> {
        info!("Initializing provision service");

        // Create a backend client for provisioning
        let measurement = mega_ctx.measurement;
        let mr_short = measurement.short();
        let node_mode = NodeMode::Provision { mr_short };
        let backend_client = NodeBackendClient::new(
            rng,
            mega_ctx.untrusted_deploy_env,
            node_mode,
            args.backend_url.clone(),
        )
        .context("Failed to init BackendClient")?;

        // Set up the request context and API servers.
        let args = Arc::new(args);
        let client = gdrive::ReqwestClient::new();
        let state = AppRouterState {
            args: args.clone(),
            backend_api: Arc::new(backend_client),
            gdrive_client: client,
            machine_id: mega_ctx.machine_id,
            measurement,
            // TODO(phlip9): use passed in rng
            rng: SysRng::new(),
            untrusted_deploy_env: mega_ctx.untrusted_deploy_env,
            untrusted_network: mega_ctx.untrusted_network,
        };

        const APP_SERVER_SPAN_NAME: &str = "(app-node-provision-server)";
        let app_listener =
            TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
                .context("Failed to bind app listener")?;
        let app_port = app_listener
            .local_addr()
            .context("Couldn't get app addr")?
            .port();
        let (app_tls_config, app_dns) =
            attestation::app_node_provision_server_config(rng, &measurement)
                .context("Failed to build TLS config for provisioning")?;
        let (app_server_task, _app_url) =
            server::spawn_server_task_with_listener(
                app_listener,
                app_router(state),
                LayerConfig::default(),
                Some((Arc::new(app_tls_config), &app_dns)),
                APP_SERVER_SPAN_NAME.into(),
                info_span!(APP_SERVER_SPAN_NAME),
                shutdown.clone(),
            )
            .context("Failed to spawn app node provision server task")?;

        let static_tasks = vec![app_server_task];

        // Notify the runner that we're ready for a client connection
        let ports = ProvisionPorts {
            measurement,
            app_port,
        };

        Ok(Self {
            static_tasks,
            ports,
            shutdown,
        })
    }

    async fn run(self) -> anyhow::Result<()> {
        // Wait for API servers to recv shutdown signal and gracefully shut down
        let (_, eph_tasks_rx) = mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);
        task::try_join_tasks_and_shutdown(
            self.static_tasks,
            eph_tasks_rx,
            self.shutdown,
            constants::USER_NODE_SHUTDOWN_TIMEOUT,
        )
        .await
        .context("Error awaiting tasks")
    }

    pub fn ports(&self) -> ProvisionPorts {
        self.ports
    }

    pub fn spawn_into_task(self) -> LxTask<()> {
        const SPAN_NAME: &str = "(provision)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            match self.run().await {
                Ok(()) => info!("Provision instance finished."),
                Err(e) => error!("Provision instance errored: {e:#}"),
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct AppRouterState {
    args: Arc<ProvisionArgs>,
    backend_api: Arc<NodeBackendClient>,
    gdrive_client: gdrive::ReqwestClient,
    machine_id: enclave::MachineId,
    measurement: enclave::Measurement,
    // TODO(phlip9): make generic, use test rng in test
    rng: SysRng,
    untrusted_deploy_env: DeployEnv,
    untrusted_network: LxNetwork,
}

impl AppRouterState {
    pub(crate) fn backend_api(&self) -> &NodeBackendClient {
        self.backend_api.as_ref()
    }
}

/// Implements [`AppNodeProvisionApi`] - only callable by the node owner.
///
/// [`AppNodeProvisionApi`]: lexe_api::def::AppNodeProvisionApi
fn app_router(state: AppRouterState) -> Router<()> {
    Router::new()
        .route("/app/provision", post(handlers::provision))
        .with_state(state)
}

/// API handlers.
mod handlers {
    use axum::extract::State;
    use common::api::user::UserPk;
    use gdrive::gvfs::GvfsRootName;
    use lexe_api::server::LxJson;

    use super::*;

    pub(super) async fn provision(
        State(mut state): State<AppRouterState>,
        LxJson(req): LxJson<NodeProvisionRequest>,
    ) -> Result<LxJson<Empty>, NodeApiError> {
        debug!("Received provision request");

        // Sanity check with no meaningful security; an attacker with cloud
        // access can still set the deploy env or network to whatever they need.
        if state.untrusted_deploy_env != req.deploy_env
            || state.untrusted_network != req.network
        {
            let req_env = req.deploy_env;
            let req_net = req.network;
            let ctx_env = state.untrusted_deploy_env;
            let ctx_net = state.untrusted_network;
            return Err(NodeApiError::provision(format!(
                "Probable configuration error, client and node don't agree on current env: \
                 client: ({req_env}, {req_net}), node: ({ctx_env}, {ctx_net})"
            )));
        }

        let sealed_seed = SealedSeed::seal_from_root_seed(
            &mut state.rng,
            &req.root_seed,
            req.deploy_env,
            req.network,
            state.measurement,
            state.machine_id,
        )
        .map_err(NodeApiError::provision)?;

        // TODO(phlip9): [perf] could get the user to pass us their auth token
        // in the provision request instead of reauthing here.

        // Authenticate as the user to the backend.
        //
        // We do this before gDrive provisioning to ensure the user is a real &
        // valid Lexe user before taxing our gDrive API quotas.
        let user_key_pair = req.root_seed.derive_user_key_pair();
        let user_pk = UserPk::new(user_key_pair.public_key().into_inner());
        let maybe_token = None;
        let authenticator =
            BearerAuthenticator::new(user_key_pair, maybe_token);
        let token = authenticator
            .get_token(state.backend_api.as_ref(), SystemTime::now())
            .await
            .map_err(NodeApiError::bad_auth)?;

        // Store the sealed seed and new node metadata in the backend.
        state
            .backend_api
            .create_sealed_seed(&sealed_seed, token)
            .await
            .context("Could not persist sealed seed")
            .map_err(NodeApiError::provision)?;

        if req.deploy_env.is_dev() {
            // If we're in dev, we don't need to provision GDrive credentials
            // or set up the GVFS.
            return Ok(LxJson(Empty {}));
        }
        // From here, we're in staging or prod.

        // Update approved versions
        let vfs_master_key = req.root_seed.derive_vfs_master_key();
        helpers::update_approved_versions(
            &mut state.rng,
            &state.backend_api,
            &authenticator,
            &vfs_master_key,
            &user_pk,
            state.measurement,
        )
        .await?;

        let oauth = state.args.oauth.as_ref().ok_or_else(|| {
            NodeApiError::provision("OAuthConfig required in staging/prod")
        })?;

        // Handle GDrive credentials.
        let credentials = match req.google_auth_code.as_deref() {
            // If we were given an auth_code, complete the OAuth2 flow and
            // persist the freshly minted gDrive credentials.
            Some(code) =>
                gdrive_provision::exchange_code_and_persist_credentials(
                    &mut state.rng,
                    &state.backend_api,
                    &state.gdrive_client,
                    oauth,
                    code,
                    &authenticator,
                    &vfs_master_key,
                )
                .await?,
            None => {
                // No auth code. Try to read GDrive credentials from Lexe's DB.
                let maybe_credentials =
                    gdrive_provision::maybe_read_and_validate_credentials(
                        &state,
                        oauth,
                        &authenticator,
                        &vfs_master_key,
                    )
                    .await?;

                match maybe_credentials {
                    Some(creds) => creds,
                    // No GDrive credentials were found in Lexe's DB;
                    // GDrive is not enabled for this user. Nothing left to do.
                    None => return Ok(LxJson(Empty {})),
                }
            }
        };

        // If the client provided a pw-encrypted seed to backup, but we are not
        // allowed to access GVFS, this is a usage error.
        if !req.allow_gvfs_access && req.encrypted_seed.is_some() {
            return Err(NodeApiError::provision(
                "A root seed backup was provided, but it cannot be \
                 persisted because `allow_gvfs_access=false`",
            ));
        }

        // If we're not allowed to access GVFS, there is nothing more to do.
        if !req.allow_gvfs_access {
            return Ok(LxJson(Empty {}));
        }

        let gvfs_root_name = GvfsRootName {
            deploy_env: req.deploy_env,
            network: req.network,
            use_sgx: cfg!(target_env = "sgx"),
            user_pk,
        };
        // Init the GVFS structure if it's not already initialized.
        gdrive_provision::setup_gvfs_and_persist_seed(
            req.encrypted_seed,
            gvfs_root_name,
            &state.backend_api,
            &mut state.rng,
            &authenticator,
            credentials,
            &vfs_master_key,
        )
        .await
        .map_err(NodeApiError::provision)?;

        Ok(LxJson(Empty {}))
    }
}

mod helpers {
    use common::{aes::AesMasterKey, api::user::UserPk, enclave::Measurement};
    use lexe_api::error::{BackendApiError, BackendErrorKind};
    use tracing::warn;

    use super::*;
    use crate::approved_versions::ApprovedVersions;

    /// Update [`ApprovedVersions`]:
    /// - Fetch the approved versions list from Lexe's DB (or create a new one)
    /// - Approve the current version
    /// - Revoke old/yanked versions based on a rolling window
    /// - Re-persist if updated
    /// - Delete sealed seeds for revoked versions.
    pub(super) async fn update_approved_versions(
        rng: &mut impl Crng,
        backend_api: &NodeBackendClient,
        authenticator: &BearerAuthenticator,
        vfs_master_key: &AesMasterKey,
        user_pk: &UserPk,
        measurement: Measurement,
    ) -> Result<(), NodeApiError> {
        // Fetch the approved versions list or create an empty one.
        let mut approved_versions = persister::read_approved_versions(
            backend_api,
            authenticator,
            vfs_master_key,
        )
        .await
        .context("Couldn't read approved versions")
        .map_err(NodeApiError::provision)?
        .unwrap_or_else(ApprovedVersions::new);

        // Approve the current version, revoke old/yanked versions, etc.
        let (updated, revoked) = approved_versions
            .approve_and_revoke(user_pk, measurement)
            .context("Error updating approved versions")
            .map_err(NodeApiError::provision)?;

        // If the list was updated, we need to (re)persist it.
        if updated {
            persister::persist_approved_versions(
                rng,
                backend_api,
                authenticator,
                vfs_master_key,
                &approved_versions,
            )
            .await
            .context("Persist approved versions failed")
            .map_err(NodeApiError::provision)?;
        }

        // If any versions were revoked, delete their sealed seeds.
        // Ok to delete serially bc usually there's only 1
        for (revoked_version, revoked_measurement) in revoked {
            let token = authenticator
                .get_token(backend_api, SystemTime::now())
                .await
                .map_err(NodeApiError::bad_auth)?;
            let try_delete = backend_api
                .delete_sealed_seeds(revoked_measurement, token.clone())
                .await;

            match try_delete {
                Ok(_) => info!(
                    %user_pk, %revoked_version, %revoked_measurement,
                    "Deleted revoked sealed seed"
                ),
                Err(BackendApiError {
                    kind: BackendErrorKind::NotFound,
                    msg,
                    ..
                }) => warn!(
                    %user_pk, %revoked_version, %revoked_measurement,
                    "Failed to delete revoked sealed seeds: \
                     revoked measurement wasn't found in DB: {msg}"
                ),
                Err(e) => {
                    return Err(NodeApiError::provision(format!(
                        "Error deleting revoked sealed seeds: {e:#}"
                    )));
                }
            }
        }

        Ok(())
    }
}
