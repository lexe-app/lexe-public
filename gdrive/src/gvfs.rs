//! Abstracts over the Google Drive API to create a simplified VFS interface.

// To avoid confusion between [`GFileId`] and [`VfsFile::id`], [`GFile`] and
// [`VfsFile`], etc, we use `g` or `v` prefixes in method, struct field, and
// variable names to denote "Google" or "VFS" respectively. 'VFS' refers to the
// VFS abstraction while 'GVFS' refers to the actual layout of files in GDrive.

use std::{collections::BTreeMap, str::FromStr};

use anyhow::{anyhow, bail, ensure, Context};
use common::{
    api::vfs::{VfsDirectory, VfsFile, VfsFileId},
    cli::Network,
    constants, Apply,
};
use serde::{Deserialize, Serialize};
use tracing::{instrument, warn};

use crate::{
    api, api::GDriveClient, gname::GName, lexe_dir, models::GFileId,
    oauth2::ApiCredentials,
};

// Allows tests to assert that these `anyhow::Error`s happened.
pub const CREATE_DUPE_MSG: &str = "Tried to create duplicate";
pub const NOT_FOUND_MSG: &str = "not found";

/// Opaque object containing info about the GVFS root. Crate users should
/// persist this and resupply it the next time [`GoogleVfs`] is initialized.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct GvfsRoot {
    /// The [`Network`] that this GVFS is for.
    pub(crate) network: Network,
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
    /// - Since the [`VfsFileId`]s in the cache were built from [`GName`]s
    ///   during init, all [`VfsFileId`]s contained in the cache are infallibly
    ///   convertible to and from [`GName`]s.
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
    #[instrument(skip_all, name = "(gvfs-init)")]
    pub async fn init(
        credentials: ApiCredentials,
        network: Network,
        maybe_given_gvfs_root: Option<GvfsRoot>,
    ) -> anyhow::Result<(Self, Option<GvfsRoot>)> {
        let client = GDriveClient::new(credentials);

        let using_given_root = maybe_given_gvfs_root.is_some();
        let mut gvfs_root = match maybe_given_gvfs_root {
            Some(given_gvfs_root) => {
                let gvfs_network = given_gvfs_root.network;
                // Sanity check in case the networks got mixed up somehow
                ensure!(
                    gvfs_network == network,
                    "given_gvfs_root network doesn't match given network: \
                    {gvfs_network}!={network}"
                );
                given_gvfs_root
            }
            None => {
                let lexe_dir = lexe_dir::get_or_create_lexe_dir(&client)
                    .await
                    .context("get_or_create_lexe_dir 1")?
                    .id;

                lexe_dir::get_or_create_gvfs_root(&client, &lexe_dir, network)
                    .await
                    .context("get_or_create_gvfs_root 1")?
            }
        };

        // Populate the cache by fetching the metadata of all gfiles.
        let mut all_gfiles = client
            .list_direct_children(&gvfs_root.gid)
            .await
            .context("list_direct_children 1")?;

        // If we're using the given gvfs root and no results were returned, it's
        // possible that the given gvfs root was wrong, since it's highly likely
        // there would be files in the GVFS root.
        // Search Google Drive for the gvfs root just in case.
        let mut gvfs_root_to_persist = None;
        if using_given_root && all_gfiles.is_empty() {
            let lexe_dir = lexe_dir::get_or_create_lexe_dir(&client)
                .await
                .context("get_or_create_lexe_dir 2")?
                .id;
            let found_gvfs_root =
                lexe_dir::get_or_create_gvfs_root(&client, &lexe_dir, network)
                    .await
                    .context("get_or_create_gvfs_root 2")?;

            if gvfs_root != found_gvfs_root {
                warn!(
                    "Given gvfs root was wrong! Re-fetching all data \
                    and supplying a new gvfs root to persist."
                );

                // Update variables
                gvfs_root = found_gvfs_root.clone();
                gvfs_root_to_persist = Some(found_gvfs_root);

                // Refetch `all_gfiles`, since that was based on a bad gvfs root
                all_gfiles = client
                    .list_direct_children(&gvfs_root.gid)
                    .await
                    .context("list_direct_children 2")?;
            }
        }

        // Check that all gvfs files have a binary MIME type.
        for gfile in all_gfiles.iter() {
            if gfile.mime_type != api::BINARY_MIME_TYPE {
                let name = &gfile.name;
                let wrong_mime = &gfile.mime_type;
                bail!("GFile '{name}' had wrong mime type {wrong_mime}");
            }
        }

        // Build the gid cache.
        let gid_cache = all_gfiles
            .into_iter()
            .map(|gfile| {
                let gname = GName::from_str(&gfile.name)
                    .context("GFile did not have a valid gname")?;
                let vfile_id = gname.to_vfile_id();
                let vfile_gid = gfile.id;
                Ok((vfile_id, vfile_gid))
            })
            .collect::<anyhow::Result<BTreeMap<_, _>>>()
            .context("Could not build gid cache")?
            .apply(tokio::sync::RwLock::new);

        let myself = Self {
            client,
            gvfs_root,
            gid_cache,
        };

        Ok((myself, gvfs_root_to_persist))
    }

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
        let gname = GName::try_from(&vfile.id)?;
        let gid = self
            .client
            .create_blob_file(
                self.gvfs_root.gid.clone(),
                gname.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?
            .id;
        locked_cache.insert(vfile.id, gid);

        Ok(())
    }

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
        let gname = GName::try_from(&vfile.id)?;
        let gid = self
            .client
            .create_blob_file(
                self.gvfs_root.gid.clone(),
                gname.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?
            .id;
        locked_cache.insert(vfile.id, gid);

        Ok(())
    }

    /// The error will contain [`NOT_FOUND_MSG`] if the file was not found.
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

    #[instrument(skip_all, name = "(gvfs-get-directory)")]
    pub async fn get_directory(
        &self,
        vdir: &VfsDirectory,
    ) -> anyhow::Result<Vec<VfsFile>> {
        // NOTE: We *could* support this just by removing the check, but this is
        // likely a programmer error confusing a VFS subdir with the VFS root.
        ensure!(
            vdir.dirname != constants::SINGLETON_DIRECTORY,
            "We do not support listing files in the VFS root"
        );

        let locked_cache = self.gid_cache.read().await;

        // A lower bound on the "smallest" `VfsFileId` that can be in this dir.
        // 1) `VfsFileId`s are ordered first by dirname then by filename.
        // 2) the empty string is the "smallest" string.
        // Therefore, using "" as the filename guarantees that the [`VfsFileId`]
        // is smaller than all possible filenames contained in the vdir, since
        // GName enforces that all filenames have at least one character.
        let lower_bound = VfsFileId::new(vdir.dirname.clone(), String::new());

        // Collect the gids and gnames of all files in this VFS subdir. Iterate
        // until the dirname no longer matches or there are no more items.
        let mut subdir_gid_gnames = Vec::new();
        for (vfile_id, gid) in locked_cache.range(lower_bound..) {
            if vfile_id.dir.dirname != vdir.dirname {
                break;
            }
            let gname = GName::try_from(vfile_id).expect("Cache invariant");
            subdir_gid_gnames.push((gid.clone(), gname));
        }

        // Early return if the subdir contained no files
        if subdir_gid_gnames.is_empty() {
            return Ok(Vec::new());
        }

        // Download all of the files.
        let vfiles = subdir_gid_gnames
            .iter()
            .map(|(gid, gname)| async {
                let data = self
                    .client
                    .download_blob_file(gid)
                    .await
                    .with_context(|| gname.clone())
                    .context("download_blob_file")?;

                let vfile_id = gname.to_vfile_id();
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

#[cfg(test)]
mod test {
    use super::*;

    /// Test utility to search for the Lexe dir and delete the regtest VFS root
    /// inside if it exists. We do NOT delete the entire Lexe dir in case there
    /// are "bitcoin" or "testnet" folders holding real funds.
    async fn delete_regtest_vfs_root(credentials: ApiCredentials) {
        let client = GDriveClient::new(credentials);

        let maybe_lexe_dir = lexe_dir::find_lexe_dir(&client)
            .await
            .expect("find_lexe_dir failed");
        let lexe_dir = match maybe_lexe_dir {
            Some(d) => d,
            None => return,
        };

        let network_str = Network::REGTEST.to_string();
        let maybe_vfs_root =
            lexe_dir::get_gvfs_root_gid(&client, &lexe_dir.id, &network_str)
                .await
                .expect("get_vfs_root failed");
        let regtest_vfs_root = match maybe_vfs_root {
            Some(vr) => vr,
            None => return,
        };

        client
            .delete_file(&regtest_vfs_root)
            .await
            .expect("delete_file failed");
    }

    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_vfs --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_vfs() {
        logger::init_for_testing();

        let credentials = ApiCredentials::from_env().unwrap();

        delete_regtest_vfs_root(credentials.clone()).await;

        let network = Network::REGTEST;
        let gid_cache = None;
        let (gvfs, _) = GoogleVfs::init(credentials, network, gid_cache)
            .await
            .unwrap();

        // Define the VFS files we'll be using throughout
        let file1 = VfsFile::new("dir".into(), "file1".into(), vec![1]);
        let file1_data2 = VfsFile::new("dir".into(), "file1".into(), vec![2]);
        let file2 = VfsFile::new("dir".into(), "file2".into(), vec![3]);

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
        let node_dir = VfsDirectory {
            dirname: "dir".into(),
        };
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
                dirname: smoler.to_owned(),
            },
            filename: bugger.to_owned(),
        };
        let bugger_dirname_smoller_filename = VfsFileId {
            dir: VfsDirectory {
                dirname: bugger.to_owned(),
            },
            filename: smoler.to_owned(),
        };

        assert!(
            smoller_dirname_bugger_filename < bugger_dirname_smoller_filename
        );
    }
}
