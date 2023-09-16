//! Handles everything related to the structure of the LexeData dir.
//!
//! The canonical folder structure is as follows:
//!
//! My Drive
//! |___... (The LexeData dir can be moved anywhere in My Drive)
//!     |___"X LexeData (DO NOT RENAME, MODIFY, OR DELETE)"
//!         |___bitcoin (mainnet vfs root)
//!         |   |___encrypted_root_seed (non-vfs file)
//!         |   |___channel_manager (vfs file)
//!         |   |___channel_monitors (vfs directory)
//!         |       |___deadbeef (vfs file)
//!         |       |___...
//!         |___testnet (testnet vfs root)
//!         |   |___encrypted_root_seed (non-vfs file)
//!         |   |___channel_manager (vfs file)
//!         |   |___channel_monitors (vfs dir)
//!         |       |___...
//!         |___...
//!
//! See also the doc comments for `LEXE_DIR_NAME`.

use anyhow::Context;
use common::cli::Network;
use tracing::{debug, warn};

use crate::{
    api,
    api::GDriveClient,
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

/// Given the [`GFileId`] of the parent LexeData dir, returns the [`GFileId`]
/// corresponding to the VFS root. The VFS root is created if it didn't exist.
pub(crate) async fn get_or_create_vfs_root(
    client: &GDriveClient,
    lexe_dir: &GFileId,
    network: Network,
) -> anyhow::Result<GFileId> {
    let network_str = network.to_string();
    let maybe_vfs_root = get_vfs_root(client, lexe_dir, &network_str)
        .await
        .context("get_vfs_root")?;

    let vfs_root = match maybe_vfs_root {
        Some(gid) => gid,
        None => client
            .create_child_dir(lexe_dir.clone(), &network_str)
            .await
            .context("create_child_dir")?,
    };

    Ok(vfs_root)
}

/// Given the [`GFileId`] of the parent LexeData dir, returns the [`GFileId`]
/// corresponding to the VFS root, if it exists.
pub(crate) async fn get_vfs_root(
    client: &GDriveClient,
    lexe_dir: &GFileId,
    network_str: &str,
) -> anyhow::Result<Option<GFileId>> {
    let maybe_vfs_root = client
        .search_direct_children(lexe_dir, network_str)
        .await
        .context("search_direct_children")?
        .map(|gfile| gfile.id);

    Ok(maybe_vfs_root)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::oauth2::ApiCredentials;

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
        let credentials = ApiCredentials::from_env().unwrap();
        let client = GDriveClient::new(credentials);
        let lexe_dir = get_or_create_lexe_dir(&client).await.unwrap();
        let lexe_dir_name = &lexe_dir.name;
        println!("Lexe dir: {lexe_dir_name}");
        let network = Network::REGTEST;
        let lexe_dir_id = lexe_dir.id;
        let vfs_root_id =
            get_or_create_vfs_root(&client, &lexe_dir_id, network)
                .await
                .unwrap();
        println!("Vfs root id: {vfs_root_id}");
    }
}
