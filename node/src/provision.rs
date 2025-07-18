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
use axum::{routing::post, Router};
use common::{
    api::provision::NodeProvisionRequest,
    cli::{node::MegaArgs, OAuthConfig},
    constants, enclave,
    env::DeployEnv,
    ln::network::LxNetwork,
    net,
    rng::{Crng, SysRng},
};
use gdrive::GoogleVfs;
use lexe_api::{
    auth::BearerAuthenticator,
    def::NodeBackendApi,
    error::NodeApiError,
    server::{self, LayerConfig},
    types::{ports::ProvisionPorts, sealed_seed::SealedSeed, Empty},
};
use lexe_tls::attestation::{self, NodeMode};
use lexe_tokio::{
    notify_once::NotifyOnce,
    task::{self, LxTask},
};
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span};

use crate::{client::NodeBackendClient, context::MegaContext, persister};

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
            client,
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
                Some((Arc::new(app_tls_config), app_dns.as_str())),
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
struct AppRouterState {
    args: Arc<ProvisionArgs>,
    backend_api: Arc<NodeBackendClient>,
    client: gdrive::ReqwestClient,
    machine_id: enclave::MachineId,
    measurement: enclave::Measurement,
    // TODO(phlip9): make generic, use test rng in test
    rng: SysRng,
    untrusted_deploy_env: DeployEnv,
    untrusted_network: LxNetwork,
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

        // If we're in staging/prod, we need to handle gDrive. We'll save the
        // gDrive credentials (so we can get access after the token expires)
        // and potentially init the GVFS setup (make directories, etc...).
        if req.deploy_env.is_staging_or_prod() {
            // If we're given an auth_code, complete the OAuth2 flow and persist
            // the gDrive credentials.
            let vfs_master_key = req.root_seed.derive_vfs_master_key();
            let credentials = helpers::provision_gdrive_credentials(
                &mut state,
                req.google_auth_code.as_deref(),
                &authenticator,
                &vfs_master_key,
            )
            .await?;

            // If we're allowed to provision GVFS, init the GVFS structure if
            // it's not already init'ed.
            if req.allow_gvfs_access {
                helpers::provision_gvfs(
                    &mut state,
                    &req,
                    &authenticator,
                    credentials,
                    &vfs_master_key,
                    &user_pk,
                )
                .await?;
            } else if req.encrypted_seed.is_some() {
                // It is a usage error if they also provided a pw-encrypted seed
                // but we are not allowed to provision GVFS.
                return Err(NodeApiError::provision(
                    "A root seed backup was provided, but it cannot be \
                     persisted because `allow_gvfs_access=false`",
                ));
            }
        }

        // Finally, store the sealed seed and new node metadata in the backend.
        //
        // We do this last so that a new user signup where gdrive fails to
        // provision (which seems to happen more often than I'd like) doesn't
        // fill up our capacity with broken nodes that can't run.
        state
            .backend_api
            .create_sealed_seed(&sealed_seed, token)
            .await
            .context("Could not persist sealed seed")
            .map_err(NodeApiError::provision)?;

        Ok(LxJson(Empty {}))
    }
}

mod helpers {
    use common::{aes::AesMasterKey, api::user::UserPk};
    use gdrive::{gvfs::GvfsRootName, oauth2::GDriveCredentials};
    use lexe_api::error::{BackendApiError, BackendErrorKind};
    use tracing::warn;

    use super::*;
    use crate::approved_versions::ApprovedVersions;

    /// If we're given a gDrive auth_code (e.g., the user is signing up for the
    /// first time), then we'll complete the OAuth2 flow to get an access_token
    /// and refresh_token. We'll want to persist these credentials to Lexe infra
    /// (encrypted ofc) so this node can get access_token's again in the future.
    ///
    /// Otherwise, we'll just sanity check the already persisted credentials.
    pub(super) async fn provision_gdrive_credentials(
        state: &mut AppRouterState,
        google_auth_code: Option<&str>,
        authenticator: &BearerAuthenticator,
        vfs_master_key: &AesMasterKey,
    ) -> Result<GDriveCredentials, NodeApiError> {
        let oauth = state
            .args
            .oauth
            .clone()
            .context("Missing OAuthConfig from Lexe operators")
            .map_err(NodeApiError::provision)?;
        let credentials = match google_auth_code {
            Some(code) => {
                // We were given an auth code. Exchange for credentials and
                // persist.

                // Use the auth code to get a GDriveCredentials.
                let code_verifier = None;
                let credentials = gdrive::oauth2::auth_code_for_token(
                    &state.client,
                    &oauth.client_id,
                    Some(&oauth.client_secret),
                    &oauth.redirect_uri,
                    code,
                    code_verifier,
                )
                .await
                .context("Couldn't get tokens using code")
                .map_err(NodeApiError::provision)?;

                // Encrypt the GDriveCredentials and upsert into Lexe's DB.
                let credentials_file = persister::encrypt_gdrive_credentials(
                    &mut state.rng,
                    vfs_master_key,
                    &credentials,
                );
                persister::persist_file(
                    state.backend_api.as_ref(),
                    authenticator,
                    &credentials_file,
                )
                .await
                .context("Could not persist new GDrive credentials")
                .map_err(NodeApiError::provision)?;

                credentials
            }
            None => {
                // No auth code was provided. Ensure that credentials already
                // exist.
                let credentials = persister::read_gdrive_credentials(
                    state.backend_api.as_ref(),
                    authenticator,
                    vfs_master_key,
                )
                .await
                .context("GDriveCredentials invalid or missing")
                .map_err(NodeApiError::provision)?;

                // Sanity check the returned credentials
                if oauth.client_id != credentials.client_id {
                    return Err(NodeApiError::provision(
                        "`client_id`s didn't match!",
                    ));
                }
                if Some(oauth.client_secret) != credentials.client_secret {
                    return Err(NodeApiError::provision(
                        "`client_secret`s didn't match!",
                    ));
                }

                credentials
            }
        };
        Ok(credentials)
    }

    /// Set up the "Google VFS" in the user's GDrive.
    ///
    /// - Creates the GVFS file folder structure (if it didn't exist)
    /// - Backup up the encrypted root seed (if it didn't exist)
    /// - Updates the approved versions list for rollback protection
    pub(super) async fn provision_gvfs(
        state: &mut AppRouterState,
        req: &NodeProvisionRequest,
        authenticator: &BearerAuthenticator,
        credentials: GDriveCredentials,
        vfs_master_key: &AesMasterKey,
        user_pk: &UserPk,
    ) -> Result<(), NodeApiError> {
        // See if we have a persisted gvfs root.
        let maybe_persisted_gvfs_root = persister::read_gvfs_root(
            &state.backend_api,
            authenticator,
            vfs_master_key,
        )
        .await
        .context("Failed to fetch persisted gvfs root")
        .map_err(NodeApiError::provision)?;

        // Init the GVFS. This makes ~one API call to populate the cache.
        let gvfs_root_name = GvfsRootName {
            deploy_env: req.deploy_env,
            network: req.network,
            use_sgx: cfg!(target_env = "sgx"),
            user_pk: *user_pk,
        };
        let (google_vfs, maybe_new_gvfs_root, mut credentials_rx) =
            GoogleVfs::init(
                credentials,
                gvfs_root_name,
                maybe_persisted_gvfs_root,
            )
            .await
            .context("Failed to init Google VFS")
            .map_err(NodeApiError::provision)?;

        // Do the GVFS operations in an async closure so we have a chance to
        // update the GDriveCredentials in Lexe's DB regardless of Ok/Err.
        let do_gvfs_ops = async {
            // If we were given a new GVFS root to persist, persist it.
            // This should only happen once.
            if let Some(new_gvfs_root) = maybe_new_gvfs_root {
                persister::persist_gvfs_root(
                    &mut state.rng,
                    &state.backend_api,
                    authenticator,
                    vfs_master_key,
                    &new_gvfs_root,
                )
                .await
                .context("Failed to persist new gvfs root")
                .map_err(NodeApiError::provision)?;
            }

            // See if a root seed backup already exists. This does not check
            // whether the backup is well-formed, matches the current seed, etc.
            let backup_exists =
                persister::password_encrypted_root_seed_exists(&google_vfs)
                    .await;

            // If no backup exists in GDrive, we should create one, or error if
            // no pw-encrypted root seed was provided.
            if !backup_exists {
                let encrypted_seed = req
                    .encrypted_seed
                    .clone()
                    .context(
                        "Missing pw-encrypted root seed backup in GDrive; \
                        please provide one in another provision request",
                    )
                    .map_err(NodeApiError::provision)?;

                persister::persist_password_encrypted_root_seed(
                    &google_vfs,
                    encrypted_seed,
                )
                .await
                .context("Failed to persist encrypted root seed")
                .map_err(NodeApiError::provision)?;
            }

            // Fetch the approved versions list or create an empty one.
            let mut approved_versions =
                persister::read_approved_versions(&google_vfs, vfs_master_key)
                    .await
                    .context("Couldn't read approved versions")
                    .map_err(NodeApiError::provision)?
                    .unwrap_or_else(ApprovedVersions::new);

            // Approve the current version, revoke old/yanked versions, etc.
            let (updated, revoked) = approved_versions
                .approve_and_revoke(user_pk, state.measurement)
                .context("Error updating approved versions")
                .map_err(NodeApiError::provision)?;

            // If the list was updated, we need to (re)persist it.
            if updated {
                persister::persist_approved_versions(
                    &mut state.rng,
                    &google_vfs,
                    vfs_master_key,
                    &approved_versions,
                )
                .await
                .context("Persist approved versions failed")
                .map_err(NodeApiError::provision)?;
            }

            // If any versions were revoked, delete their sealed seeds.
            if !revoked.is_empty() {
                // Ok to delete serially bc usually there's only 1
                for (revoked_version, revoked_measurement) in revoked {
                    let token = authenticator
                        .get_token(
                            state.backend_api.as_ref(),
                            SystemTime::now(),
                        )
                        .await
                        .map_err(NodeApiError::bad_auth)?;
                    let try_delete = state
                        .backend_api
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
                        Err(e) =>
                            return Err(NodeApiError::provision(format!(
                                "Error deleting revoked sealed seeds: {e:#}"
                            ))),
                    }
                }
            }

            Ok::<(), NodeApiError>(())
        };
        let try_gvfs_ops = do_gvfs_ops.await;

        // If the GDriveCredentials were updated during the calls above, persist
        // the updated credentials so we can avoid a unnecessary refresh.
        let try_update_credentials =
            if matches!(credentials_rx.has_changed(), Ok(true)) {
                let credentials_file = persister::encrypt_gdrive_credentials(
                    &mut state.rng,
                    vfs_master_key,
                    &credentials_rx.borrow_and_update(),
                );

                persister::persist_file(
                    state.backend_api.as_ref(),
                    authenticator,
                    &credentials_file,
                )
                .await
                .context("Could not persist updated GDrive credentials")
                .map_err(NodeApiError::provision)
            } else {
                Ok(())
            };

        // Finally done. Return the first of any errors.
        try_gvfs_ops.and(try_update_credentials)
    }
}
