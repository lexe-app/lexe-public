//! A module which contains our discrepancy evaluation and resolution logic
//! between Google and Lexe VFS files. It'll be replaced once we switch to VSS.

use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

use anyhow::Context;
use common::{
    aes::AesMasterKey,
    api::{
        auth::BearerAuthenticator,
        vfs::{VfsFile, VfsFileId},
    },
    Secret,
};
use gdrive::GoogleVfs;
use lexe_ln::persister;
use tracing::{debug, error, info, warn};

use crate::api::BackendApiClient;

/// A [`VfsFile`] discrepancy between Google and Lexe which should be resolved.
struct VfsFileDiscrepancy<'a> {
    /// The kind of action that should be taken to resolve this discrepancy.
    action: ResolutionAction,
    /// The "correct" [`VfsFile`]. Other versions should be updated to this.
    correct_file: &'a VfsFile,
}

#[derive(Debug)]
enum ResolutionAction {
    /// Both Google and Lexe contained the file.
    /// Lexe's version should be updated to Google's version.
    UpdateLexeVersion,
    /// Lexe didn't have the file; it should be restored from Google's version.
    CreateLexeFile,
    /// Google didn't have the file; it should be restored from Lexe's version.
    CreateGoogleFile,
}

impl<'a> VfsFileDiscrepancy<'a> {
    /// Resolves the discrepancy by creating or updating as required.
    pub(super) async fn resolve(
        &self,
        backend_api: &(dyn BackendApiClient + Send + Sync),
        authenticator: &BearerAuthenticator,
        gvfs: &GoogleVfs,
    ) -> anyhow::Result<()> {
        let kind = &self.action;
        let file_id = &self.correct_file.id;
        info!(?kind, %file_id, "Resolving discrepancy");

        let resolve_future = async {
            match self.action {
                ResolutionAction::UpdateLexeVersion => {
                    let token = authenticator
                        .get_token(backend_api, SystemTime::now())
                        .await
                        .context("Could not get auth token")?;
                    backend_api
                        .upsert_file(self.correct_file, token)
                        .await
                        .context("Failed to update Lexe's version")?;
                }
                ResolutionAction::CreateLexeFile => {
                    let token = authenticator
                        .get_token(backend_api, SystemTime::now())
                        .await
                        .context("Could not get auth token")?;
                    backend_api
                        .create_file(self.correct_file, token)
                        .await
                        .context("Failed to restore Lexe's version")?;
                }
                ResolutionAction::CreateGoogleFile => {
                    gvfs.create_file(self.correct_file.clone())
                        .await
                        .context("Failed to restore Google's version")?;
                }
            }

            anyhow::Ok(())
        };

        resolve_future
            .await
            .inspect(|()| info!(?kind, %file_id, "Discrepancy resolved."))
            .with_context(|| format!("{kind:?} {file_id}",))
            .context("Failed to resolve discrepancy")
    }
}

/// Given the [`Option<VfsFile>`]s for the channel manager returned to us by
/// both Google and Lexe, get the decrypted channel manager plaintext bytes.
///
/// - If the files are the same, we decrypt either one and return the bytes.
/// - If the files are different, or one exists and the other doesn't, we'll
///   reason about likely scenarios and resolve the discrepancy accordingly.
/// - If no files were returned, we return [`None`].
///
/// NOTE: Passing [`None`] for a file MUST indicate that an API call to the data
/// store succeeded and positively confirms that that file does not exist.
pub(super) async fn evaluate_and_resolve_channel_manager(
    backend_api: &(dyn BackendApiClient + Send + Sync),
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
        // Note that both managers being None is an accepted case.
        return maybe_google_file
            .map(|file| {
                persister::decrypt_file(vfs_master_key, file_id, file)
                    .map(Secret::new)
                    .context("Decryption failed")
            })
            .transpose();
    }
    // Uh oh! There was a discrepancy between Google and Lexe.
    // We'll have to do extra work to fix it.

    // Decrypt the VFS files, thereby validating and authenticating them. We
    // clone before decryption so that we don't have to re-encrypt the files
    // later when we fix the discrepancy.
    let maybe_google_bytes = maybe_google_file
        .clone()
        .map(|file| {
            persister::decrypt_file(vfs_master_key, file_id, file)
                .map(Secret::new)
                .context("Failed to decrypt file from Google")
        })
        .transpose()?;
    let maybe_lexe_bytes = maybe_lexe_file
        .clone()
        .map(|file| {
            persister::decrypt_file(vfs_master_key, file_id, file)
                .map(Secret::new)
                .context("Failed to decrypt file from Lexe")
        })
        .transpose()?;

    // Evaluate and determine how we will fix the discrepancy.
    let discrepancy = match (&maybe_google_file, &maybe_lexe_file) {
        (Some(google_file), Some(_lexe_file)) => {
            // The file was found in both GDrive and in Lexe's DB, but they
            // were different. This could happen if a channel manager
            // persist has a partial failure, updating one copy but not the
            // other. GDrive offers rollback protection, so it is the
            // primary source of truth. Update Lexe with the Google version.
            warn!("Google-Lexe manager mismatch");
            VfsFileDiscrepancy {
                action: ResolutionAction::UpdateLexeVersion,
                correct_file: google_file,
            }
        }
        (Some(google_file), None) => {
            // The channel manager was found in GDrive but not in Lexe's DB.
            // This should basically never happen unless (1) Lexe somehow
            // lost their data or (2) the user requested to delete their
            // account but somehow added the data back to their My Drive.
            // Let's make some noise and copy the data into Lexe's DB.
            error!(
                "Channel manager found in gdrive but not in Lexe's DB; /
                copying to Lexe's DB"
            );
            VfsFileDiscrepancy {
                action: ResolutionAction::CreateLexeFile,
                correct_file: google_file,
            }
        }
        (None, Some(lexe_file)) => {
            // Lexe has a valid channel manager in its DB, yet none was
            // returned from a successful API call to Google.
            // In other words, the user exists but they are missing
            // *critical* information in their Google Drive.
            //
            // We assume that the user has lost their backup (by deleting
            // the LexeData dir in My Drive despite all the "DO NOT DELETE"
            // warnings to the contrary) and needs help with funds recovery.
            // At this point, the user has no choice but to use the channel
            // manager from Lexe's DB (which may have been rolled back).
            //
            // We'll make some noise then copy Lexe's channel manager back
            // into the user's Google Drive.
            error!(
                "Channel manager found in Lexe's DB but not in \
                Google Drive. The user appears to have lost their \
                backup. To prevent potential funds loss, we will \
                recover using the channel manager from Lexe's DB."
            );
            VfsFileDiscrepancy {
                action: ResolutionAction::CreateGoogleFile,
                correct_file: lexe_file,
            }
        }
        (None, None) => unreachable!("Early exit checked for equality"),
    };

    discrepancy
        .resolve(backend_api, authenticator, gvfs)
        .await
        .context("Failed to resolve channel manager discrepancy")?;

    // For security, the GDrive version always takes precedence.
    Ok(maybe_google_bytes.or(maybe_lexe_bytes))
}

/// Compares all channel monitor files returned by both Google and Lexe,
/// returning a [`Vec<VfsFile>`] considered to be the "correct" set of monitors.
///
/// If Google and Lexe returned different results, we'll reason about likely
/// scenarios in each case and resolve the discrepancies accordingly.
///
/// The logic is very similar to [`evaluate_and_resolve_channel_manager`], but
/// we should resist the urge to extract a function that abstracts over both,
/// since there are nuanced differences between them with important security
/// implications.
pub(super) async fn evaluate_and_resolve_all_monitors(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    gvfs: &GoogleVfs,
    mut google_files: Vec<VfsFile>,
    lexe_files: Vec<VfsFile>,
) -> anyhow::Result<Vec<VfsFile>> {
    // Build a hashmap (&file_id -> &file) for both
    let google_map = google_files
        .iter()
        .map(|file| (&file.id, file))
        .collect::<HashMap<&VfsFileId, &VfsFile>>();
    let lexe_map = lexe_files
        .iter()
        .map(|file| (&file.id, file))
        .collect::<HashMap<&VfsFileId, &VfsFile>>();

    // Early exit if Google and Lexe returned the same (encrypted) data.
    if google_map == lexe_map {
        return Ok(google_files);
    }
    // Uh oh! There was a discrepancy between Google's and Lexe's versions.
    // Since GDrive has rollback protection it is the primary source
    // of truth; fix any discrepancies by updating Lexe's version.

    // A hashset of all `&VfsFileId`s.
    let all_file_ids = google_map
        .keys()
        .chain(lexe_map.keys())
        .copied()
        .collect::<HashSet<&VfsFileId>>();

    // Need these variables since we iterate over refs into the original Vecs
    let mut to_replace_in_lexe_files = HashMap::<VfsFileId, VfsFile>::new();
    let mut to_append_to_lexe_files = Vec::<VfsFile>::new();
    let mut to_append_to_google_files = Vec::<VfsFile>::new();

    // Iterate through all known file ids and fix any discrepancies.
    for file_id in all_file_ids {
        // Evaluate whether there is a discrepancy for the files at this path
        let maybe_google_monitor = google_map.get(&file_id).copied();
        let maybe_lexe_monitor = lexe_map.get(&file_id).copied();
        let maybe_discrepancy = evaluate_monitor_pair(
            file_id,
            maybe_google_monitor,
            maybe_lexe_monitor,
        );

        if let Some(discrepancy) = maybe_discrepancy {
            // Fix the discrepancy.
            discrepancy
                .resolve(backend_api, authenticator, gvfs)
                .await?;

            // Update the intermediate vars with the created / updated file.
            match discrepancy.action {
                ResolutionAction::UpdateLexeVersion => {
                    to_replace_in_lexe_files.insert(
                        file_id.clone(),
                        discrepancy.correct_file.clone(),
                    );
                }
                ResolutionAction::CreateLexeFile => to_append_to_lexe_files
                    .push(discrepancy.correct_file.clone()),
                ResolutionAction::CreateGoogleFile => to_append_to_google_files
                    .push(discrepancy.correct_file.clone()),
            }
        }
    }

    // Update the original `Vec<VfsFile>`s as needed.
    let mut lexe_files = lexe_files
        .into_iter()
        .map(|file| match to_replace_in_lexe_files.remove(&file.id) {
            Some(updated_file) => updated_file,
            None => file,
        })
        .collect::<Vec<VfsFile>>();
    lexe_files.extend(to_append_to_lexe_files);
    google_files.extend(to_append_to_google_files);

    // All discrepancies have been fixed. Return the files.
    Ok(google_files)
}

/// Given a single pair of channel monitor files returned by Google and Lexe,
/// determines whether there is a [`VfsFileDiscrepancy`] between the two.
///
/// Panics if any [`VfsFile::id`] doesn't match the given `file_id`.
fn evaluate_monitor_pair<'file>(
    file_id: &VfsFileId,
    maybe_google_monitor: Option<&'file VfsFile>,
    maybe_lexe_monitor: Option<&'file VfsFile>,
) -> Option<VfsFileDiscrepancy<'file>> {
    match (maybe_google_monitor, maybe_lexe_monitor) {
        (Some(google_file), Some(lexe_file)) => {
            // Both Google and Lexe contained the file.
            assert_eq!(file_id, &google_file.id);
            assert_eq!(file_id, &lexe_file.id);
            if google_file == lexe_file {
                debug!("Monitor pair OK: {file_id}");
                None
            } else {
                warn!("Google-Lexe monitor mismatch: {file_id}");
                // Update Lexe's version to Google's version.
                // TODO(max): We can be smarter and deserialize / decrypt both,
                // then compare the blockhash or something. But we'll probably
                // switch to VSS before that happens.
                Some(VfsFileDiscrepancy {
                    action: ResolutionAction::UpdateLexeVersion,
                    correct_file: google_file,
                })
            }
        }
        (Some(google_file), None) => {
            // Lexe didn't have the file. Copy Google's version to Lexe.
            assert_eq!(file_id, &google_file.id);
            error!("Lexe DB is missing a monitor: {file_id}");
            Some(VfsFileDiscrepancy {
                action: ResolutionAction::CreateLexeFile,
                correct_file: google_file,
            })
        }
        (None, Some(lexe_file)) => {
            // Google didn't have the file; restore from Lexe's version.
            // This case could be triggered by a deletion race where
            // deleting from Google succeeds but the node crashes before
            // it could be deleted from Lexe's DB. Since this is rare,
            // we should restore the monitor from Lexe's version in case
            // the deletion from Google was done on accident.
            // NOTE: if a malicious Lexe is intentionally trying to
            // resurface a previously-deleted monitor, that 'attack'
            // doesn't actually accomplish anything.
            assert_eq!(file_id, &lexe_file.id);
            error!("Restoring monitor from Lexe: {file_id}");
            Some(VfsFileDiscrepancy {
                action: ResolutionAction::CreateGoogleFile,
                correct_file: lexe_file,
            })
        }
        (None, None) => {
            // A weird case which probably isn't intended by the caller.
            // Let's not panic in prod if it happens though.
            warn!("Both monitors to compare are None? {file_id}");
            // Currently does nothing since this fn only runs in staging/prod.
            // Maybe that'll change one day, idk.
            debug_assert!(false);
            None
        }
    }
}
