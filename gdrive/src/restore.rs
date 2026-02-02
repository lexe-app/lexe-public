//! Provides a `GDriveRestoreClient`. Used by the user's mobile client when
//! they need to restore their wallet from a prior Google Drive backup.

use anyhow::Context;
use bytes::Bytes;
use common::{
    env::DeployEnv, ln::network::LxNetwork, rng::Crng, root_seed::RootSeed,
};
use lexe_api_core::vfs::{
    PW_ENC_ROOT_SEED_FILENAME, SINGLETON_DIRECTORY, VfsFile, VfsFileId,
};
use tokio::sync::watch;
use tracing::{instrument, warn};

use crate::{
    GvfsRoot,
    api::GDriveClient,
    gvfs::GvfsRootName,
    gvfs_file_id::GvfsFileId,
    lexe_dir::find_lexe_dir,
    models::GFile,
    oauth2::{GDriveCredentials, ReqwestClient},
};

/// A candidate wallet to restore.
pub struct GDriveRestoreCandidate {
    pub gvfs_root: GvfsRoot,
    pub pw_enc_root_seed: VfsFile,
}

/// A small `GDriveClient` wrapper that we use on the mobile client when
/// restoring a wallet from their Google Drive backup.
pub struct GDriveRestoreClient {
    client: GDriveClient,
    _credentials_rx: watch::Receiver<GDriveCredentials>,
}

impl GDriveRestoreClient {
    pub fn new(client: ReqwestClient, credentials: GDriveCredentials) -> Self {
        let (client, credentials_rx) = GDriveClient::new(client, credentials);
        Self {
            client,
            _credentials_rx: credentials_rx,
        }
    }

    /// Look for all [`GDriveRestoreCandidate`]s in the user's gdrive.
    ///
    /// We support multiple wallets per user, which may be shared by the the
    /// same gdrive account, so this may return multiple "candidate" backups.
    /// In the app, we'll need to ask the user to choose which candidate to
    /// restore from.
    #[instrument(skip_all, name = "(restore-candidates)")]
    pub async fn find_restore_candidates(
        &self,
        deploy_env: DeployEnv,
        network: LxNetwork,
        use_sgx: bool,
    ) -> anyhow::Result<Vec<GDriveRestoreCandidate>> {
        let gvfs_roots = self
            .find_gvfs_root_candidates(deploy_env, network, use_sgx)
            .await?;

        let mut out = Vec::new();
        for gvfs_root in gvfs_roots {
            match self.get_pw_enc_root_seed(&gvfs_root).await {
                Ok(Some(pw_enc_root_seed)) =>
                    out.push(GDriveRestoreCandidate {
                        gvfs_root,
                        pw_enc_root_seed,
                    }),
                Ok(None) => warn!(
                    "GVFS root was missing a root seed backup: {}",
                    gvfs_root.name
                ),
                Err(err) => warn!("{err:#}: GVFS root: {}", gvfs_root.name),
            }
        }

        Ok(out)
    }

    /// Rotate the password used to encrypt the root seed backup in GDrive.
    ///
    /// Finds all restore candidates, filters by `user_pk`, then re-encrypts the
    /// existing root seed under `new_password` and updates the backup file.
    #[instrument(skip_all, name = "(rotate-backup-password)")]
    pub async fn rotate_backup_password(
        &self,
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
        network: LxNetwork,
        use_sgx: bool,
        root_seed: &RootSeed,
        new_password: &str,
    ) -> anyhow::Result<()> {
        let candidates = self
            .find_restore_candidates(deploy_env, network, use_sgx)
            .await?;

        let user_pk = root_seed.derive_user_pk();
        let gvfs_root = candidates
            .into_iter()
            .find(|candidate| candidate.gvfs_root.name.user_pk == user_pk)
            .map(|candidate| candidate.gvfs_root)
            .context("No root seed backup found for user_pk")?;

        let encrypted_seed = root_seed
            .password_encrypt(rng, new_password)
            .context("Could not encrypt root seed under new password")?;

        let vfs_id =
            VfsFileId::new(SINGLETON_DIRECTORY, PW_ENC_ROOT_SEED_FILENAME);
        let gfile = self
            .find_pw_enc_root_seed_gfile(&vfs_id, &gvfs_root)
            .await?;
        match gfile {
            Some(gfile) => {
                self.client
                    .update_blob_file(gfile.id, Bytes::from(encrypted_seed))
                    .await
                    .context("Request for root seed backup update failed")?;
            }
            None => {
                let gvfs_id = GvfsFileId::try_from(&vfs_id)?;
                self.client
                    .create_blob_file(
                        gvfs_root.gid.clone(),
                        gvfs_id.into_inner(),
                        encrypted_seed,
                    )
                    .await
                    .context("Request for root seed backup create failed")?;
            }
        }

        Ok(())
    }

    /// Locate the LexeData dir and find all GVFS roots inside it that match the
    /// given env (deploy_env, network, sgx).
    async fn find_gvfs_root_candidates(
        &self,
        deploy_env: DeployEnv,
        network: LxNetwork,
        use_sgx: bool,
    ) -> anyhow::Result<Vec<GvfsRoot>> {
        // Look for LexeData root dir
        let maybe_lexe_dir = find_lexe_dir(&self.client)
            .await
            .context("LexeData lookup failed")?;
        let lexe_dir = match maybe_lexe_dir {
            Some(lexe_dir) => lexe_dir,
            None => return Ok(Vec::new()),
        };

        // Keep it simple. Just read all files/folders in LexeData.
        let candidate_gvfs_roots = self
            .client
            .list_direct_children(&lexe_dir.id)
            .await
            .context("Request for GVFS root directories failed")?;

        // Select only the potential GVFS roots for this env (deploy_env,
        // network, use_sgx).
        let candidate_gvfs_roots = candidate_gvfs_roots
            .into_iter()
            .filter_map(|gvfs_root| {
                GvfsRootName::parse(&gvfs_root.name).map(|name| GvfsRoot {
                    name,
                    gid: gvfs_root.id,
                })
            })
            .filter(|x| {
                x.name.deploy_env == deploy_env
                    && x.name.network == network
                    && x.name.use_sgx == use_sgx
            })
            .collect();

        Ok(candidate_gvfs_roots)
    }

    /// For a given GVFS, attempt to locate the password-encrypted root seed
    /// backup file and download it.
    async fn get_pw_enc_root_seed(
        &self,
        gvfs_root: &GvfsRoot,
    ) -> anyhow::Result<Option<VfsFile>> {
        let vfs_id =
            VfsFileId::new(SINGLETON_DIRECTORY, PW_ENC_ROOT_SEED_FILENAME);
        let gfile =
            self.find_pw_enc_root_seed_gfile(&vfs_id, gvfs_root).await?;

        let gfile = match gfile {
            Some(x) => x,
            None => return Ok(None),
        };

        let data = self
            .client
            .download_blob_file(&gfile.id)
            .await
            .context("Request for root seed backup data failed")?;

        Ok(Some(VfsFile { id: vfs_id, data }))
    }

    async fn find_pw_enc_root_seed_gfile(
        &self,
        vfs_id: &VfsFileId,
        gvfs_root: &GvfsRoot,
    ) -> anyhow::Result<Option<GFile>> {
        let gvfs_id = GvfsFileId::try_from(vfs_id)?;
        let gvfs_file_name = gvfs_id.into_inner();
        self.client
            .search_direct_children(&gvfs_root.gid, &gvfs_file_name)
            .await
            .context("Request for root seed backup file metadata failed")
    }
}
