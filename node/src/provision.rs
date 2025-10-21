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
use gdrive::GoogleVfs;
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
struct AppRouterState {
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
                helpers::exchange_code_and_persist_credentials(
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
                    helpers::maybe_read_and_validate_credentials(
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

        // Init the GVFS structure if it's not already initialized.
        helpers::setup_gvfs_and_persist_seed(
            &mut state,
            req,
            &authenticator,
            credentials,
            &vfs_master_key,
            &user_pk,
        )
        .await?;

        Ok(LxJson(Empty {}))
    }
}

pub(crate) mod helpers {
    use common::{aes::AesMasterKey, api::user::UserPk, enclave::Measurement};
    use gdrive::{gvfs::GvfsRootName, oauth2::GDriveCredentials};
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
                Err(e) =>
                    return Err(NodeApiError::provision(format!(
                        "Error deleting revoked sealed seeds: {e:#}"
                    ))),
            }
        }

        Ok(())
    }

    /// Completes the OAuth2 flow by exchanging the `auth_code` for an
    /// `access_token` and `refresh_token`, then persists the
    /// [`GDriveCredentials`] (which are encrypted) to Lexe's DB.
    pub(crate) async fn exchange_code_and_persist_credentials(
        rng: &mut impl Crng,
        backend_api: &NodeBackendClient,
        gdrive_client: &gdrive::ReqwestClient,
        oauth: &OAuthConfig,
        google_auth_code: &str,
        authenticator: &BearerAuthenticator,
        vfs_master_key: &AesMasterKey,
    ) -> Result<GDriveCredentials, NodeApiError> {
        let code_verifier = None;
        let credentials = gdrive::oauth2::auth_code_for_token(
            gdrive_client,
            &oauth.client_id,
            Some(&oauth.client_secret),
            &oauth.redirect_uri,
            google_auth_code,
            code_verifier,
        )
        .await
        .context("Couldn't get tokens using code")
        .map_err(NodeApiError::provision)?;

        let credentials_file = persister::encrypt_gdrive_credentials(
            rng,
            vfs_master_key,
            &credentials,
        );

        persister::persist_file(backend_api, authenticator, &credentials_file)
            .await
            .context("Could not persist new GDrive credentials")
            .map_err(NodeApiError::provision)?;

        Ok(credentials)
    }

    /// Reads the GDrive credentials from Lexe's DB and sanity checks them.
    pub(super) async fn maybe_read_and_validate_credentials(
        state: &AppRouterState,
        oauth: &OAuthConfig,
        authenticator: &BearerAuthenticator,
        vfs_master_key: &AesMasterKey,
    ) -> Result<Option<GDriveCredentials>, NodeApiError> {
        // See if credentials already exist.
        let maybe_credentials = persister::read_gdrive_credentials(
            state.backend_api.as_ref(),
            authenticator,
            vfs_master_key,
        )
        .await
        .context("Couldn't read GDrive credentials")
        .map_err(NodeApiError::provision)?;

        let credentials = match maybe_credentials {
            Some(creds) => creds,
            None => return Ok(None),
        };

        // Sanity check the returned credentials
        if credentials.client_id != oauth.client_id {
            return Err(NodeApiError::provision("`client_id`s didn't match!"));
        }
        if credentials.client_secret.as_deref()
            != Some(oauth.client_secret.as_ref())
        {
            return Err(NodeApiError::provision(
                "`client_secret`s didn't match!",
            ));
        }

        Ok(Some(credentials))
    }

    /// Set up the "Google VFS" in the user's GDrive.
    ///
    /// - Creates the GVFS file folder structure (if it didn't exist)
    /// - Persist the encrypted root seed to GDrive (if provided).
    pub(super) async fn setup_gvfs_and_persist_seed(
        state: &mut AppRouterState,
        req: NodeProvisionRequest,
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

            match req.encrypted_seed {
                Some(enc_seed) => {
                    // If we were given a seed, persist it unconditionally, as
                    // the user may have rotated their encryption password.
                    persister::upsert_password_encrypted_root_seed(
                        &google_vfs,
                        enc_seed,
                    )
                    .await
                    .context("Failed to persist encrypted root seed")
                    .map_err(NodeApiError::provision)?;
                }
                None => {
                    // GDrive is enabled and the user doesn't have a seed
                    // backup, and didn't provide one. For safety, require it.
                    if !backup_exists {
                        return Err(NodeApiError::provision(
                            "Missing pw-encrypted root seed backup in GDrive; \
                             please provide one in another provision request",
                        ));
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
