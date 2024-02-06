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

use std::{
    convert::Infallible,
    net::TcpListener,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use common::{
    api::{
        auth::BearerAuthenticator,
        def::NodeRunnerApi,
        error::{NodeApiError, NodeErrorKind},
        ports::Ports,
        provision::{NodeProvisionRequest, SealedSeed},
        qs::GetByMeasurement,
        rest, Empty,
    },
    cli::node::ProvisionArgs,
    enclave,
    enclave::{MachineId, Measurement},
    net,
    rng::{Crng, SysRng},
    shutdown::ShutdownChannel,
    task::LxTask,
    tls, Apply,
};
use gdrive::GoogleVfs;
use tracing::{debug, info, info_span, instrument, Span};
use warp::{filters::BoxedFilter, http::Response, hyper::Body, Filter};

use crate::{api::BackendApiClient, persister};

const WARP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
struct RequestContext {
    args: Arc<ProvisionArgs>,
    client: reqwest::Client,
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
    debug!(?args.port, "provisioning");

    // Set up the request context and warp routes.
    let args = Arc::new(args);
    // TODO(phlip9): Add Google certs here once the webpki feature is removed
    // from the `gdrive` crate
    let client = reqwest::Client::new();
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

    let app_routes = app_routes(ctx);
    // TODO(phlip9): remove when rest::serve_* supports TLS
    let app_routes =
        app_routes.with(rest::trace_requests(Span::current().id()));
    cfg_if::cfg_if! {
        if #[cfg(not(test))] {
            use common::{constants, enclave::MrShort};
            let mr_short = MrShort::from(&measurement);
            let dns_name = constants::node_provision_dns(&mr_short).to_owned();
        } else {
            // In tests we're not going through a proxy, so just bind cert to
            // "localhost".
            let dns_name = "localhost".to_owned();
        }
    }
    let app_tls_config =
        tls::attestation::node_provision_tls_config(rng, dns_name)
            .context("Failed to build TLS config for provisioning")?;
    let (app_addr, app_service) = warp::serve(app_routes)
        .tls()
        .preconfigured_tls(app_tls_config)
        .bind_with_graceful_shutdown(
            net::LOCALHOST_WITH_EPHEMERAL_PORT,
            shutdown.clone().recv_owned(),
        );
    let app_port = app_addr.port();
    let app_api_task = LxTask::spawn_named_with_span(
        "app api",
        info_span!(parent: None, "(app-api)"),
        app_service,
    );

    let lexe_routes = lexe_routes(measurement, shutdown.clone());
    let lexe_listener =
        TcpListener::bind(net::LOCALHOST_WITH_EPHEMERAL_PORT)
            .context("Could not bind TcpListener for Lexe operator API")?;
    let (lexe_api_task, lexe_addr) =
        rest::serve_routes_with_listener_and_shutdown(
            lexe_routes,
            shutdown.clone().recv_owned(),
            lexe_listener,
            "lexe api",
            info_span!(parent: None, "(lexe-api)"),
        )
        .context("Failed to serve Lexe routes")?;
    let lexe_port = lexe_addr.port();

    info!(%app_addr, %lexe_addr, "API socket addresses: ");

    // Notify the runner that we're ready for a client connection
    let ports = Ports::new_provision(measurement, app_port, lexe_port);
    runner_api
        .ready(&ports)
        .await
        .context("Failed to notify runner of our readiness")?;
    debug!("Notified runner; awaiting client request");

    // Wait for shutdown signal
    shutdown.recv_owned().await;

    // Check that the API tasks haven't hung
    app_api_task
        .apply(|fut| tokio::time::timeout(WARP_SHUTDOWN_TIMEOUT, fut))
        .await
        .context("Timed out waiting for app API task to finish")?
        .context("App task panicked")?;
    lexe_api_task
        .apply(|fut| tokio::time::timeout(WARP_SHUTDOWN_TIMEOUT, fut))
        .await
        .context("Timed out waiting for Lexe API task to finish")?
        .context("Lexe task panicked")?;

    Ok(())
}

/// Implements [`AppNodeProvisionApi`] - only callable by the node owner.
///
/// [`AppNodeProvisionApi`]: common::api::def::AppNodeProvisionApi
fn app_routes(ctx: RequestContext) -> BoxedFilter<(Response<Body>,)> {
    let provision = warp::path("provision")
        .and(warp::post())
        .and(inject::request_context(ctx))
        .and(warp::body::json::<NodeProvisionRequest>())
        .then(handlers::provision)
        .map(rest::into_response);
    let app_routes = warp::path("app").and(provision);

    app_routes.boxed()
}

/// Implements [`LexeNodeProvisionApi`] - only callable by the Lexe operators.
///
/// [`LexeNodeProvisionApi`]: common::api::def::LexeNodeProvisionApi
fn lexe_routes(
    measurement: Measurement,
    shutdown: ShutdownChannel,
) -> BoxedFilter<(Response<Body>,)> {
    let shutdown = warp::path("shutdown")
        .and(warp::get())
        .and(warp::query::<GetByMeasurement>())
        .and(inject::measurement(measurement))
        .and(inject::shutdown(shutdown))
        .map(handlers::shutdown)
        .map(rest::into_response);
    let lexe_routes = warp::path("lexe").and(shutdown);

    lexe_routes.boxed()
}

/// Filters for injecting structs/data into subsequent [`Filter`]s.
mod inject {
    use super::*;

    pub(super) fn request_context(
        ctx: RequestContext,
    ) -> impl Filter<Extract = (RequestContext,), Error = Infallible> + Clone
    {
        warp::any().map(move || ctx.clone())
    }

    pub(super) fn measurement(
        measurement: Measurement,
    ) -> impl Filter<Extract = (Measurement,), Error = Infallible> + Clone {
        warp::any().map(move || measurement)
    }

    pub(super) fn shutdown(
        shutdown: ShutdownChannel,
    ) -> impl Filter<Extract = (ShutdownChannel,), Error = Infallible> + Clone
    {
        warp::any().map(move || shutdown.clone())
    }
}

/// API handlers.
mod handlers {
    use common::api::error::{BackendApiError, BackendErrorKind};
    use tracing::warn;

    use super::*;
    use crate::approved_versions::ApprovedVersions;

    /// POST /app/provision [`NodeProvisionRequest`] -> [`()`]
    pub(super) async fn provision(
        mut ctx: RequestContext,
        req: NodeProvisionRequest,
    ) -> Result<Empty, NodeApiError> {
        debug!("Received provision request");

        let sealed_seed = SealedSeed::seal_from_root_seed(
            &mut ctx.rng,
            &req.root_seed,
            req.deploy_env,
            req.network,
            ctx.measurement,
            ctx.machine_id,
        )
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Provision,
            msg: format!("{e:#}"),
        })?;

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
            .map_err(|e| NodeApiError {
                kind: NodeErrorKind::Provision,
                msg: format!("Could not persist sealed seed: {e:#}"),
            })?;
        let user_pk = sealed_seed.id.user_pk;

        if !req.deploy_env.is_staging_or_prod() {
            // If we're not in staging/prod, provisioning is done.
            return Ok(Empty {});
        }
        // We're in staging/prod. There's some more work to do.

        let oauth = ctx.args.oauth.clone().ok_or_else(|| NodeApiError {
            kind: NodeErrorKind::Provision,
            msg: "Missing OAuthConfig from Lexe operators".to_owned(),
        })?;
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
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!("Couldn't get tokens using code: {e:#}"),
                })?;

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
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!(
                        "Could not persist new GDrive credentials: {e:#}"
                    ),
                })?;

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
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!("GDriveCredentials invalid or missing: {e:#}"),
                })?;

                // Sanity check the returned credentials
                if oauth.client_id != credentials.client_id {
                    return Err(NodeApiError {
                        kind: NodeErrorKind::Provision,
                        msg: "`client_id`s didn't match!".to_owned(),
                    });
                }
                if oauth.client_secret != credentials.client_secret {
                    return Err(NodeApiError {
                        kind: NodeErrorKind::Provision,
                        msg: "`client_secret`s didn't match!".to_owned(),
                    });
                }

                credentials
            }
        };

        // If we are not allowed to access the Google VFS, we are done.
        if !req.allow_gvfs_access {
            // It is a usage error if they also provided a pw-encrypted seed.
            if req.encrypted_seed.is_some() {
                return Err(NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: "A root seed backup was provided, but it cannot be \
                        persisted because `allow_gvfs_access=false`"
                        .to_owned(),
                });
            }

            return Ok(Empty {});
        }

        // See if we have a persisted gvfs root.
        let maybe_persisted_gvfs_root = persister::read_gvfs_root(
            &*ctx.backend_api,
            &authenticator,
            &vfs_master_key,
        )
        .await
        .map_err(|e| NodeApiError {
            kind: NodeErrorKind::Provision,
            msg: format!("Failed to fetch persisted gvfs root: {e:#}"),
        })?;

        // Init the GVFS. This makes ~one API call to populate the cache.
        let (google_vfs, maybe_new_gvfs_root, mut credentials_rx) =
            GoogleVfs::init(
                credentials,
                req.network,
                maybe_persisted_gvfs_root,
            )
            .await
            .map_err(|e| NodeApiError {
                kind: NodeErrorKind::Provision,
                msg: format!("Failed to init Google VFS: {e:#}"),
            })?;

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
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!("Failed to persist new gvfs root: {e:#}"),
                })?;
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
                let encrypted_seed =
                    req.encrypted_seed.ok_or_else(|| NodeApiError {
                        kind: NodeErrorKind::Provision,
                        msg:
                            "Missing pw-encrypted root seed backup in GDrive; \
                            please provide one in another provision request"
                                .to_owned(),
                    })?;

                persister::persist_password_encrypted_root_seed(
                    &google_vfs,
                    req.network,
                    encrypted_seed,
                )
                .await
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!(
                        "Failed to persist encrypted root seed: {e:#}"
                    ),
                })?;
            }

            // Fetch the approved versions list or create an empty one.
            let mut approved_versions =
                persister::read_approved_versions(&google_vfs, &vfs_master_key)
                    .await
                    .map_err(|e| NodeApiError {
                        kind: NodeErrorKind::Provision,
                        msg: format!("Couldn't read approved versions: {e:#}"),
                    })?
                    .unwrap_or_else(ApprovedVersions::new);

            // Approve the current version, revoke old/yanked versions, etc.
            let (updated, revoked) = approved_versions
                .approve_and_revoke(&user_pk, ctx.measurement)
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!("Error updating approved versions: {e:#}"),
                })?;

            // If the list was updated, we need to (re)persist it.
            if updated {
                persister::persist_approved_versions(
                    &mut ctx.rng,
                    &google_vfs,
                    &vfs_master_key,
                    &approved_versions,
                )
                .await
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!("Persist approved versions failed: {e:#}"),
                })?;
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
                .map_err(|e| NodeApiError {
                    kind: NodeErrorKind::Provision,
                    msg: format!(
                        "Could not persist updated GDrive credentials: {e:#}"
                    ),
                })
            } else {
                Ok(())
            };

        // Finally done. Return the first of any errors, otherwise Ok(Empty {}).
        try_gvfs_ops.and(try_update_credentials).map(|()| Empty {})
    }

    /// GET /lexe/shutdown [`GetByMeasurement`] -> [`()`]
    pub(super) fn shutdown(
        req: GetByMeasurement,
        measurement: Measurement,
        shutdown: ShutdownChannel,
    ) -> Result<Empty, NodeApiError> {
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

        Ok(Empty {})
    }
}
