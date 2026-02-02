use anyhow::Context;
use base64::Engine;
pub(crate) use common::root_seed::RootSeed as RootSeedRs;
use common::{
    api::user::{NodePk, UserPk},
    rng::SysRng,
};
use flutter_rust_bridge::RustOpaqueNom;
pub(crate) use gdrive::restore::{
    GDriveRestoreCandidate as GDriveRestoreCandidateRs,
    GDriveRestoreClient as GDriveRestoreClientRs,
};
use gdrive::{GoogleVfs, gvfs::GvfsRootName};
use lexe_api::vfs::{
    CHANNEL_MANAGER_FILENAME, CHANNEL_MONITORS_DIR, SINGLETON_DIRECTORY,
    VfsDirectory, VfsFile, VfsFileId,
};
use serde::Serialize;
use tracing::{error, instrument};

use crate::ffi::types::{DeployEnv, Network, RootSeed};

/// Context required to execute the Google Drive OAuth2 authorization flow.
pub struct GDriveOAuth2Flow {
    pub client_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub redirect_uri_scheme: String,
    pub url: String,
}

/// A basic authenticated Google Drive client, before we know which `UserPk`
/// to use.
pub struct GDriveClient {
    pub(crate) inner: RustOpaqueNom<GDriveClientInner>,
}

pub(crate) struct GDriveClientInner {
    client: gdrive::ReqwestClient,
    credentials: gdrive::oauth2::GDriveCredentials,
}

/// An authenticated Google Drive client used for restoring from backup.
pub struct GDriveRestoreClient {
    pub(crate) inner: RustOpaqueNom<GDriveRestoreClientRs>,
}

/// A candidate root seed backup. We just need the correct password to restore.
pub struct GDriveRestoreCandidate {
    pub(crate) inner: RustOpaqueNom<GDriveRestoreCandidateRs>,
}

impl GDriveOAuth2Flow {
    /// Begin the OAuth2 flow for the given mobile `client_id`. We'll also get
    /// a `server_code` we can exchange at the node provision enclave, which
    /// uses `server_client_id`.
    ///
    /// flutter_rust_bridge:sync
    pub fn init(client_id: String, server_client_id: &str) -> Self {
        let pkce = gdrive::oauth2::OAuth2PkceCodeChallenge::from_rng(
            &mut SysRng::new(),
        );

        // TODO(phlip9): Linux and Windows need to provide their own
        // `http://localhost:{port}` redirect URI.

        // Mobile clients use a "custom URI scheme", which is just their client
        // id with the DNS name segments reversed.
        let redirect_uri_scheme = client_id
            .as_str()
            .split('.')
            .rev()
            .collect::<Vec<_>>()
            .join(".");
        let redirect_uri = format!("{redirect_uri_scheme}:/");

        let url = gdrive::oauth2::auth_code_url(
            &client_id,
            Some(server_client_id),
            &redirect_uri,
            &pkce.code_challenge,
        );

        Self {
            client_id,
            code_verifier: pkce.code_verifier,
            redirect_uri,
            redirect_uri_scheme,
            url,
        }
    }

    /// After the user has authorized access and we've gotten the redirect,
    /// call this fn to exchange the client auth code for credentials + client.
    #[instrument(skip_all, name = "(gdrive-exchange)")]
    pub async fn exchange(
        &self,
        result_uri: &str,
    ) -> anyhow::Result<GDriveClient> {
        let code = gdrive::oauth2::parse_redirect_result_uri(result_uri)?;

        // // Uncomment while debugging client auth
        // tracing::info!("export GOOGLE_AUTH_CODE=\"{code}\"");

        let client = gdrive::oauth2::ReqwestClient::new();
        let client_secret = None;
        let credentials = gdrive::oauth2::auth_code_for_token(
            &client,
            &self.client_id,
            client_secret,
            &self.redirect_uri,
            code,
            Some(&self.code_verifier),
        )
        .await
        .context("Auth code exchange failed")?;

        // // Uncomment while debugging server auth
        // {
        //     let server_code = credentials.server_code.unwrap();
        //     tracing::info!("export GOOGLE_AUTH_CODE=\"{server_code}\"");
        // }

        Ok(GDriveClient {
            inner: RustOpaqueNom::new(GDriveClientInner {
                client,
                credentials,
            }),
        })
    }
}

impl GDriveClient {
    /// flutter_rust_bridge:sync
    pub fn into_restore_client(self) -> GDriveRestoreClient {
        let (client, credentials) = match self.inner.try_unwrap() {
            Ok(inner) => (inner.client, inner.credentials),
            Err(inner) => (inner.client.clone(), inner.credentials.clone()),
        };
        GDriveRestoreClient {
            inner: RustOpaqueNom::new(GDriveRestoreClientRs::new(
                client,
                credentials,
            )),
        }
    }

    /// flutter_rust_bridge:sync
    pub fn server_code(&self) -> Option<String> {
        self.inner.credentials.server_code.clone()
    }

    /// Read the core persisted Node state from the user's Google Drive VFS
    /// and dump it as a JSON blob.
    ///
    /// Used for debugging.
    pub async fn dump_state(
        &self,
        deploy_env: DeployEnv,
        network: Network,
        use_sgx: bool,
        root_seed: RootSeed,
    ) -> anyhow::Result<String> {
        #[derive(Serialize)]
        struct NodeStateDump {
            user_pk: UserPk,
            node_pk: NodePk,
            channel_manager: Option<Blob>,
            channel_monitors: Option<Vec<Blob>>,
        }

        #[derive(Serialize)]
        struct Blob {
            ciphertext: String,
            data: Option<String>,
        }

        let vfs_master_key = root_seed.inner.derive_vfs_master_key();
        let user_pk = root_seed.inner.derive_user_pk();
        let credentials = self.inner.credentials.clone();

        // A closure to decrypt and base64-encode blobs
        let base64 = base64::engine::general_purpose::STANDARD;
        let decrypt_blob_fn = |file: VfsFile| -> Blob {
            let ciphertext = base64.encode(&file.data);

            let aad =
                &[file.id.dir.dirname.as_bytes(), file.id.filename.as_bytes()];
            let data = vfs_master_key
                .decrypt(aad, file.data)
                .context("Failed to decrypt VFS file")
                .inspect_err(|err| error!("{:?} {err:#?}", file.id))
                .ok()
                .map(|data| base64.encode(data));

            Blob { ciphertext, data }
        };

        // Try to init the GDrive VFS
        let gvfs_root_name = GvfsRootName {
            deploy_env: deploy_env.into(),
            network: network.into(),
            use_sgx,
            user_pk,
        };
        let gvfs_cache = None;
        let (gvfs, _maybe_gvfs_root, _credentials_rx) =
            GoogleVfs::init(credentials, gvfs_root_name, gvfs_cache)
                .await
                .context("Failed to init GoogleVfs")?;

        // Try to read the channel manager
        let chanmgr_file_id = VfsFileId::new(
            SINGLETON_DIRECTORY.to_owned(),
            CHANNEL_MANAGER_FILENAME.to_owned(),
        );
        let maybe_chanmgr_file = gvfs
            .get_file(&chanmgr_file_id)
            .await
            .context("Failed to get channel_manager file")
            .inspect_err(|err| error!("{err:#?}"))
            .ok()
            .flatten();
        let channel_manager = maybe_chanmgr_file.map(decrypt_blob_fn);

        // Try to read the channel monitors
        let chanmons_dir = VfsDirectory::new(CHANNEL_MONITORS_DIR);
        let maybe_chanmon_files = gvfs
            .get_directory(&chanmons_dir)
            .await
            .context("Failed to get channel_monitors dir")
            .inspect_err(|err| error!("{err:#?}"))
            .ok();
        let channel_monitors = maybe_chanmon_files.map(|files| {
            files.into_iter().map(decrypt_blob_fn).collect::<Vec<_>>()
        });

        // Serialize the state dump to JSON
        let node_state_dump = NodeStateDump {
            user_pk,
            node_pk: root_seed.inner.derive_node_pk(&mut SysRng::new()),
            channel_manager,
            channel_monitors,
        };
        serde_json::to_string_pretty(&node_state_dump)
            .context("Failed to serialize node state dump")
    }
}

impl GDriveRestoreClient {
    pub async fn find_restore_candidates(
        &self,
        deploy_env: DeployEnv,
        network: Network,
        use_sgx: bool,
    ) -> anyhow::Result<Vec<GDriveRestoreCandidate>> {
        let restore_candidates = self
            .inner
            .find_restore_candidates(deploy_env.into(), network.into(), use_sgx)
            .await?;

        Ok(restore_candidates
            .into_iter()
            .map(|x| GDriveRestoreCandidate {
                inner: RustOpaqueNom::new(x),
            })
            .collect::<Vec<_>>())
    }

    pub async fn rotate_backup_password(
        &self,
        deploy_env: DeployEnv,
        network: Network,
        use_sgx: bool,
        root_seed: RootSeed,
        new_password: String,
    ) -> anyhow::Result<()> {
        self.inner
            .rotate_backup_password(
                &mut SysRng::new(),
                deploy_env.into(),
                network.into(),
                use_sgx,
                &root_seed.inner,
                &new_password,
            )
            .await
    }
}

impl GDriveRestoreCandidate {
    /// flutter_rust_bridge:sync
    pub fn user_pk(&self) -> String {
        self.inner.gvfs_root.name.user_pk.to_string()
    }

    /// flutter_rust_bridge:sync
    pub fn try_decrypt(&self, password: &str) -> anyhow::Result<RootSeed> {
        let root_seed = RootSeedRs::password_decrypt(
            password,
            self.inner.pw_enc_root_seed.data.clone(),
        )?;
        Ok(RootSeed::from(root_seed))
    }
}
