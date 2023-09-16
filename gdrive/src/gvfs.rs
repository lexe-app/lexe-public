//! Abstracts over the Google Drive API to create a simplified VFS interface.

// To avoid confusion between [`GFileId`] and [`VfsFile::id`], [`GFile`] and
// [`VfsFile`], etc, we use `g` or `v` prefixes in method, struct field, and
// variable names to denote "Google" or "VFS" respectively.

use anyhow::{anyhow, ensure, Context};
use common::{
    api::vfs::{VfsDirectory, VfsFile, VfsFileId},
    cli::Network,
    constants, Apply,
};
use tracing::instrument;

use crate::{
    api, api::GDriveClient, lexe_dir, models::GFileId, oauth2::ApiCredentials,
};

// Allows tests to assert that these `anyhow::Error`s happened.
const CREATE_DUPE_MSG: &str = "Tried to create duplicate";
const NOT_FOUND_MSG: &str = "not found";

// TODO(max): Figure out caching after everything else works

pub struct GoogleVfs {
    client: GDriveClient,
    /// The [`GFileId`] of the VFS root.
    vfs_root_gid: GFileId,
}

impl GoogleVfs {
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
        let vfs_root_gid =
            lexe_dir::get_or_create_vfs_root(&client, &lexe_dir, network)
                .await
                .context("get_or_create_vfs_root")?;

        Ok(Self {
            client,
            vfs_root_gid,
        })
    }

    #[instrument(skip_all, name = "(gvfs-get-file)")]
    pub async fn get_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<Option<VfsFile>> {
        // TODO(max): Return early if gid is in cache

        // Get the gid of the gdir that contains this file.
        // If the file is in the VFS root, its parent gid is the VFS root's gid.
        // If the file is in a VFS dir, its parent gid is the gid of the VFS dir
        let parent_gid = if vfile_id.dir.dirname
            == constants::SINGLETON_DIRECTORY
        {
            self.vfs_root_gid.clone()
        } else {
            let maybe_vdir_gid = self
                .get_vdir_gid(&vfile_id.dir)
                .await
                .context("get_vdir_gid")?;
            match maybe_vdir_gid {
                Some(d) => d,
                // Containing gdir didn't exist, therefore the file didn't exist
                None => return Ok(None),
            }
        };

        // Now we know the gid of the gdir that contains the file.
        // Search its direct children and get the gfile if it exists.
        let maybe_gfile = self
            .client
            .search_direct_children(&parent_gid, &vfile_id.filename)
            .await
            .context("search_direct_children")?;
        let gfile = match maybe_gfile {
            Some(gf) => gf,
            // Containing gdir existed, but the file itself didn't.
            None => return Ok(None),
        };

        // Download the file data and return the VfsFile to the caller.
        let data = self
            .client
            .download_blob_file(&gfile.id)
            .await
            .context("download_blob_file")?;

        let vfile = VfsFile {
            id: vfile_id.clone(),
            data,
        };

        Ok(Some(vfile))
    }

    #[instrument(skip_all, name = "(gvfs-create-file)")]
    pub async fn create_file(&self, vfile: VfsFile) -> anyhow::Result<()> {
        // First, confirm that the file doesn't already exist.
        if self
            .get_vfile_gid(&vfile.id)
            .await
            .context("get_vfile_gid")?
            .is_some()
        {
            let dirname = &vfile.id.dir.dirname;
            let filename = &vfile.id.filename;
            return Err(anyhow!("{CREATE_DUPE_MSG}: {dirname}/{filename}"));
        }

        // Get the gid of the vdir that contains it (which may be the root).
        let maybe_vdir_gid = self
            .get_vdir_gid(&vfile.id.dir)
            .await
            .context("get_vdir_gid")?;

        // If the vdir did not exist, we have to create it.
        let vdir_gid = match maybe_vdir_gid {
            Some(v) => v,
            // We know this is a VFS subdir because get_vdir_gid returned None,
            // and the parent of the VFS subdir is the VFS root.
            None => self
                .client
                .create_child_dir(
                    self.vfs_root_gid.clone(),
                    &vfile.id.dir.dirname,
                )
                .await
                .context("create_child_dir")?,
        };

        // Upload the blob file into the containing vdir (which may be the root)
        // TODO(max): Cache the gid returned in the file metadata
        let _gfile = self
            .client
            .create_blob_file(vdir_gid, vfile.id.filename, vfile.data)
            .await
            .context("create_blob_file")?;

        Ok(())
    }

    #[instrument(skip_all, name = "(gvfs-upsert-file)")]
    pub async fn upsert_file(&self, vfile: VfsFile) -> anyhow::Result<()> {
        // If the file exists, update it
        if let Some(gid) = self
            .get_vfile_gid(&vfile.id)
            .await
            .context("get_vfile_gid")?
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

        // Get the gid of the containing vdir (which may be the root)
        let maybe_vdir_gid = self
            .get_vdir_gid(&vfile.id.dir)
            .await
            .context("get_vdir_gid")?;

        // If the vdir did not exist, we have to create it.
        let vdir_gid = match maybe_vdir_gid {
            Some(v) => v,
            // We know this is a VFS subdir because get_vdir_gid returned None,
            // and the parent of the VFS subdir is the VFS root.
            None => self
                .client
                .create_child_dir(
                    self.vfs_root_gid.clone(),
                    &vfile.id.dir.dirname,
                )
                .await
                .context("create_child_dir")?,
        };

        // Upload the blob file into the containing vdir (which may be the root)
        // TODO(max): Cache the gid returned in the file metadata
        let _gfile = self
            .client
            .create_blob_file(vdir_gid, vfile.id.filename, vfile.data)
            .await
            .context("create_blob_file")?;

        Ok(())
    }

    #[instrument(skip_all, name = "(gvfs-delete-file)")]
    pub async fn delete_file(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<()> {
        let vfile_gid = match self.get_vfile_gid(vfile_id).await? {
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

        let vdir_gid =
            match self.get_vdir_gid(vdir).await.context("get_vdir_gid")? {
                Some(gid) => gid,
                // Subdir doesn't exist, therefore there are no files in it
                None => return Ok(Vec::new()),
            };

        // Get the gfile metadata for all files contained in this VFS subdir
        let child_gfiles = self
            .client
            .list_direct_children(&vdir_gid)
            .await
            .context("list_direct_children")?;

        // Early return if the gdir contained no children
        if child_gfiles.is_empty() {
            return Ok(Vec::new());
        }

        // Validate the MIME type of all returned child metadatas.
        for gfile in child_gfiles.iter() {
            if gfile.mime_type != api::BINARY_MIME_TYPE {
                let child_name = &gfile.name;
                let wrong_mime = &gfile.mime_type;
                return Err(anyhow!(
                    "Child '{child_name}' had wrong mime type {wrong_mime}"
                ));
            }
        }

        // Download all of the files.
        let vfiles = child_gfiles
            .iter()
            .map(|gfile| async {
                let data = self
                    .client
                    .download_blob_file(&gfile.id)
                    .await
                    .with_context(|| gfile.name.clone())
                    .context("download_blob_file")?;

                let dirname = vdir.dirname.clone();
                let filename = gfile.name.clone();

                let vfile = VfsFile::new(dirname, filename, data);

                Ok::<VfsFile, anyhow::Error>(vfile)
            })
            .apply(futures::future::join_all)
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<VfsFile>>>()?;

        Ok(vfiles)
    }

    /// Returns the [`GFileId`] corresponding to a [`VfsFileId`], or [`None`] if
    /// it doesn't exist.
    async fn get_vfile_gid(
        &self,
        vfile_id: &VfsFileId,
    ) -> anyhow::Result<Option<GFileId>> {
        // If this is a singleton file, search the vfs root's children.
        if vfile_id.dir.dirname == constants::SINGLETON_DIRECTORY {
            let maybe_gfile = self
                .client
                .search_direct_children(&self.vfs_root_gid, &vfile_id.filename)
                .await
                .context("search_direct_children 1")?;
            let maybe_gid = maybe_gfile.map(|gfile| gfile.id);
            return Ok(maybe_gid);
        }

        // This file is in a VFS subdir. Search the subdir's children.
        let maybe_vdir_gid = self
            .get_vdir_gid(&vfile_id.dir)
            .await
            .context("get_vdir_gid")?;
        let vdir_gid = match maybe_vdir_gid {
            Some(v) => v,
            // A gid for the subdir didn't exist => the file doesn't exist
            None => return Ok(None),
        };
        let maybe_gfile = self
            .client
            .search_direct_children(&vdir_gid, &vfile_id.filename)
            .await
            .context("search_direct_children 2")?;
        let maybe_gid = maybe_gfile.map(|gfile| gfile.id);
        Ok(maybe_gid)
    }

    /// Returns the [`GFileId`] for a VFS dir, which includes the VFS root as
    /// well as VFS subdirs. Returns [`None`] if the subdir doesn't exist.
    async fn get_vdir_gid(
        &self,
        vdir: &VfsDirectory,
    ) -> anyhow::Result<Option<GFileId>> {
        // If the caller wants the gid of the VFS root, we already have it.
        if vdir.dirname == constants::SINGLETON_DIRECTORY {
            return Ok(Some(self.vfs_root_gid.clone()));
        }
        // From here onwards, we know this is a VFS subdir.

        // TODO(max): Return early if gid is in cache

        // All VFS subdirs are direct children of the VFS root
        let maybe_gdir = self
            .client
            .search_direct_children(&self.vfs_root_gid, &vdir.dirname)
            .await
            .context("search_direct_children")?;
        let gdir = match maybe_gdir {
            Some(g) => g,
            // The dir doesn't exist
            None => return Ok(None),
        };

        // Validate the MIME type
        if gdir.mime_type != api::FOLDER_MIME_TYPE {
            let name = &gdir.name;
            let wrong_mime = &gdir.mime_type;
            return Err(anyhow!(
                "Dir '{name}' had wrong mime type {wrong_mime}"
            ));
        }

        Ok(Some(gdir.id))
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
            lexe_dir::get_vfs_root(&client, &lexe_dir.id, &network_str)
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
