//! This module contains utilities for finding or creating the LexeData dir,
//! as well as finding or creating the "gvfs root" for each network.
//!
//! The canonical folder structure is as follows:
//!
//! My Drive
//! |___ ... (The LexeData dir can be moved anywhere in My Drive)
//!     |___"X LexeData (DO NOT RENAME, MODIFY, OR DELETE)"
//!         |___"mainnet" (mainnet gvfs root)
//!         |   |___"./encrypted_root_seed" (singleton file, password-encrypted)
//!         |   |___"./channel_manager" (singleton file, AES encrypted)
//!         |   |___"channel_monitors/deadcafe" (vfs subdir file, AES encrypted)
//!         |   |___"channel_monitors/baddecaf" (vfs subdir file, AES encrypted)
//!         |   |___...
//!         |___"testnet3" (testnet gvfs root)
//!         |   |___"./encrypted_root_seed" (singleton file, password-encrypted)
//!         |   |___"./channel_manager" (singleton file, AES encrypted)
//!         |   |___"channel_monitors/deadcafe" (vfs subdir file, AES encrypted)
//!         |   |___"channel_monitors/baddecaf" (vfs subdir file, AES encrypted)
//!         |   |___...
//!         |___...
//!
//! See also the doc comments for `LEXE_DIR_NAME`.

use anyhow::Context;
use tracing::{debug, warn};

use crate::{
    api,
    api::GDriveClient,
    gvfs::{GvfsRoot, GvfsRootName},
    models::{GFile, GFileCow, GFileId, ListFiles},
};

/// The name of the Lexe data dir visible in a user's My Drive.
///
/// - The "X " prefix (close to the end of the alphabet) reduces the user's
///   annoyance at having our folder placed in their top-level hierarchy.
/// - The " (DO NOT RENAME, MODIFY, OR DELETE)" suffix clearly conveys not to
///   mess with the folder. Users are allowed to move the folder if they wish,
///   but that info will be in a less visible README.txt inside the dir so it
///   doesn't detract from the main message of "Don't touch!!"
/// - "LexeData" is the search term we use when locating the folder, to allow
///   for some fault-tolerance in case the user intentionally or accidentally
///   renames some portion of the prefix or suffix, even though we quite clearly
///   warn them not to do so.
pub const LEXE_DIR_NAME: &str = "X LexeData (DO NOT RENAME, MODIFY, OR DELETE)";

/// Searches "My Drive" for the LexeData dir and returns it if found.
/// Otherwise, the Lexe data dir is created in the "My Drive" root.
pub(crate) async fn get_or_create_lexe_dir(
    client: &GDriveClient,
) -> anyhow::Result<GFile> {
    // TODO(max): Fix the name if it differs from the const
    match find_lexe_dir(client).await.context("find_lexe_dir")? {
        Some(dir) => Ok(dir),
        None => create_lexe_dir(client).await.context("create_lexe_dir"),
    }
}

/// Searches "My Drive" for the Lexe data dir.
///
/// - We search for folders containing the exact string 'LexeData' and filter
///   out any inexact matches which the Google Drive API may have returned.
/// - If no matches are found, then [`Ok(None)`] is returned.
/// - If exactly one match is found, it is returned.
/// - If multiple matches are found, the match with the earliest creation date
///   is returned. This provides some fault-tolerance in the case that
///   successive duplicates are created for some reason.
///
/// NOTE: Unless our credentials have the higher-privileged
/// `https://www.googleapis.com/auth/drive` scope, this method can only
/// search within the files created by our own app! The stricter scope
/// is desirable for privacy/security reasons but may cause e.g. a "restore
/// from Drive" flow to fail if our app ID registered with Google
/// (definition unclear) changes between persist + restore.
/// <https://developers.google.com/drive/api/guides/api-specific-auth#scopes>
pub(crate) async fn find_lexe_dir(
    client: &GDriveClient,
) -> anyhow::Result<Option<GFile>> {
    // NOTE: This query for 'LexeData' is case-insensitive. Thus, we check
    // for the exact (case-sensitive) match when we get the results back.
    let query = "name contains 'LexeData' \
            and mimeType = 'application/vnd.google-apps.folder'\
            and trashed = false";

    let mut data = ListFiles {
        q: query.into(),
        // Order by creation time, ascending.
        order_by: Some("createdTime".into()),
        page_token: None,
    };

    let mut resp =
        client.list_files(&data).await.context("first list_files")?;

    if resp.files.len() > 1 {
        warn!("Search for 'LexeData' returned multiple results!");
    }

    // Pick out the first result that contains 'LexeData' *exactly*.
    let mut maybe_file = resp
        .files
        .into_iter()
        .find(|file| file.name.contains("LexeData"));

    // Keep paginating so long as:
    // 1) we haven't found an exact match
    // 2) there are still more results (there is a next_page_token)
    // 3) we haven't hit the limit of 10 additional pages (in case there is some
    //    bug putting us in an infinite loop)
    // Paginating at all should be extremely rare.
    let mut pages = 0;
    let limit = 10;
    while maybe_file.is_none()
        && resp.next_page_token.is_some()
        && pages < limit
    {
        warn!("No exact matches found");
        data.page_token = resp.next_page_token;

        resp = client.list_files(&data).await.context("paged list_files")?;

        maybe_file = resp
            .files
            .into_iter()
            .find(|file| file.name.contains("LexeData"));

        pages += 1;
    }

    if maybe_file.is_some() {
        debug!("Found a folder with 'LexeData' in the name");
    } else {
        debug!("Did not find any folders with 'LexeData' in the name");
    }
    Ok(maybe_file)
}

/// Creates the "LexeData" dir.
// TODO(max): Add a README.txt which gives more info about the dir
async fn create_lexe_dir(client: &GDriveClient) -> anyhow::Result<GFile> {
    let data = GFileCow {
        id: None,
        name: Some(LEXE_DIR_NAME.into()),
        // Create the dir in the My Drive root
        parents: None,
        mime_type: Some(api::FOLDER_MIME_TYPE.into()),
        // TODO(max): Choose a different color? Supported colors have to be
        // fetched via an API call to the "about" endpoint, and are returned in
        // the 'folderColorPalette' field.
        // https://developers.google.com/drive/api/reference/rest/v3/about
        folder_color_rgb: Some("#FF5733".into()),
    };
    client
        .create_empty_file(&data)
        .await
        .context("create_empty_file")
}

/// Given the [`GFileId`] of the parent LexeData dir, returns the [`GvfsRoot`].
/// The [`GvfsRoot`] is created if it didn't exist.
pub(crate) async fn get_or_create_gvfs_root(
    client: &GDriveClient,
    lexe_dir: &GFileId,
    gvfs_root_name: GvfsRootName,
) -> anyhow::Result<GvfsRoot> {
    let gvfs_root_name_str = gvfs_root_name.to_string();
    let maybe_gvfs_root_gid =
        get_gvfs_root_gid(client, lexe_dir, &gvfs_root_name_str)
            .await
            .context("get_gvfs_root_gid")?;

    let gvfs_root_gid = match maybe_gvfs_root_gid {
        Some(gid) => gid,
        None => client
            .create_child_dir(lexe_dir.clone(), &gvfs_root_name_str)
            .await
            .context("create_child_dir")?,
    };

    let gvfs_root = GvfsRoot {
        name: gvfs_root_name,
        gid: gvfs_root_gid,
    };

    Ok(gvfs_root)
}

/// Given the [`GFileId`] of the parent LexeData dir, returns the [`GFileId`]
/// corresponding of the GVFS root, if it exists.
pub(crate) async fn get_gvfs_root_gid(
    client: &GDriveClient,
    lexe_dir: &GFileId,
    gvfs_root_name: &str,
) -> anyhow::Result<Option<GFileId>> {
    let maybe_gvfs_root_gid = client
        .search_direct_children(lexe_dir, gvfs_root_name)
        .await
        .context("search_direct_children")?
        .map(|gfile| gfile.id);

    Ok(maybe_gvfs_root_gid)
}

#[cfg(test)]
mod test {
    use common::{api::user::UserPk, env::DeployEnv, ln::network::LxNetwork};

    use super::*;
    use crate::{ReqwestClient, oauth2::GDriveCredentials};

    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_lexe_dir --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_lexe_dir() {
        let client = ReqwestClient::new();
        let credentials = GDriveCredentials::from_env().unwrap();
        let (client, _rx) = GDriveClient::new(client, credentials);
        let lexe_dir = get_or_create_lexe_dir(&client).await.unwrap();
        let lexe_dir_name = &lexe_dir.name;
        println!("Lexe dir: {lexe_dir_name}");
        let lexe_dir_id = lexe_dir.id;

        let gvfs_root_name = GvfsRootName {
            deploy_env: DeployEnv::Dev,
            network: LxNetwork::Regtest,
            use_sgx: false,
            user_pk: UserPk::from_u64(123123),
        };
        println!("GvfsRootName: {gvfs_root_name}");

        let gvfs_root_gid =
            get_or_create_gvfs_root(&client, &lexe_dir_id, gvfs_root_name)
                .await
                .unwrap()
                .gid;
        println!("Gvfs root id: {gvfs_root_gid}");
    }
}
