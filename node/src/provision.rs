//! # Provisioning a new lexe node
//!
//! This module is responsible for running the node provisioning process for new
//! users and for existing users upgrading to new enclave versions.
//!
//! The intention of the provisioning process is for users to transfer their
//! secure secrets into a trusted enclave version without the operator (lexe)
//! learning their secrets. These secrets include sensitive data like wallet
//! private keys or mTLS certificates.
//!
//! A node enclave must also convince the user that the software is a version
//! that they trust and the software is running inside an up-to-date secure
//! enclave. We do this using a variant of RA-TLS (Remote Attestation TLS),
//! where the enclave platform endorsements and enclave measurements are bundled
//! in&&to a self-signed TLS certificate, which users must verify when
//! connecting to the provisioning endpoint.

use std::{net::TcpListener, sync::Arc, time::SystemTime};

use anyhow::Context;
use axum::{
    routing::{get, post},
    Router,
};
use common::{
    api::{
        self,
        auth::BearerAuthenticator,
        def::NodeRunnerApi,
        error::{NodeApiError, NodeErrorKind},
        ports::Ports,
        provision::{NodeProvisionRequest, SealedSeed},
        qs::GetByMeasurement,
        server::LayerConfig,
        Empty,
    },
    cli::node::ProvisionArgs,
    enclave::{self, MachineId, Measurement},
    net,
    rng::{Crng, SysRng},
    shutdown::ShutdownChannel,
    tls,
};
use gdrive::GoogleVfs;
use tracing::{debug, info, info_span, instrument};

use crate::{api::BackendApiClient, persister};

#[derive(Clone)]
struct RequestContext {
    args: Arc<ProvisionArgs>,
    client: gdrive::ReqwestClient,
    machine_id: MachineId,
    measurement: Measurement,
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
    // TODO(phlip9): make generic, use test rng in test
    rng: SysRng,
}

/// Provision a user node.
///
/// The `UserPk` is given by the runner so we know which user we should
/// provision to and have a simple method to authenticate their connection.
#[instrument(skip_all, parent = None, name = "(node-provision)")]
pub async fn provision_node<R: Crng>(
    rng: &mut R,
    args: ProvisionArgs,
    runner_api: Arc<dyn NodeRunnerApi + Send + Sync>,
    backend_api: Arc<dyn BackendApiClient + Send + Sync>,
) -> anyhow::Result<()> {
    debug!("provisioning");

    // Set up the request context and warp routes.
    let args = Arc::new(args);
    let client = gdrive::ReqwestClient::new();
    let machine_id = enclave::machine_id();
    let measurement = enclave::measurement();
    let ctx = RequestContext {
        args: args.clone(),
        client,
        machine_id,
        measurement,
        backend_api,
        // TODO(phlip9): use passed in rng
        rng: SysRng::new(),
    };
    let shutdown = ShutdownChannel::new();

    const APP_SERVER_SPAN_NAME: &str = "(app-node-provision-server)";
    let app_listener = TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
        .context("Failed to bind app listener")?;
    let app_port = app_listener
        .local_addr()
        .context("Couldn't get app addr")?
        .port();
    let (app_tls_config, app_dns) =
        tls::attestation::app_node_provision_server_config(rng, &measurement)
            .context("Failed to build TLS config for provisioning")?;
    let (app_server_task, _app_url) =
        api::server::spawn_server_task_with_listener(
            app_listener,
            app_router(ctx),
            LayerConfig::default(),
            Some((Arc::new(app_tls_config), app_dns.as_str())),
            APP_SERVER_SPAN_NAME,
            info_span!(parent: None, APP_SERVER_SPAN_NAME),
            shutdown.clone(),
        )
        .context("Failed to spawn app node provision server task")?;

    const LEXE_SERVER_SPAN_NAME: &str = "(lexe-node-provision-server)";
    let lexe_listener = TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
        .context("Failed to bind lexe listener")?;
    let lexe_port = lexe_listener
        .local_addr()
        .context("Couldn't get lexe addr")?
        .port();
    let lexe_tls_and_dns = None;
    let lexe_router = lexe_router(LexeRouterState {
        measurement,
        shutdown: shutdown.clone(),
    });
    let (lexe_server_task, _lexe_url) =
        api::server::spawn_server_task_with_listener(
            lexe_listener,
            lexe_router,
            LayerConfig::default(),
            lexe_tls_and_dns,
            LEXE_SERVER_SPAN_NAME,
            info_span!(parent: None, LEXE_SERVER_SPAN_NAME),
            shutdown,
        )
        .context("Failed to spawn lexe node provision server task")?;

    // Notify the runner that we're ready for a client connection
    let ports = Ports::new_provision(measurement, app_port, lexe_port);
    runner_api
        .ready(&ports)
        .await
        .context("Failed to notify runner of our readiness")?;
    debug!("Notified runner; awaiting client request");

    // Wait for API servers to receive shutdown signal and gracefully shut down
    app_server_task.await.context("App task panicked")?;
    lexe_server_task.await.context("Lexe task panicked")?;

    Ok(())
}

/// Implements [`AppNodeProvisionApi`] - only callable by the node owner.
///
/// [`AppNodeProvisionApi`]: common::api::def::AppNodeProvisionApi
fn app_router(ctx: RequestContext) -> Router<()> {
    Router::new()
        .route("/app/provision", post(handlers::provision))
        .with_state(ctx)
}

#[derive(Clone)]
struct LexeRouterState {
    measurement: Measurement,
    shutdown: ShutdownChannel,
}

/// Implements [`LexeNodeProvisionApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeProvisionApi`]: common::api::def::LexeNodeProvisionApi
fn lexe_router(state: LexeRouterState) -> Router<()> {
    Router::new()
        .route("/lexe/shutdown", get(handlers::shutdown))
        .with_state(state)
}

/// API handlers.
mod handlers {
    use axum::extract::State;
    use common::api::{
        error::{BackendApiError, BackendErrorKind},
        server::{extract::LxQuery, LxJson},
    };
    use tracing::warn;

    use super::*;
    use crate::approved_versions::ApprovedVersions;

    pub(super) async fn provision(
        State(mut ctx): State<RequestContext>,
        LxJson(req): LxJson<NodeProvisionRequest>,
    ) -> Result<LxJson<Empty>, NodeApiError> {
        debug!("Received provision request");

        let sealed_seed = SealedSeed::seal_from_root_seed(
            &mut ctx.rng,
            &req.root_seed,
            req.deploy_env,
            req.network,
            ctx.measurement,
            ctx.machine_id,
        )
        .map_err(NodeApiError::provision)?;

        // TODO(phlip9): [perf] could get the user to pass us their auth token
        // in the provision request instead of reauthing here.

        // authenticate as the user to the backend
        let user_key_pair = req.root_seed.derive_user_key_pair();
        let authenticator = BearerAuthenticator::new(
            user_key_pair,
            None, /* maybe_token */
        );
        let token = authenticator
            .get_token(ctx.backend_api.as_ref(), SystemTime::now())
            .await
            .map_err(|err| NodeApiError {
                kind: NodeErrorKind::BadAuth,
                msg: format!("{err:#}"),
            })?;

        // store the sealed seed and new node metadata in the backend
        ctx.backend_api
            .create_sealed_seed(&sealed_seed, token)
            .await
            .context("Could not persist sealed seed")
            .map_err(NodeApiError::provision)?;
        let user_pk = sealed_seed.id.user_pk;

        if !req.deploy_env.is_staging_or_prod() {
            // If we're not in staging/prod, provisioning is done.
            return Ok(LxJson(Empty {}));
        }
        // We're in staging/prod. There's some more work to do.

        let oauth = ctx
            .args
            .oauth
            .clone()
            .context("Missing OAuthConfig from Lexe operators")
            .map_err(NodeApiError::provision)?;
        let vfs_master_key = req.root_seed.derive_vfs_master_key();
        let credentials = match req.google_auth_code {
            Some(code) => {
                // We were given an auth code. Exchange for credentials and
                // persist.

                // Use the auth code to get a GDriveCredentials.
                let credentials = gdrive::oauth2::auth_code_for_token(
                    &ctx.client,
                    oauth.client_id,
                    oauth.client_secret,
                    &oauth.redirect_uri,
                    &code,
                )
                .await
                .context("Couldn't get tokens using code")
                .map_err(NodeApiError::provision)?;

                // Encrypt the GDriveCredentials and upsert into Lexe's DB.
                let credentials_file = persister::encrypt_gdrive_credentials(
                    &mut ctx.rng,
                    &vfs_master_key,
                    &credentials,
                );
                persister::persist_file(
                    ctx.backend_api.as_ref(),
                    &authenticator,
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
                    ctx.backend_api.as_ref(),
                    &authenticator,
                    &vfs_master_key,
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
                if oauth.client_secret != credentials.client_secret {
                    return Err(NodeApiError::provision(
                        "`client_secret`s didn't match!",
                    ));
                }

                credentials
            }
        };

        // If we are not allowed to access the Google VFS, we are done.
        if !req.allow_gvfs_access {
            // It is a usage error if they also provided a pw-encrypted seed.
            if req.encrypted_seed.is_some() {
                return Err(NodeApiError::provision(
                    "A root seed backup was provided, but it cannot be \
                    persisted because `allow_gvfs_access=false`",
                ));
            }

            return Ok(LxJson(Empty {}));
        }

        // See if we have a persisted gvfs root.
        let maybe_persisted_gvfs_root = persister::read_gvfs_root(
            &*ctx.backend_api,
            &authenticator,
            &vfs_master_key,
        )
        .await
        .context("Failed to fetch persisted gvfs root")
        .map_err(NodeApiError::provision)?;

        // Init the GVFS. This makes ~one API call to populate the cache.
        let (google_vfs, maybe_new_gvfs_root, mut credentials_rx) =
            GoogleVfs::init(
                credentials,
                req.network,
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
                    &mut ctx.rng,
                    &*ctx.backend_api,
                    &authenticator,
                    &vfs_master_key,
                    &new_gvfs_root,
                )
                .await
                .context("Failed to persist new gvfs root")
                .map_err(NodeApiError::provision)?;
            }

            // See if a root seed backup already exists. This does not check
            // whether the backup is well-formed, matches the current seed, etc.
            let backup_exists = persister::password_encrypted_root_seed_exists(
                &google_vfs,
                req.network,
            )
            .await;

            // If no backup exists in GDrive, we should create one, or error if
            // no pw-encrypted root seed was provided.
            if !backup_exists {
                let encrypted_seed = req
                    .encrypted_seed
                    .context(
                        "Missing pw-encrypted root seed backup in GDrive; \
                        please provide one in another provision request",
                    )
                    .map_err(NodeApiError::provision)?;

                persister::persist_password_encrypted_root_seed(
                    &google_vfs,
                    req.network,
                    encrypted_seed,
                )
                .await
                .context("Failed to persist encrypted root seed")
                .map_err(NodeApiError::provision)?;
            }

            // Fetch the approved versions list or create an empty one.
            let mut approved_versions =
                persister::read_approved_versions(&google_vfs, &vfs_master_key)
                    .await
                    .context("Couldn't read approved versions")
                    .map_err(NodeApiError::provision)?
                    .unwrap_or_else(ApprovedVersions::new);

            // Approve the current version, revoke old/yanked versions, etc.
            let (updated, revoked) = approved_versions
                .approve_and_revoke(&user_pk, ctx.measurement)
                .context("Error updating approved versions")
                .map_err(NodeApiError::provision)?;

            // If the list was updated, we need to (re)persist it.
            if updated {
                persister::persist_approved_versions(
                    &mut ctx.rng,
                    &google_vfs,
                    &vfs_master_key,
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
                        .get_token(ctx.backend_api.as_ref(), SystemTime::now())
                        .await
                        .map_err(|e| NodeApiError {
                            kind: NodeErrorKind::BadAuth,
                            msg: format!("{e:#}"),
                        })?;
                    let try_delete = ctx
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
                        }) => warn!(
                            %user_pk, %revoked_version, %revoked_measurement,
                            "Failed to delete revoked sealed seeds: \
                             revoked measurement wasn't found in DB: {msg}"
                        ),
                        Err(e) =>
                            return Err(NodeApiError {
                                kind: NodeErrorKind::Provision,
                                msg: format!(
                                    "Error deleting revoked sealed seeds: {e:#}"
                                ),
                            }),
                    }
                }
            }

            Ok::<_, NodeApiError>(())
        };
        let try_gvfs_ops = do_gvfs_ops.await;

        // If the GDriveCredentials were updated during the calls above, persist
        // the updated credentials so we can avoid a unnecessary refresh.
        let try_update_credentials =
            if matches!(credentials_rx.has_changed(), Ok(true)) {
                let credentials_file = persister::encrypt_gdrive_credentials(
                    &mut ctx.rng,
                    &vfs_master_key,
                    &credentials_rx.borrow_and_update(),
                );

                persister::persist_file(
                    ctx.backend_api.as_ref(),
                    &authenticator,
                    &credentials_file,
                )
                .await
                .context("Could not persist updated GDrive credentials")
                .map_err(NodeApiError::provision)
            } else {
                Ok(())
            };

        // Finally done. Return the first of any errors, otherwise Ok(Empty {}).
        try_gvfs_ops
            .and(try_update_credentials)
            .map(|()| LxJson(Empty {}))
    }

    pub(super) async fn shutdown(
        State(state): State<LexeRouterState>,
        LxQuery(req): LxQuery<GetByMeasurement>,
    ) -> Result<LxJson<Empty>, NodeApiError> {
        let LexeRouterState {
            measurement,
            shutdown,
        } = state;

        // Sanity check that the caller did indeed intend to shut down this node
        let given_measure = &req.measurement;
        if given_measure != &measurement {
            return Err(NodeApiError {
                kind: NodeErrorKind::WrongMeasurement,
                msg: format!("Given: {given_measure}, current: {measurement}"),
            });
        }

        // Send a shutdown signal.
        shutdown.send();

        Ok(LxJson(Empty {}))
    }
}
