//! Logic to conduct operations on multiple data backends at the same time.
//!
//! TODO(max): This module should eventually contain a multi-VSS abstraction.

use std::time::SystemTime;

use anyhow::Context;
use common::{aes::AesMasterKey, constants::IMPORTANT_PERSIST_RETRIES};
use gdrive::GoogleVfs;
use lexe_api::{
    auth::BearerAuthenticator,
    vfs::{VfsFile, VfsFileId},
};
use lexe_ln::persister;
use lexe_std::backoff;
use secrecy::Secret;

use crate::api::BackendApiClient;

/// Helper to read a VFS from both Google Drive and Lexe's DB.
/// The read from GDrive is skipped if `maybe_google_vfs` is [`None`].
pub(super) async fn read(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    vfs_master_key: &AesMasterKey,
    maybe_google_vfs: Option<&GoogleVfs>,
    file_id: &VfsFileId,
) -> anyhow::Result<Option<Secret<Vec<u8>>>> {
    let read_from_lexe = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .get_file(file_id, token)
            .await
            .with_context(|| format!("{file_id}"))
            .context("Couldn't fetch from Lexe")
    };

    let maybe_plaintext = match maybe_google_vfs {
        Some(gvfs) => {
            let read_from_google = async {
                gvfs.get_file(file_id)
                    .await
                    .with_context(|| format!("{file_id}"))
                    .context("Couldn't fetch from Google")
            };

            let (try_maybe_google_file, try_maybe_lexe_file) =
                tokio::join!(read_from_google, read_from_lexe);
            let maybe_google_file = try_maybe_google_file?;
            let maybe_lexe_file = try_maybe_lexe_file?;

            super::discrepancy::evaluate_and_resolve(
                backend_api,
                authenticator,
                vfs_master_key,
                gvfs,
                file_id,
                maybe_google_file,
                maybe_lexe_file.maybe_file,
            )
            .await
            .context("Evaluation and resolution failed")?
        }
        None => {
            let maybe_lexe_file = read_from_lexe.await?;
            maybe_lexe_file
                .maybe_file
                .map(|file| {
                    persister::decrypt_file(vfs_master_key, file_id, file)
                        .map(Secret::new)
                        .context("Failed to decrypt file")
                })
                .transpose()?
        }
    };

    Ok(maybe_plaintext)
}

/// Helper to upsert an important VFS file to both Google Drive and Lexe's DB.
///
/// - The upsert to GDrive is skipped if `maybe_google_vfs` is [`None`].
/// - Up to [`IMPORTANT_PERSIST_RETRIES`] additional attempts will be made if
///   the first attempt fails.
pub(super) async fn upsert(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    maybe_google_vfs: Option<&GoogleVfs>,
    file: VfsFile,
) -> anyhow::Result<()> {
    let google_upsert_future = async {
        match maybe_google_vfs {
            Some(gvfs) => {
                let mut try_upsert = gvfs
                    .upsert_file(file.clone())
                    .await
                    .context("(First attempt)");

                let mut backoff_iter = backoff::get_backoff_iter();
                for i in 0..IMPORTANT_PERSIST_RETRIES {
                    if try_upsert.is_ok() {
                        break;
                    }

                    tokio::time::sleep(backoff_iter.next().unwrap()).await;

                    try_upsert = gvfs
                        .upsert_file(file.clone())
                        .await
                        .with_context(|| format!("(Retry #{i})"));
                }

                try_upsert.context("Failed to upsert to GVFS")
            }
            None => Ok(()),
        }
    };
    let lexe_upsert_future = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .upsert_file_with_retries(&file, token, IMPORTANT_PERSIST_RETRIES)
            .await
            .map(|_| ())
            .context("Failed to upsert to Lexe DB")
    };

    // Since Google is the source of truth (and upserting to Google is more
    // likely to fail), do Google first. This introduces some latency but
    // prevents us from having to roll back Lexe's state if it fails.
    google_upsert_future.await?;
    lexe_upsert_future.await?;

    Ok(())
}

/// Helper to delete a VFS file from both Google Drive and Lexe's DB.
/// The deletion from GDrive is skipped if `maybe_google_vfs` is [`None`].
pub(super) async fn delete(
    backend_api: &(dyn BackendApiClient + Send + Sync),
    authenticator: &BearerAuthenticator,
    maybe_google_vfs: Option<&GoogleVfs>,
    file_id: &VfsFileId,
) -> anyhow::Result<()> {
    let delete_from_google = async {
        match maybe_google_vfs {
            Some(gvfs) => gvfs
                .delete_file(file_id)
                .await
                .with_context(|| format!("{file_id}"))
                .context("Couldn't delete from Google"),
            None => Ok(()),
        }
    };
    let delete_from_lexe = async {
        let token = authenticator
            .get_token(backend_api, SystemTime::now())
            .await
            .context("Could not get token")?;
        backend_api
            .delete_file(file_id, token)
            .await
            .with_context(|| format!("{file_id}"))
            .context("Couldn't delete from Lexe")
    };

    let (try_delete_google_file, try_delete_lexe_file) =
        tokio::join!(delete_from_google, delete_from_lexe);
    try_delete_google_file?;
    try_delete_lexe_file?;

    Ok(())
}
