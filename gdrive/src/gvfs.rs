//! Abstracts over the Google Drive API to create a simplified VFS interface.

// To avoid confusion between [`GFileId`] and [`VfsFile::id`], [`GFile`] and
// [`VfsFile`], etc, we use `g` or `v` prefixes in method, struct field, and
// variable names to denote "Google" or "VFS" respectively. 'VFS' refers to the
// VFS abstraction while 'GVFS' refers to the actual layout of files in GDrive.

use std::{collections::BTreeMap, fmt, str::FromStr};

use anyhow::{anyhow, ensure, Context};
use common::{api::user::UserPk, env::DeployEnv, ln::network::LxNetwork};
use lexe_api_core::{
    vfs,
    vfs::{VfsDirectory, VfsFile, VfsFileId},
};
use lexe_std::Apply;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, instrument, warn};

use crate::{
    api::{self, GDriveClient},
    gvfs_file_id::GvfsFileId,
    lexe_dir,
    models::GFileId,
    oauth2::GDriveCredentials,
    ReqwestClient,
};

// Allows tests to assert that these `anyhow::Error`s happened.
pub const CREATE_DUPE_MSG: &str = "Tried to create duplicate";
pub const NOT_FOUND_MSG: &str = "not found";

/// The name of the fully namespaced data dir inside `LEXE_DIR_NAME`
/// that contains the actual channel_manager, etc...
///
/// This extra namespacing allows a single device to have Lexe wallets across
/// (1) different (deploy_env, network, sgx), like staging-testnet-sgx vs
/// prod-mainnet-sgx, and (2) different wallets within a (deploy_env, network,
/// sgx), though this isn't fully supported yet.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, proptest_derive::Arbitrary))]
pub struct GvfsRootName {
    pub deploy_env: DeployEnv,
    pub network: LxNetwork,
    pub use_sgx: bool,
    pub user_pk: UserPk,
}

/// Opaque object containing info about the GVFS root. Crate users should
/// persist this and resupply it the next time [`GoogleVfs`] is initialized.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GvfsRoot {
    pub name: GvfsRootName,
    /// The [`GFileId`] corresponding to the GVFS root dir in Google Drive.
    pub(crate) gid: GFileId,
}

/// Abstracts over the GDrive API client to expose a simple VFS interface:
///
/// - [`get_file`](Self::get_file)
/// - [`create_file`](Self::create_file)
/// - [`upsert_file`](Self::upsert_file)
/// - [`delete_file`](Self::delete_file)
/// - [`get_directory`](Self::get_directory)
///
/// ### Characteristics
///
/// - Initialization takes 1 API round trip (~250ms) in the majority of cases.
///   If initializing for the first time, or if no [`GvfsRoot`] was supplied,
///   more API calls are necessary.
/// - An internal cache allows all VFS operations to require no extra roundtrips
///   just for fetching GDrive-specific metadata; [`get_file`] will directly
///   download the file, and [`create_file`] will directly upload it.
/// - The internal cache assumes that this [`GoogleVfs`] instance is the only
///   one modifying the underlying data store. DO NOT concurrently access data
///   stored in Google Drive from multiple locations.
///
/// [`get_file`]: Self::get_file
/// [`create_file`]: Self::create_file
/// [`upsert_file`]: Self::upsert_file
/// [`delete_file`]: Self::delete_file
/// [`get_directory`]: Self::get_directory
pub struct GoogleVfs {
    client: GDriveClient,
    gvfs_root: GvfsRoot,
    /// Caches the [`GFileId`]s of all GVFS files.
    ///
    /// ### Cache invariants
    ///
    /// - Since the cache is populated during init with a real API call to the
    ///   underlying data store listing all files contained in the GVFS root,
    ///   the cache is *complete*; therefore, it can be used to determine
    ///   whether a requested file *doesn't* exist (exclusion), in addition to
    ///   the standard case of determining an existing file's [`GFileId`].
    /// - Since the [`VfsFileId`]s in the cache were built from [`GvfsFileId`]s
    ///   during init, all [`VfsFileId`]s contained in the cache are infallibly
    ///   convertible to and from [`GvfsFileId`]s.
    ///
    /// ### Cache consistency
    ///
    /// - VFS methods which read from the GVFS (`get_file` and `get_directory`)
    ///   must hold a reader lock on the cache for the entire time that it is
    ///   acting based off of data in cache.
    /// - Methods which write to the GVFS (`create_file`, `upsert_file`,
    ///   `delete_file`) must likewise hold a writer lock for the entire time
    ///   it is acting on data read from cache, and must additionally update
    ///   the cache with any changes made to the underlying data store.
    /// - NOTE that cache consistency carries the additional assumption that no
    ///   other [`GoogleVfs`] clients are mutating the underlying data store.
    ///
    /// Even if we don't cache anything, the reader-writer lock is still
    /// required due to the possibility of TOCTTOU races arising from
    /// concurrent access. For example, consider the following interleaving
    /// if two tasks/threads call `create_file` at about the same time:
    ///
    /// - Thread 1 reads and confirms the file doesn't exist.
    /// - Thread 2 reads and confirms the file doesn't exist.
    /// - Thread 1 writes to the GVFS, creating the file.
    /// - Thread 2 writes to the GVFS, creating a duplicate.
    ///
    /// The reader-writer lock would force thread 2 to wait until thread 1 has
    /// finished its write; thread 2 would then see that the file already
    /// exists and would not create a duplicate.
    gid_cache: tokio::sync::RwLock<BTreeMap<VfsFileId, GFileId>>,
}

impl GoogleVfs {
    /// Initializes the [`GoogleVfs`]. Callers should supply the [`GvfsRoot`] if
    /// known in order to save a few extra round trips during init.
    ///
    /// If no [`GvfsRoot`] was supplied, or if the supplied [`GvfsRoot`] was
    /// wrong, `Some(GvfsRoot)` is returned, which the caller should persist.
    ///
    /// Whenever the [`GDriveCredentials`] is refreshed, an update is sent over
    /// the returned [`watch::Receiver`], which the caller should persist.
    #[instrument(skip_all, name = "(gvfs-init)")]
    pub async fn init(
        credentials: GDriveCredentials,
        gvfs_root_name: GvfsRootName,
        maybe_given_gvfs_root: Option<GvfsRoot>,
    ) -> anyhow::Result<(
        Self,
        Option<GvfsRoot>,
        watch::Receiver<GDriveCredentials>,
    )> {
        let client = ReqwestClient::new();
        let (client, credentials_rx) = GDriveClient::new(client, credentials);
        let (google_vfs, maybe_new_gvfs_root) = Self::init_from_client(
            client,
            gvfs_root_name,
            maybe_given_gvfs_root,
        )
        .await?;
        Ok((google_vfs, maybe_new_gvfs_root, credentials_rx))
    }

    /// Extracting this helper saves some extra API calls in tests.
    async fn init_from_client(
        client: GDriveClient,
        gvfs_root_name: GvfsRootName,
        maybe_given_gvfs_root: Option<GvfsRoot>,
    ) -> anyhow::Result<(Self, Option<GvfsRoot>)> {
        let using_given_root = maybe_given_gvfs_root.is_some();
        let mut gvfs_root_found_or_corrected = false;

        let mut gvfs_root = match maybe_given_gvfs_root {
            Some(given_gvfs_root) => {
                // Sanity check in case the config got mixed up somehow
                let given_gvfs_root_name = &given_gvfs_root.name;
                ensure!(
                    given_gvfs_root_name == &gvfs_root_name,
                    "persisted GvfsRoot's name doesn't match expected value: \
                     {given_gvfs_root_name} != {gvfs_root_name}"
                );
                given_gvfs_root
            }
            None => {
                gvfs_root_found_or_corrected = true;

                let lexe_dir = lexe_dir::get_or_create_lexe_dir(&client)
                    .await
                    .context("get_or_create_lexe_dir 1")?
                    .id;
                lexe_dir::get_or_create_gvfs_root(
                    &client,
                    &lexe_dir,
                    gvfs_root_name.clone(),
                )
                .await
                .context("get_or_create_gvfs_root 1")?
            }
        };

        // Populate the cache by fetching the metadata of all gfiles.
        let mut try_all_gfiles = client
            .list_direct_children(&gvfs_root.gid)
            .await
            .context("list_direct_children (original)");

        // If we're using the given gvfs root, it's possible it was wrong and
        // that we've been acting on incorrect information. We'll search Google
        // Drive for the gvfs root if we hit any of these cases:
        //
        // 1) list_direct_children returned an empty list: This is suspicious
        //    because we expect there to be files in the gvfs. The gvfs gid
        //    might be correctly-formatted but pointing to a non-existent dir.
        // 2) list_direct_children returned error: It's possible that this error
        //    was caused by a wrong or malformed gvfs root gid, so we should do
        //    a search just in case.
        let do_gvfs_search = match try_all_gfiles {
            Ok(ref files) => files.is_empty(),
            Err(_) => true,
        };
        if using_given_root && do_gvfs_search {
            let lexe_dir = lexe_dir::get_or_create_lexe_dir(&client)
                .await
                .context("get_or_create_lexe_dir 2")?
                .id;
            let found_gvfs_root = lexe_dir::get_or_create_gvfs_root(
                &client,
                &lexe_dir,
                gvfs_root_name,
            )
            .await
            .context("get_or_create_gvfs_root 2")?;

            if gvfs_root != found_gvfs_root {
                warn!(
                    "Given gvfs root was wrong! Re-fetching all data \
                    and supplying a new gvfs root to persist."
                );

                // Update variables
                gvfs_root = found_gvfs_root;
                gvfs_root_found_or_corrected = true;

                // Refetch `try_all_gfiles` since it was based on a bad root
                try_all_gfiles = client
                    .list_direct_children(&gvfs_root.gid)
                    .await
                    .context("list_direct_children (refetch)");
            }
        }

        let all_gfiles = try_all_gfiles?;

        // Check that all gvfs files have a binary MIME type.
        // NOTE: Here, we used to `bail!()` if the file had the wrong MIME type.
        // However, Google started returning that ./channel_manager had MIME
        // type "application/x-tex-tfm" (???). So now, we just accept all MIME
        // types, since any corruption will be caught during deserialization.
        for gfile in all_gfiles.iter() {
            if gfile.mime_type != api::BINARY_MIME_TYPE {
                let name = &gfile.name;
                let wrong_mime = &gfile.mime_type;
                // Don't even log at WARN since this is an expected error.
                info!("GFile '{name}' has wrong mime type {wrong_mime}");
            }
        }

        // Build the gid cache.
        let gid_cache = all_gfiles
            .into_iter()
            .map(|gfile| {
                let gvfile_id = GvfsFileId::from_str(&gfile.name)
                    .context("GFile did not have a valid gvfile_id")?;
                let vfile_id = gvfile_id.to_vfile_id();
                let vfile_gid = gfile.id;
                Ok((vfile_id, vfile_gid))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>()
            .context("Could not build gid cache")?
            .apply(tokio::sync::RwLock::new);

        // Return a GVFS root to persist if we found it or corrected it.
        let gvfs_root_to_persist = if gvfs_root_found_or_corrected {
            Some(gvfs_root.clone())
        } else {
            None
        };

        let myself = Self {
            client,
            gvfs_root,
            gid_cache,
        };

        Ok((myself, gvfs_root_to_persist))
    }

    /// Whether a file for the given [`VfsFileId`] exists.
    /// This method only reads from the cache so it is essentially free.
    pub async fn file_exists(&self, vfile_id: &VfsFileId) -> bool {
        self.gid_cache.read().await.get(vfile_id).is_some()
    }

    // TODO(max): GoogleVfs should impl the Vfs trait
    #[instrument(skip_all, name = "(gvfs-get-file)")]
    pub async fn get_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<Option<VfsFile>> {
        let locked_cache = self.gid_cache.read().await;
        let vfile_gid = match locked_cache.get(vfile_id) {
            Some(gid) => gid.clone(),
            // No gid => no file, by cache invariants
            None => return Ok(None),
        };

        // Download the file data and return the VfsFile to the caller.
        let data = self
            .client
            .download_blob_file(&vfile_gid)
            .await
            .context("download_blob_file")?;

        let vfile = VfsFile {
            id: vfile_id.clone(),
            data,
        };

        Ok(Some(vfile))
    }

    /// The error will contain [`CREATE_DUPE_MSG`] if the file was a duplicate.
    // TODO(max): GoogleVfs should impl the Vfs trait
    #[instrument(skip_all, name = "(gvfs-create-file)")]
    pub async fn create_file(&self, vfile: VfsFile) -> anyhow::Result<()> {
        let mut locked_cache = self.gid_cache.write().await;

        // First, confirm that the file doesn't already exist.
        if locked_cache.get(&vfile.id).is_some() {
            let dirname = &vfile.id.dir.dirname;
            let filename = &vfile.id.filename;
            return Err(anyhow!("{CREATE_DUPE_MSG}: {dirname}/{filename}"));
        }

        // Upload the blob file into the GVFS root.
        let gvfile_id = GvfsFileId::try_from(&vfile.id)?;
        let gid = self
            .client
            .create_blob_file(
                self.gvfs_root.gid.clone(),
                gvfile_id.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?
            .id;
        locked_cache.insert(vfile.id, gid);

        Ok(())
    }

    // TODO(max): GoogleVfs should impl the Vfs trait
    #[instrument(skip_all, name = "(gvfs-upsert-file)")]
    pub async fn upsert_file(&self, vfile: VfsFile) -> anyhow::Result<()> {
        let mut locked_cache = self.gid_cache.write().await;

        // If the file exists, update it
        if let Some(gid) = locked_cache.get(&vfile.id) {
            return self
                .client
                .update_blob_file(gid.clone(), vfile.data)
                .await
                .map(|_| ())
                .context("update_blob_file");
        }
        // From here, we know the file doesn't exist. Create it.
        // NOTE: We don't use `create_file` here in order to avoid a deadlock.

        // Upload the blob file into the GVFS root.
        let gvfile_id = GvfsFileId::try_from(&vfile.id)?;
        let gid = self
            .client
            .create_blob_file(
                self.gvfs_root.gid.clone(),
                gvfile_id.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?
            .id;
        locked_cache.insert(vfile.id, gid);

        Ok(())
    }

    /// The error will contain [`NOT_FOUND_MSG`] if the file was not found.
    // TODO(max): GoogleVfs should impl the Vfs trait
    #[instrument(skip_all, name = "(gvfs-delete-file)")]
    pub async fn delete_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<()> {
        let mut locked_cache = self.gid_cache.write().await;

        let gid = match locked_cache.get(vfile_id) {
            Some(gid) => gid,
            None => {
                let dirname = &vfile_id.dir.dirname;
                let filename = &vfile_id.filename;
                return Err(anyhow!("{dirname}/{filename} {NOT_FOUND_MSG}"));
            }
        };

        self.client
            .delete_file(gid)
            .await
            .map(|_| ())
            .context("Failed to delete gdrive file")?;

        locked_cache
            .remove(vfile_id)
            .expect("My phone was just here, where did it go???");

        Ok(())
    }

    // TODO(max): GoogleVfs should impl the Vfs trait
    #[instrument(skip_all, name = "(gvfs-get-directory)")]
    pub async fn get_directory(
        &self,
        vdir: &VfsDirectory,
    ) -> anyhow::Result<Vec<VfsFile>> {
        // NOTE: We *could* support this just by removing the check, but this is
        // likely a programmer error confusing a VFS subdir with the VFS root.
        ensure!(
            vdir.dirname != vfs::SINGLETON_DIRECTORY,
            "We do not support listing files in the VFS root"
        );

        let locked_cache = self.gid_cache.read().await;

        // A lower bound on the "smallest" `VfsFileId` that can be in this dir.
        // 1) `VfsFileId`s are ordered first by dirname then by filename.
        // 2) the empty string is the "smallest" string.
        // Therefore, using "" as the filename guarantees that the [`VfsFileId`]
        // is smaller than all possible filenames contained in the vdir, since
        // GvfsFileId enforces that all filenames have at least one character.
        let lower_bound = VfsFileId::new(vdir.dirname.clone(), String::new());

        // Collect the gids and gvids of all files in this VFS subdir. Iterate
        // until the dirname no longer matches or there are no more items.
        let mut subdir_gid_gvids = Vec::new();
        for (vfile_id, gid) in locked_cache.range(lower_bound..) {
            if vfile_id.dir.dirname != vdir.dirname {
                break;
            }
            let gvfile_id =
                GvfsFileId::try_from(vfile_id).expect("Cache invariant");
            subdir_gid_gvids.push((gid.clone(), gvfile_id));
        }

        // Early return if the subdir contained no files
        if subdir_gid_gvids.is_empty() {
            return Ok(Vec::new());
        }

        // Download all of the files.
        let vfiles = subdir_gid_gvids
            .iter()
            .map(|(gid, gvfile_id)| async {
                let data = self
                    .client
                    .download_blob_file(gid)
                    .await
                    .with_context(|| gvfile_id.clone())
                    .context("download_blob_file")?;

                let vfile_id = gvfile_id.to_vfile_id();
                let vfile = VfsFile { id: vfile_id, data };

                Ok::<VfsFile, anyhow::Error>(vfile)
            })
            .apply(futures::future::join_all)
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<VfsFile>>>()?;

        Ok(vfiles)
    }
}

// --- impl GvfsRootName --- //

impl GvfsRootName {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let mut iter = s.split('-');
        let (deploy_env, network, sgx, user_pk) = match (
            iter.next(),
            iter.next(),
            iter.next(),
            iter.next(),
            iter.next(),
            iter.next(),
        ) {
            (
                Some("lexe"),
                Some(deploy_env),
                Some(network),
                Some(sgx),
                Some(user_pk),
                None,
            ) => (deploy_env, network, sgx, user_pk),
            _ => return None,
        };

        let deploy_env = DeployEnv::from_str(deploy_env).ok()?;
        let network = LxNetwork::from_str(network).ok()?;
        let use_sgx = match sgx {
            "sgx" => true,
            "dbg" => false,
            _ => return None,
        };
        let user_pk = UserPk::from_str(user_pk).ok()?;

        Some(Self {
            deploy_env,
            network,
            use_sgx,
            user_pk,
        })
    }
}

impl fmt::Display for GvfsRootName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let deploy_env = self.deploy_env.as_str();
        let network = self.network.as_str();
        let sgx = if self.use_sgx { "sgx" } else { "dbg" };
        let user_pk = self.user_pk;
        write!(f, "lexe-{deploy_env}-{network}-{sgx}-{user_pk}")
    }
}

impl FromStr for GvfsRootName {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).with_context(|| format!("Invalid GVFS root name: '{s}'"))
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn test_gvfs_root_name_serde() {
        // FromStr/Display
        let ex = GvfsRootName {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx: false,
            user_pk: UserPk::from_u64(6546565654654654),
        };
        assert_eq!("lexe-dev-regtest-dbg-be2e581811421700000000000000000000000000000000000000000000000000", ex.to_string());
        roundtrip::fromstr_display_roundtrip_proptest::<GvfsRootName>();

        // JSON
        let json_str = r#"{"deploy_env":"dev","network":"regtest","use_sgx":false,"user_pk":"be2e581811421700000000000000000000000000000000000000000000000000"}"#;
        assert_eq!(json_str, serde_json::to_string(&ex).unwrap());
        assert_eq!(ex, serde_json::from_str::<GvfsRootName>(json_str).unwrap());
        roundtrip::json_value_roundtrip_proptest::<GvfsRootName>();
    }

    /// Test utility to search for the Lexe dir and delete the regtest VFS root
    /// inside if it exists. We do NOT delete the entire Lexe dir in case there
    /// are "mainnet" or "testnet{3,4}" folders holding real funds.
    async fn delete_vfs_root(
        client: &GDriveClient,
        gvfs_root_name: &GvfsRootName,
    ) {
        let maybe_lexe_dir = lexe_dir::find_lexe_dir(client)
            .await
            .expect("find_lexe_dir failed");
        let lexe_dir = match maybe_lexe_dir {
            Some(d) => d,
            None => return,
        };

        let gvfs_root_name_str = gvfs_root_name.to_string();
        let maybe_gvfs_root = lexe_dir::get_gvfs_root_gid(
            client,
            &lexe_dir.id,
            &gvfs_root_name_str,
        )
        .await
        .expect("get_vfs_root failed");
        let regtest_gvfs_root = match maybe_gvfs_root {
            Some(vr) => vr,
            None => return,
        };

        client
            .delete_file(&regtest_gvfs_root)
            .await
            .expect("delete_file failed");
    }

    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_gvfs --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_gvfs() {
        logger::init_for_testing();

        let client = ReqwestClient::new();
        let credentials = GDriveCredentials::from_env().unwrap();
        let (client, _rx) = GDriveClient::new(client, credentials);

        let gvfs_root_name = GvfsRootName {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx: false,
            user_pk: UserPk::from_u64(6549849),
        };
        delete_vfs_root(&client, &gvfs_root_name).await;

        let gvfs_root = None;
        let (gvfs, created_root) =
            GoogleVfs::init_from_client(client, gvfs_root_name, gvfs_root)
                .await
                .unwrap();
        created_root
            .expect("Should have been given the newly created root to persist");

        // Define the VFS files we'll be using throughout
        let file1 = VfsFile::new("dir", "file1", vec![1]);
        let file1_data2 = VfsFile::new("dir", "file1", vec![2]);
        let file2 = VfsFile::new("dir", "file2", vec![3]);

        // Create and get file1
        gvfs.create_file(file1.clone()).await.unwrap();
        let get_file1 = gvfs.get_file(&file1.id).await.unwrap().unwrap();
        assert_eq!(get_file1, file1);

        // Attempt to create file1 again, where data is vec![2].
        // We should get a duplicate error.
        // When we fetch the file1 again, the data should be unmodified.
        let err = gvfs.create_file(file1_data2.clone()).await.unwrap_err();
        assert!(err.to_string().contains(CREATE_DUPE_MSG));
        let get_file1 = gvfs.get_file(&file1.id).await.unwrap().unwrap();
        assert_eq!(get_file1.data, vec![1]);

        // Upsert file1's data to vec![2]
        gvfs.upsert_file(file1_data2.clone()).await.unwrap();
        let file1_resp = gvfs.get_file(&file1_data2.id).await.unwrap().unwrap();
        assert_eq!(file1_resp.data, vec![2]);

        // Create file2 in the same directory, this time via upsert.
        // Fetch both files using `get_directory`.
        gvfs.upsert_file(file2.clone()).await.unwrap();
        let node_dir = VfsDirectory::new("dir");
        let get_dir_resp = gvfs.get_directory(&node_dir).await.unwrap();
        assert_eq!(get_dir_resp, vec![file1_data2.clone(), file2.clone()]);

        // Delete file1.
        // Fetching file1 should return None.
        // Fetching the directory should only return file2.
        gvfs.delete_file(&file1_data2.id).await.unwrap();
        let maybe_file1 = gvfs.get_file(&file1.id).await.unwrap();
        assert!(maybe_file1.is_none());
        let get_dir_resp = gvfs.get_directory(&node_dir).await.unwrap();
        assert_eq!(get_dir_resp, vec![file2.clone()]);

        // Attempting to delete file1 again should return a 'NotFound' error
        let err = gvfs.delete_file(&file1_data2.id).await.unwrap_err();
        assert!(err.to_string().contains(NOT_FOUND_MSG));
    }

    /// Initialize a [`GoogleVfs`] with a [`GvfsRoot`] whose [`GFileId`] is
    /// correctly-formatted but points to a deleted or non-existent directory.
    ///
    /// Init should find 0 files, search for the gvfs root, and recover.
    ///
    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_init_deleted_root --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_init_deleted_root() {
        logger::init_for_testing();

        let client = ReqwestClient::new();
        let credentials = GDriveCredentials::from_env().unwrap();
        let (client, _rx) = GDriveClient::new(client, credentials);

        let gvfs_root_name = GvfsRootName {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx: false,
            user_pk: UserPk::from_u64(9849849),
        };
        delete_vfs_root(&client, &gvfs_root_name).await;

        // Some random gid I created during testing
        let deleted_gid =
            GFileId("1kcQUtO1jMRw9MXJ-9ArWTgPa_elm2ikT".to_owned());
        let deleted_root = GvfsRoot {
            name: gvfs_root_name.clone(),
            gid: deleted_gid,
        };

        // Validate the precondition for our test: that list_direct_children
        // will return a success response with 0 results in it.
        let all_files = client
            .list_direct_children(&deleted_root.gid)
            .await
            .expect("This test expects a success response but with 0 results");
        assert!(all_files.is_empty(), "Test precondition not satisfied");

        let (gvfs, updated_root) = GoogleVfs::init_from_client(
            client,
            gvfs_root_name,
            Some(deleted_root),
        )
        .await
        .unwrap();
        updated_root.expect("Should have been given an updated GVFS root");

        // Sanity check that the updated gvfs root is valid
        let file1 = VfsFile::new("dir", "file1", vec![1]);
        gvfs.create_file(file1.clone()).await.unwrap();
        let get_file1 = gvfs.get_file(&file1.id).await.unwrap().unwrap();
        assert_eq!(get_file1, file1);
    }

    /// Initialize a [`GoogleVfs`] with a [`GvfsRoot`] whose [`GFileId`] is
    /// completely invalid - the [`GFileId`] is not correctly formatted.
    ///
    /// Init should get a 404, search for the gvfs root, and recover.
    ///
    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_init_bogus_root --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_init_bogus_root() {
        logger::init_for_testing();

        let client = ReqwestClient::new();
        let credentials = GDriveCredentials::from_env().unwrap();
        let (client, _rx) = GDriveClient::new(client, credentials);

        // TODO(max): In the other case, make a call to the list_direct_children
        // method to ensure that it actually does return Err.

        let gvfs_root_name = GvfsRootName {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx: false,
            user_pk: UserPk::from_u64(3385140),
        };
        delete_vfs_root(&client, &gvfs_root_name).await;

        let bogus_gid = GFileId("t0tAlLy!!wrong-/\\[]} format 11 ".to_owned());
        let bogus_root = GvfsRoot {
            name: gvfs_root_name.clone(),
            gid: bogus_gid,
        };

        // Validate the precondition for our test: that list_direct_children
        // will return an error response.
        client
            .list_direct_children(&bogus_root.gid)
            .await
            .map(|_| ())
            .expect_err("Test precondition not satisfied: expected Err resp");

        let (gvfs, updated_root) = GoogleVfs::init_from_client(
            client,
            gvfs_root_name,
            Some(bogus_root),
        )
        .await
        .unwrap();
        updated_root.expect("Should have been given an updated GVFS root");

        // Sanity check that the updated gvfs root is valid
        let file1 = VfsFile::new("dir", "file1", vec![1]);
        gvfs.create_file(file1.clone()).await.unwrap();
        let get_file1 = gvfs.get_file(&file1.id).await.unwrap().unwrap();
        assert_eq!(get_file1, file1);
    }

    /// Checks that the `dirname` contained in a [`VfsFileId`] takes precedence
    /// in [`VfsFileId`]'s [`Ord`] implementation, in case a Lexe dev
    /// accidentally reorders the fields or something. This invariant is relied
    /// upon by the range search inside [`get_directory`].
    ///
    /// [`get_directory`]: GoogleVfs::get_directory
    #[test]
    fn vfile_id_dirname_ordering_precedence() {
        let smoler = "a";
        let bugger = "b";
        assert!(smoler < bugger);

        let smoller_dirname_bugger_filename = VfsFileId {
            dir: VfsDirectory {
                dirname: smoler.into(),
            },
            filename: bugger.into(),
        };
        let bugger_dirname_smoller_filename = VfsFileId {
            dir: VfsDirectory {
                dirname: bugger.into(),
            },
            filename: smoler.into(),
        };

        assert!(
            smoller_dirname_bugger_filename < bugger_dirname_smoller_filename
        );
    }
}
