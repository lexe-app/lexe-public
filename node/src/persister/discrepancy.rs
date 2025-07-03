//! A module which contains our discrepancy evaluation and resolution logic
//! between Google and Lexe VFS files. It'll be replaced once we switch to VSS.

use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

use anyhow::{anyhow, Context};
use common::{aes::AesMasterKey, debug_panic_release_log, Secret};
use gdrive::GoogleVfs;
use lexe_api::{
    auth::BearerAuthenticator,
    def::NodeBackendApi,
    vfs::{VfsFile, VfsFileId},
};
use lexe_ln::persister;
use tracing::{error, warn};

use crate::client::NodeBackendClient;

/// Given the [`Option<VfsFile>`]s returned by Google and Lexe, evaluates
/// whether there is a discrepancy between the two and resolves it if so.
/// Returns the decrypted plaintext bytes of the 'correct' [`VfsFile`].
///
/// NOTE: Passing [`None`] for a file MUST indicate that an API call to the data
/// store succeeded and positively confirms that that file does not exist, at
/// least according to the data store.
pub(super) async fn evaluate_and_resolve(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    gvfs: &GoogleVfs,
    file_id: &VfsFileId,
    maybe_google_file: Option<VfsFile>,
    maybe_lexe_file: Option<VfsFile>,
) -> anyhow::Result<Option<Secret<Vec<u8>>>> {
    if maybe_google_file == maybe_lexe_file {
        // Encrypted files match, therefore the contents match.
        // Proceed to decrypt either one of them.
        // Returns Ok(None) if the files were None.
        return maybe_google_file
            .map(|file| {
                persister::decrypt_file(vfs_master_key, file_id, file)
                    .map(Secret::new)
                    .with_context(|| format!("Decryption failed: {file_id}"))
            })
            .transpose();
    }
    // Uh oh! There was a discrepancy between Google and Lexe.
    // We'll have to do extra work to fix it.

    // Decrypt the VFS files, thereby validating and authenticating them.
    // If the file failed to decrypt, assume the file is corrupt.
    let mut google_corrupt = false;
    let mut lexe_corrupt = false;
    let maybe_google_bytes = maybe_google_file.clone().and_then(|file| {
        persister::decrypt_file(vfs_master_key, file_id, file)
            .map(Secret::new)
            .inspect_err(|_| google_corrupt = true)
            .inspect_err(|e| error!("Google file corrupt: {file_id}: {e:#}"))
            .ok()
    });
    let maybe_lexe_bytes = maybe_lexe_file.clone().and_then(|file| {
        persister::decrypt_file(vfs_master_key, file_id, file)
            .map(Secret::new)
            .inspect_err(|_| lexe_corrupt = true)
            .inspect_err(|e| error!("Lexe file corrupt: {file_id}: {e:#}"))
            .ok()
    });

    let correct_bytes = match (maybe_google_bytes, maybe_lexe_bytes) {
        (Some(google_bytes), Some(_lexe_bytes)) => {
            // Google and Lexe have valid but non-matching files. This could
            // happen if persistence partially failed, updating one copy but not
            // the other. GDrive offers rollback protection, so it is the
            // primary source of truth. We'll update Lexe with Google's version.
            warn!("Google-Lexe file mismatch: {file_id}");

            let token = authenticator
                .get_token(backend_api, SystemTime::now())
                .await
                .context("Could not get auth token")?;
            let correct_file = maybe_google_file.expect("google_bytes is Some");

            backend_api
                .upsert_file(&correct_file, token)
                .await
                .context("Failed to update Lexe's version")?;

            Some(google_bytes)
        }
        (Some(google_bytes), None) => {
            // A valid file was found in GDrive but not in Lexe's DB.
            //
            // - For channel managers, which are never deleted, this should
            //   basically never happen unless Lexe somehow lost their data or
            //   the user requested to delete their account but somehow added
            //   the data back to their My Drive.
            // - For channel monitors, it's possible that there was a partial
            //   failure during channel monitor deletion.
            //
            // In either case we'll log an error and copy the data to Lexe's DB.
            error!("File found in GDrive but not in Lexe's DB: {file_id}");

            let token = authenticator
                .get_token(backend_api, SystemTime::now())
                .await
                .context("Could not get auth token")?;
            let correct_file = maybe_google_file.expect("google_bytes is Some");

            // Update Lexe's version if it was corrupt, otherwise create it.
            if lexe_corrupt {
                backend_api
                    .upsert_file(&correct_file, token)
                    .await
                    .context("Failed to update Lexe's version")?;
            } else {
                backend_api
                    .create_file(&correct_file, token)
                    .await
                    .context("Failed to create Lexe's version")?;
            }

            Some(google_bytes)
        }
        (None, Some(lexe_bytes)) => {
            // A valid file was found in Lexe's DB but not in GDrive.
            //
            // - Channel manager: Lexe has a valid channel manager in its DB,
            //   yet none was returned from a successful API call to Google. In
            //   other words, the user exists but they are missing *critical*
            //   information in their Google Drive.
            //
            //   We assume that the user has lost their backup (by modifying or
            //   deleting the LexeData dir in My Drive despite all the warnings
            //   to the contrary) and needs help with funds recovery. At this
            //   point, the user has no choice but to use the channel manager
            //   from Lexe's DB (which may have been rolled back).
            //
            // - Channel monitor: This case could be triggered by a partial
            //   failure during channel monitor archiving / deletion. Since it
            //   should be rare that an API call to Google succeeds but the same
            //   API call to Lexe doesn't, we should restore the monitor from
            //   Lexe's version in case the user deleted their monitor from
            //   their GDrive on accident.
            //
            //   NOTE: if a malicious Lexe is intentionally trying to resurface
            //   a previously-deleted monitor, that 'attack' doesn't actually
            //   accomplish anything.
            //
            // In either case, we'll make some noise then copy Lexe's version to
            // the user's GDrive.
            error!("File found in Lexe's DB but not in GDrive: {file_id}");

            let correct_file = maybe_lexe_file.expect("lexe_bytes is Some");

            // Update Google's version if it was corrupt, otherwise create it.
            if google_corrupt {
                gvfs.upsert_file(correct_file)
                    .await
                    .context("Failed to update Google's version")?;
            } else {
                gvfs.create_file(correct_file)
                    .await
                    .context("Failed to restore Google's version")?;
            }

            Some(lexe_bytes)
        }
        (None, None) => {
            // Neither Google nor Lexe have a valid version of this file.
            // Since the early return covered the `(None, None)` case, at least
            // one of the files is corrupt. It is possible that Lexe introduced
            // some backwards-incompatible change to its E2EE or AES derivation
            // scheme which caused decryption to fail. To avoid deleting the
            // manager and monitors in this case, we'll just return an error.
            assert!(google_corrupt || lexe_corrupt);
            return Err(anyhow!(
                "All files corrupt. Did we make a backwards-incompatible change
                 to our AES key encryption and/or encryption scheme? {file_id}"
            ));
        }
    };

    Ok(correct_bytes)
}

/// Given all files returned by Google and Lexe in a given VFS directory,
/// evaluates and resolves any discrepancies between them.
/// Returns the decrypted ([`VfsFileId`], [`Vec<u8>`]) plaintext pairs.
// TODO(max): Keep around until we switch to VSS, at which point we *may* reuse
// some of this code.
#[allow(dead_code)]
pub(super) async fn evaluate_and_resolve_all(
    backend_api: &NodeBackendClient,
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    gvfs: &GoogleVfs,
    google_files: Vec<VfsFile>,
    lexe_files: Vec<VfsFile>,
) -> anyhow::Result<Vec<(VfsFileId, Secret<Vec<u8>>)>> {
    // HashMap (file_id -> file) for Google and Lexe files respectively
    let mut google_map = google_files
        .into_iter()
        .map(|file| (file.id.clone(), file))
        .collect::<HashMap<VfsFileId, VfsFile>>();
    let mut lexe_map = lexe_files
        .into_iter()
        .map(|file| (file.id.clone(), file))
        .collect::<HashMap<VfsFileId, VfsFile>>();

    // Deduplicated HashSet of all `file_id`s contained in either map
    let all_file_ids = google_map
        .keys()
        .chain(lexe_map.keys())
        .cloned()
        .collect::<HashSet<VfsFileId>>();

    // Convert to (file_id, Option<google_file>, Option<lexe_file>)
    let combined = all_file_ids.into_iter().map(|file_id| {
        let maybe_google_file = google_map.remove(&file_id);
        let maybe_lexe_file = lexe_map.remove(&file_id);
        (file_id, maybe_google_file, maybe_lexe_file)
    });

    // Map each tuple to a future to evaluate and resolve discrepancies.
    let resolution_futures = combined.map(
        |(file_id, maybe_google_file, maybe_lexe_file)| async move {
            let plaintext = evaluate_and_resolve(
                backend_api,
                authenticator,
                vfs_master_key,
                gvfs,
                &file_id,
                maybe_google_file,
                maybe_lexe_file,
            )
            .await
            .with_context(|| format!("{file_id}"))?;

            anyhow::Ok((file_id, plaintext))
        },
    );

    // Execute the futures concurrently, collect results
    let mut plaintext_pairs = Vec::new();
    let mut err_msgs = Vec::new();
    for result in futures::future::join_all(resolution_futures).await {
        match result {
            Ok((file_id, maybe_plaintext)) => match maybe_plaintext {
                Some(plaintext) => plaintext_pairs.push((file_id, plaintext)),
                // At least one of the files given to `evaluate_and_resolve` is
                // Some, so the output should always be Some. But it is possible
                // that `evaluate_and_resolve`'s logic will change later.
                None => debug_panic_release_log!(
                    "Resolution resulted in no plaintext: {file_id}"
                ),
            },
            Err(e) => err_msgs.push(format!("{e:#}")),
        }
    }

    // Returns the decrypted plaintext (file_id, plaintext) pairs.
    if err_msgs.is_empty() {
        Ok(plaintext_pairs)
    } else {
        let joined_msg = err_msgs.join("; ");
        Err(anyhow!("Resolution failed: {joined_msg}"))
    }
}
