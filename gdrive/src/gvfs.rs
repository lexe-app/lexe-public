//! Abstracts over the Google Drive API to create a simplified VFS interface.

// To avoid confusion between [`GFileId`] and [`VfsFile::id`], [`GFile`] and
// [`VfsFile`], etc, we use `g` or `v` prefixes in method, struct field, and
// variable names to denote "Google" or "VFS" respectively. 'VFS' refers to the
// VFS abstraction while 'GVFS' refers to the actual layout of files in GDrive.

use anyhow::{anyhow, ensure, Context};
use common::{
    api::vfs::{VfsDirectory, VfsFile, VfsFileId},
    cli::Network,
    constants, Apply,
};
use tracing::instrument;

use crate::{
    api, api::GDriveClient, gname::GName, lexe_dir, models::GFileId,
    oauth2::ApiCredentials,
};

// Allows tests to assert that these `anyhow::Error`s happened.
pub const CREATE_DUPE_MSG: &str = "Tried to create duplicate";
pub const NOT_FOUND_MSG: &str = "not found";

/// Abstracts over the GDrive API client to expose a simple VFS interface:
/// - [`get_file`](Self::get_file)
/// - [`create_file`](Self::create_file)
/// - [`upsert_file`](Self::upsert_file)
/// - [`delete_file`](Self::delete_file)
/// - [`get_directory`](Self::get_directory)
pub struct GoogleVfs {
    client: GDriveClient,
    /// The [`GFileId`] corresponding to the root of the GVFS.
    gvfs_root_gid: GFileId,
    // TODO(max): Add cache here
    // TODO(max): Add tokio::sync::RwLock<()> here
}

impl GoogleVfs {
    /// Initializes the [`GoogleVfs`]. Callers should supply the `gvfs_root_gid`
    /// if known in order to save a few extra round trips during init.
    // TODO(max): Add some getter for the gvfs_root_gid so caller can persist
    #[instrument(skip_all, name = "(gvfs-init)")]
    pub async fn init(
        credentials: ApiCredentials,
        network: Network,
    ) -> anyhow::Result<Self> {
        let client = GDriveClient::new(credentials);

        let lexe_dir = lexe_dir::get_or_create_lexe_dir(&client)
            .await
            .context("get_or_create_lexe_dir")?
            .id;
        let gvfs_root_gid =
            lexe_dir::get_or_create_gvfs_root(&client, &lexe_dir, network)
                .await
                .context("get_or_create_vfs_root")?;

        // TODO(max): Do the initial population of the cache.
        // TODO(max): Check MIME types of all files here
        // TODO(max): Check that all files have valid gname here

        Ok(Self {
            client,
            gvfs_root_gid,
        })
    }

    #[instrument(skip_all, name = "(gvfs-get-file)")]
    pub async fn get_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<Option<VfsFile>> {
        let maybe_vfile_gid = self
            .find_vfile_gid(vfile_id)
            .await
            .context("find_vfile_gid")?;

        let vfile_gid = match maybe_vfile_gid {
            Some(gid) => gid,
            // No gid -> no file
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
        // First, confirm that the file doesn't already exist.
        if self
            .find_vfile_gid(&vfile.id)
            .await
            .context("find_vfile_gid")?
            .is_some()
        {
            let dirname = &vfile.id.dir.dirname;
            let filename = &vfile.id.filename;
            return Err(anyhow!("{CREATE_DUPE_MSG}: {dirname}/{filename}"));
        }

        // Upload the blob file into the GVFS root.
        // TODO(max): Cache the gid returned in the file metadata
        let gname = GName::try_from(&vfile.id)?;
        let _gfile = self
            .client
            .create_blob_file(
                self.gvfs_root_gid.clone(),
                gname.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?;

        Ok(())
    }

    #[instrument(skip_all, name = "(gvfs-upsert-file)")]
    pub async fn upsert_file(&self, vfile: VfsFile) -> anyhow::Result<()> {
        // If the file exists, update it
        if let Some(gid) = self
            .find_vfile_gid(&vfile.id)
            .await
            .context("find_vfile_gid")?
        {
            return self
                .client
                .update_blob_file(gid, vfile.data)
                .await
                .map(|_| ())
                .context("update_blob_file");
        }
        // From here, we know the file doesn't exist. Create it. NOTE: We don't
        // use `create_file` here in order to avoid a redundant "exists?" check.

        // Upload the blob file into the GVFS root.
        // TODO(max): Cache the gid returned in the file metadata
        let gname = GName::try_from(&vfile.id)?;
        let _gfile = self
            .client
            .create_blob_file(
                self.gvfs_root_gid.clone(),
                gname.into_inner(),
                vfile.data,
            )
            .await
            .context("create_blob_file")?;

        Ok(())
    }

    /// The error will contain [`NOT_FOUND_MSG`] if the file was not found.
    #[instrument(skip_all, name = "(gvfs-delete-file)")]
    pub async fn delete_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<()> {
        let vfile_gid = match self.find_vfile_gid(vfile_id).await? {
            Some(gid) => gid,
            None => {
                let dirname = &vfile_id.dir.dirname;
                let filename = &vfile_id.filename;
                return Err(anyhow!("{dirname}/{filename} {NOT_FOUND_MSG}"));
            }
        };

        self.client
            .delete_file(&vfile_gid)
            .await
            .map(|_| ())
            .context("Failed to delete gdrive file")
    }

    #[instrument(skip_all, name = "(gvfs-get-directory)")]
    pub async fn get_directory(
        &self,
        vdir: &VfsDirectory,
    ) -> anyhow::Result<Vec<VfsFile>> {
        ensure!(
            vdir.dirname != constants::SINGLETON_DIRECTORY,
            "We do not support listing files in the VFS root"
        );

        // Get the gfile metadata for all files contained in the GVFS root
        let all_gfiles = self
            .client
            // TODO(max): Do a more efficient search by substring
            .list_direct_children(&self.gvfs_root_gid)
            .await
            .context("list_direct_children")?;

        // Validate the MIME type of all returned child metadatas.
        // TODO(max): This should be moved to init
        for gfile in all_gfiles.iter() {
            if gfile.mime_type != api::BINARY_MIME_TYPE {
                let child_name = &gfile.name;
                let wrong_mime = &gfile.mime_type;
                return Err(anyhow!(
                    "Child '{child_name}' had wrong mime type {wrong_mime}"
                ));
            }
        }

        // Filter out all gfiles which are not contained in the requested subdir
        // or which had an invalid gname, and associate each with their gname.
        let subdir_gfiles = all_gfiles
            .into_iter()
            .filter_map(|gfile| {
                let gname = GName::try_from(gfile.name.clone()).ok()?;
                if gname.dirname() != vdir.dirname {
                    return None;
                }
                Some((gfile, gname))
            })
            .collect::<Vec<_>>();

        // Early return if the subdir contained no files
        if subdir_gfiles.is_empty() {
            return Ok(Vec::new());
        }

        // Download all of the files.
        let vfiles = subdir_gfiles
            .iter()
            .map(|(gfile, gname)| async {
                let data = self
                    .client
                    .download_blob_file(&gfile.id)
                    .await
                    .with_context(|| gfile.name.clone())
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

    /// Finds the [`GFileId`] for a [`VfsFileId`] by searching Google Drive.
    /// Returns [`None`] if the VFS file doesn't exist.
    async fn find_vfile_gid(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<Option<GFileId>> {
        let gname = GName::try_from(vfile_id)?;

        // In Google Drive, all files are in the VFS root under a GName.
        let maybe_gid = self
            .client
            .search_direct_children(&self.gvfs_root_gid, gname.as_inner())
            .await
            .context("search_direct_children")?
            .map(|gfile| gfile.id);

        Ok(maybe_gid)
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
            lexe_dir::get_gvfs_root(&client, &lexe_dir.id, &network_str)
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
        let gvfs = GoogleVfs::init(credentials, network).await.unwrap();

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
}
