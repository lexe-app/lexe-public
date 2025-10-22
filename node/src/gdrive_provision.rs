use std::sync::Arc;

use anyhow::Context;
use common::{
    aes::AesMasterKey,
    api::{provision::NodeProvisionRequest, user::UserPk},
    cli::OAuthConfig,
    rng::{Crng, SysRng},
};
use gdrive::{GoogleVfs, gvfs::GvfsRootName, oauth2::GDriveCredentials};
use lexe_api::{auth::BearerAuthenticator, error::NodeApiError};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use tracing::{debug, info, info_span, warn};

use crate::{client::NodeBackendClient, persister, provision};

/// Reads the GDrive credentials from Lexe's DB and sanity checks them.
pub(crate) async fn maybe_read_and_validate_credentials(
    state: &provision::AppRouterState,
    oauth: &OAuthConfig,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
) -> Result<Option<GDriveCredentials>, NodeApiError> {
    // See if credentials already exist.
    let maybe_credentials = persister::read_gdrive_credentials(
        state.backend_api(),
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
        return Err(NodeApiError::provision("`client_secret`s didn't match!"));
    }

    Ok(Some(credentials))
}

pub(crate) enum GoogleVfsInitError {
    FetchCreds(anyhow::Error),
    VfsInit(anyhow::Error),
    PersistRoot(anyhow::Error),
}

/// Helper to efficiently initialize a [`GoogleVfs`] and handle related work.
/// Also spawns a task which persists updated GDrive credentials.
/// Returns None if GDrive credentials are not available.
pub(crate) async fn maybe_init_google_vfs(
    backend_api: Arc<NodeBackendClient>,
    authenticator: Arc<BearerAuthenticator>,
    vfs_master_key: Arc<AesMasterKey>,
    gvfs_root_name: GvfsRootName,
    mut shutdown: NotifyOnce,
) -> Result<Option<(GoogleVfs, LxTask<()>)>, GoogleVfsInitError> {
    // Fetch the encrypted GDriveCredentials and persisted GVFS root.
    let (try_gdrive_credentials, try_persisted_gvfs_root) = tokio::join!(
        persister::read_gdrive_credentials(
            &backend_api,
            &authenticator,
            &vfs_master_key,
        ),
        persister::read_gvfs_root(
            &backend_api,
            &authenticator,
            &vfs_master_key
        ),
    );
    let maybe_gdrive_credentials = try_gdrive_credentials
        .context("Could not read GDrive credentials")
        .map_err(GoogleVfsInitError::FetchCreds)?;
    let persisted_gvfs_root = try_persisted_gvfs_root
        .context("Could not read gvfs root")
        .map_err(GoogleVfsInitError::FetchCreds)?;

    let gdrive_credentials = match maybe_gdrive_credentials {
        Some(creds) => creds,
        None => {
            info!("No GDrive credentials found; running without GoogleVfs");
            return Ok(None);
        }
    };

    let (google_vfs, maybe_new_gvfs_root, mut credentials_rx) =
        GoogleVfs::init(
            gdrive_credentials,
            gvfs_root_name,
            persisted_gvfs_root,
        )
        .await
        .map_err(GoogleVfsInitError::VfsInit)?;

    // If we were given a new GVFS root to persist, persist it.
    // This should only happen once so it won't impact startup time.
    let mut rng = SysRng::new();
    if let Some(new_gvfs_root) = maybe_new_gvfs_root {
        persister::persist_gvfs_root(
            &mut rng,
            &backend_api,
            &authenticator,
            &vfs_master_key,
            &new_gvfs_root,
        )
        .await
        .context("Failed to persist new GVFS root")
        .map_err(GoogleVfsInitError::PersistRoot)?;
    }

    // Spawn a task that repersists the GDriveCredentials every time
    // the contained access token is updated.
    let credentials_persister_task = {
        const SPAN_NAME: &str = "(gdrive-creds-persister)";
        LxTask::spawn_with_span(SPAN_NAME, info_span!(SPAN_NAME), async move {
            loop {
                tokio::select! {
                    Ok(()) = credentials_rx.changed() => {
                        let credentials_file =
                            persister::encrypt_gdrive_credentials(
                                &mut rng,
                                &vfs_master_key,
                                &credentials_rx.borrow_and_update(),
                            );

                        let try_persist = persister::persist_file(
                            &backend_api,
                            &authenticator,
                            &credentials_file,
                        )
                        .await;

                        match try_persist {
                            Ok(()) => debug!(
                                "Successfully persisted updated credentials"
                            ),
                            Err(e) => warn!(
                                "Failed to persist updated credentials: {e:#}"
                            ),
                        }
                    }
                    () = shutdown.recv() => return,
                }
            }
        })
    };

    Ok(Some((google_vfs, credentials_persister_task)))
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

/// Set up the "Google VFS" in the user's GDrive.
///
/// - Creates the GVFS file folder structure (if it didn't exist)
/// - Persist the encrypted root seed to GDrive (if provided).
pub(super) async fn setup_gvfs_and_persist_seed(
    state: &mut provision::AppRouterState,
    req: NodeProvisionRequest,
    authenticator: &BearerAuthenticator,
    credentials: GDriveCredentials,
    vfs_master_key: &AesMasterKey,
    user_pk: &UserPk,
) -> Result<(), NodeApiError> {
    // See if we have a persisted gvfs root.
    let maybe_persisted_gvfs_root = persister::read_gvfs_root(
        state.backend_api(),
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
        GoogleVfs::init(credentials, gvfs_root_name, maybe_persisted_gvfs_root)
            .await
            .context("Failed to init Google VFS")
            .map_err(NodeApiError::provision)?;

    // Do the GVFS operations in an async closure so we have a chance to
    // update the GDriveCredentials in Lexe's DB regardless of Ok/Err.
    let do_gvfs_ops = async {
        // If we were given a new GVFS root to persist, persist it.
        // This should only happen once.
        if let Some(new_gvfs_root) = maybe_new_gvfs_root {
            let (rng, backend_api) = state.rng_and_backend();
            persister::persist_gvfs_root(
                rng,
                backend_api,
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
            persister::password_encrypted_root_seed_exists(&google_vfs).await;

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
            let (rng, backend_api) = state.rng_and_backend();
            let credentials_file = persister::encrypt_gdrive_credentials(
                rng,
                vfs_master_key,
                &credentials_rx.borrow_and_update(),
            );

            persister::persist_file(
                backend_api,
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
