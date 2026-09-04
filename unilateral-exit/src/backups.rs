//! Imports the user's downloaded backup files into the recovery dir.
//!
//! Backup files in the user's Google Drive are flat files named
//! `<dirname>/<filename>`. On download, Drive replaces the `/` with `_` for
//! individual downloads and zips, or with ` ` for Drive desktop sync. However,
//! `<dirname>/<filename>` is the ciphertext's AAD, so we recover it from the
//! mangled filename and persist the blob into the recovery dir under it.
//!
//! LDK archives a monitor only once all of its funds are claimed, so archived
//! monitors are not imported.
//!
//! Local state persisted by a previous recovery run is newer than the provided
//! backups, so files already present in the recovery dir take precedence.
//
// TODO(max): Fix the Drive file naming so downloads aren't mangled. The
// `./channel_manager` singleton fares worst. macOS Archive Utility treats its
// zip entry `._channel_manager` as AppleDouble metadata and silently drops it,
// and Finder hides it as a dotfile when extracted with Finder or CLI `unzip`.
//
// Target scheme:
// - VFS dirnames and filenames use only ASCII alphanumerics, `_`, and `-`, with
//   no repeated `--`/`__` and no leading or trailing `-`/`_`. All names in use
//   today already comply.
// - Drive names flatten to `{dirname}--{filename}`, or a bare `{filename}` for
//   singletons. The VFS `.` dirname, and thus the AAD, stays unchanged.

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, ensure};
use lexe_api::{
    types::retries::Retries,
    vfs::{self, Vfs, VfsFile, VfsFileId},
};
use lexe_common::ln::channel::LxMonitorName;
use tracing::info;

use crate::persister::RecoveryPersister;

/// Imports the provided backup files into the recovery dir.
pub(crate) async fn import(
    persister: &RecoveryPersister,
    channel_manager_path: Option<&Path>,
    monitor_paths: &[PathBuf],
) -> anyhow::Result<()> {
    if let Some(path) = channel_manager_path {
        let file_id = VfsFileId::new(
            vfs::SINGLETON_DIRECTORY,
            vfs::CHANNEL_MANAGER_FILENAME,
        );
        import_file(persister, path, file_id)
            .await
            .with_context(|| path.display().to_string())
            .context("Failed to import channel manager backup")?;
    }

    for path in monitor_paths {
        import_monitor(persister, path)
            .await
            .with_context(|| path.display().to_string())
            .context("Failed to import channel monitor backup")?;
    }

    Ok(())
}

async fn import_monitor(
    persister: &RecoveryPersister,
    path: &Path,
) -> anyhow::Result<()> {
    let basename = path
        .file_name()
        .context("Path has no filename")?
        .to_string_lossy();
    // An archived monitor's funds are all claimed, so skip.
    if basename.starts_with(vfs::CHANNEL_MONITORS_ARCHIVE_DIR) {
        info!("Skipping archived channel monitor backup");
        return Ok(());
    }
    let name = monitor_name(&basename).context(
        "Couldn't parse a channel monitor name from this filename. \
         Expected a Google Drive backup named `channel_monitors_<name>`; \
         if the file was renamed when downloading, rename it back.",
    )?;
    let file_id = VfsFileId::new(vfs::CHANNEL_MONITORS_DIR, name.to_string());
    import_file(persister, path, file_id).await
}

/// Persists the backup file at `path` into the recovery dir under `file_id`,
/// the id its ciphertext is bound to.
async fn import_file(
    persister: &RecoveryPersister,
    path: &Path,
    file_id: VfsFileId,
) -> anyhow::Result<()> {
    if persister.read_file(&file_id).await?.is_some() {
        let path = path.display();
        info!(
            %file_id,
            "Recovery dir already has this file; ignoring {path}, \
             which can now be deleted"
        );
        return Ok(());
    }

    let data = tokio::fs::read(path).await.context("Failed to read file")?;
    ensure!(!data.is_empty(), "Refusing to persist an empty backup file");

    // Check that the file decrypts, so a wrong file fails fast.
    let file = VfsFile {
        id: file_id.clone(),
        data: data.clone(),
    };
    persister
        .decrypt_file(&file_id, file)
        .context("Couldn't decrypt this backup under this root seed")?;

    let retries = Retries::from_count(1);
    persister
        .upsert_file(&file_id, data.into(), retries)
        .await
        .with_context(|| file_id.to_string())
        .context("Failed to persist imported file")?;

    info!(%file_id, "Imported backup file");
    Ok(())
}

/// Parses a monitor's name from its downloaded filename.
fn monitor_name(basename: &str) -> Option<LxMonitorName> {
    // Strip the dirname and the character substituted for the `/`.
    let mut rest = basename.strip_prefix(vfs::CHANNEL_MONITORS_DIR)?.chars();
    let _substituted_slash = rest.next()?;
    LxMonitorName::from_str(rest.as_str()).ok()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn monitor_name_handles_drive_downloads() {
        let txid = "0123456789abcdef".repeat(4);
        let name = format!("{txid}_1");

        // Individual downloads and zips replace the `/` in
        // `channel_monitors/<name>` with `_`; Drive desktop sync with ` `.
        for substituted in ['_', ' '] {
            let basename = format!("channel_monitors{substituted}{name}");
            assert_eq!(monitor_name(&basename).unwrap().to_string(), name);
        }
    }
}
