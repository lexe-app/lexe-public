//! Node/LSP VFS migrations.
//!
//! Similar to database migrations but for VFS-persisted state. Each migration
//! creates an empty marker file in `migrations/<name>` when complete.

use std::collections::HashSet;

use anyhow::Context;
use bytes::Bytes;
use lexe_api::{
    types::Empty,
    vfs::{self, Vfs, VfsDirectory, VfsFileId},
};
use tracing::debug;

/// Marker filename for the payments_v2 migration.
pub const MARKER_PAYMENTS_V2: &str = "payments_v2";

/// Marker for wallets created before node-v0.8.12 that use legacy derivation.
///
/// If this marker exists, we need to check for funds in the legacy wallet and
/// sweep them to the new BIP39-compatible wallet.
pub const MARKER_LEGACY_BDK: &str = "legacy_bdk";

/// Tracks which node/LSP migrations have been applied.
///
/// Initialized at startup by reading all filenames from the `migrations/`
/// directory. Each applied migration has an empty marker file.
pub struct Migrations {
    applied: HashSet<String>,
}

impl Migrations {
    /// Read all applied migrations from the VFS.
    pub async fn read(vfs: &impl Vfs) -> anyhow::Result<Self> {
        let dir = VfsDirectory::new(vfs::MIGRATIONS_DIR);
        let dir_list = vfs
            .list_directory(&dir)
            .await
            .context("Failed to list migrations dir")?;

        let applied: HashSet<String> = dir_list.filenames.into_iter().collect();
        debug!(?applied, "migrations");

        Ok(Self { applied })
    }

    /// Mark a migration as applied by writing an empty marker file.
    pub async fn mark_applied(
        vfs: &(impl Vfs + ?Sized),
        name: &'static str,
    ) -> anyhow::Result<()> {
        let file_id = VfsFileId::new(vfs::MIGRATIONS_DIR, name);
        let data = Bytes::new();
        let retries = 1;
        vfs.upsert_file(&file_id, data, retries)
            .await
            .map(|Empty {}| ())
            .with_context(|| {
                format!("Failed to mark migration '{name}' as applied")
            })?;
        Ok(())
    }

    /// Check if a migration has been applied.
    pub fn is_applied(&self, name: &str) -> bool {
        self.applied.contains(name)
    }
}
