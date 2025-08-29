//! Virtual File System ('vfs')
//!
//! Our "virtual file system" is a simple way to represent a key-value store
//! with optional namespacing by "directory". You can think of the `vfs` as a
//! local directory that can contain files or directories, but where the
//! directories cannot contain other directories (no nesting).
//!
//! Any file can be uniquely identified by its `<dirname>/<filename>`, and all
//! files exclusively contain only binary data [`Vec<u8>`].
//!
//! Singleton objects like the channel manager are stored in the global
//! namespace, e.g. at `./channel_manager` or `./bdk_wallet_db`
//!
//! Growable or shrinkable collections of objects (e.g. channel monitors), are
//! stored in their own "directory", e.g. `channel_monitors/<funding_txo>`.

use std::{
    borrow::Cow,
    fmt::{self, Display},
    io::Cursor,
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use common::serde_helpers::hexstr_or_bytes;
use lightning::util::ser::{MaybeReadable, ReadableArgs, Writeable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{error::BackendApiError, types::Empty};

// --- Constants --- //

/// The vfs directory name used by singleton objects.
pub const SINGLETON_DIRECTORY: &str = ".";

pub const EVENTS_DIR: &str = "events";
pub const CHANNEL_MONITORS_DIR: &str = "channel_monitors";
pub const CHANNEL_MONITORS_ARCHIVE_DIR: &str = "channel_monitors_archive";

pub const CHANNEL_MANAGER_FILENAME: &str = "channel_manager";
pub const PW_ENC_ROOT_SEED_FILENAME: &str = "password_encrypted_root_seed";
// Filename history:
// - "bdk_wallet_db" for our pre BDK 1.0 wallet DB.
// - "bdk_wallet_db_v1" for our BDK 1.0.0-alpha.X wallet DB.
// - "bdk_wallet_changeset" since BDK 1.0.0-beta.X.
pub const WALLET_CHANGESET_FILENAME: &str = "bdk_wallet_changeset";

pub static REVOCABLE_CLIENTS_FILE_ID: VfsFileId =
    VfsFileId::new_const(SINGLETON_DIRECTORY, "revocable_clients");

// --- Trait --- //

/// Lexe's async persistence interface.
// TODO(max): We'll eventually move all usage of this to VSS.
#[async_trait]
pub trait Vfs {
    // --- Required methods --- //

    /// Fetch the given [`VfsFile`] from the backend.
    ///
    /// Prefer [`Vfs::read_file`] which adds logging and error context.
    async fn get_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Option<VfsFile>, BackendApiError>;

    /// Upsert the given [`VfsFile`] to the backend with the given # of retries.
    ///
    /// Prefer [`Vfs::persist_file`] which adds logging and error context.
    async fn upsert_file(
        &self,
        file: &VfsFile,
        retries: usize,
    ) -> Result<Empty, BackendApiError>;

    /// Deletes the [`VfsFile`] with the given [`VfsFileId`] from the backend.
    ///
    /// Prefer [`Vfs::remove_file`] which adds logging and error context.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
    ) -> Result<Empty, BackendApiError>;

    /// Fetches all files in the given [`VfsDirectory`] from the backend.
    ///
    /// Prefer [`Vfs::read_dir_files`] which adds logging and error context.
    async fn get_directory(
        &self,
        dir: &VfsDirectory,
    ) -> Result<Vec<VfsFile>, BackendApiError>;

    /// Serialize a LDK [`Writeable`] then encrypt it under the VFS master key.
    fn encrypt_ldk_writeable<W: Writeable>(
        &self,
        file_id: VfsFileId,
        writeable: &W,
    ) -> VfsFile;

    /// Serialize `T` then encrypt it to a file under the VFS master key.
    fn encrypt_json<T: Serialize>(
        &self,
        file_id: VfsFileId,
        value: &T,
    ) -> VfsFile;

    /// Decrypt a file previously encrypted under the VFS master key.
    fn decrypt_file(
        &self,
        expected_file_id: &VfsFileId,
        file: VfsFile,
    ) -> anyhow::Result<Vec<u8>>;

    // --- Provided methods --- //

    /// Reads, decrypts, and JSON-deserializes a type `T` from the DB.
    async fn read_json<T: DeserializeOwned>(
        &self,
        file_id: &VfsFileId,
    ) -> anyhow::Result<Option<T>> {
        let json_bytes = match self.read_bytes(file_id).await? {
            Some(bytes) => bytes,
            None => return Ok(None),
        };
        let value = serde_json::from_slice(json_bytes.as_slice())
            .with_context(|| format!("{file_id}"))
            .context("JSON deserialization failed")?;
        Ok(Some(value))
    }

    /// Reads, decrypts, and JSON-deserializes a [`VfsDirectory`] of type `T`.
    async fn read_dir_json<T: DeserializeOwned>(
        &self,
        dir: &VfsDirectory,
    ) -> anyhow::Result<Vec<(VfsFileId, T)>> {
        let ids_and_bytes = self.read_dir_bytes(dir).await?;
        let mut ids_and_values = Vec::with_capacity(ids_and_bytes.len());
        for (file_id, bytes) in ids_and_bytes {
            let value = serde_json::from_slice(bytes.as_slice())
                .with_context(|| format!("{file_id}"))
                .context("JSON deserialization failed (in dir)")?;
            ids_and_values.push((file_id, value));
        }
        Ok(ids_and_values)
    }

    /// Reads, decrypts, and deserializes a LDK [`ReadableArgs`] of type `T`
    /// with read args `A` from the DB.
    async fn read_readableargs<T, A>(
        &self,
        file_id: &VfsFileId,
        read_args: A,
    ) -> anyhow::Result<Option<T>>
    where
        T: ReadableArgs<A>,
        A: Send,
    {
        let bytes = match self.read_bytes(file_id).await? {
            Some(b) => b,
            None => return Ok(None),
        };

        let value = Self::deser_readableargs(file_id, &bytes, read_args)?;

        Ok(Some(value))
    }

    /// Reads, decrypts, and deserializes a [`VfsDirectory`] of LDK
    /// [`MaybeReadable`]s from the DB, along with their [`VfsFileId`]s.
    /// [`None`] values are omitted from the result.
    async fn read_dir_maybereadable<T: MaybeReadable>(
        &self,
        dir: &VfsDirectory,
    ) -> anyhow::Result<Vec<(VfsFileId, T)>> {
        let ids_and_bytes = self.read_dir_bytes(dir).await?;
        let mut ids_and_values = Vec::with_capacity(ids_and_bytes.len());
        for (file_id, bytes) in ids_and_bytes {
            let mut reader = Cursor::new(&bytes);
            let maybe_value = T::read(&mut reader)
                .map_err(|e| anyhow!("{e:?}"))
                .with_context(|| format!("{file_id}"))
                .context("LDK MaybeReadable deserialization failed (in dir)")?;
            if let Some(event) = maybe_value {
                ids_and_values.push((file_id, event));
            }
        }
        Ok(ids_and_values)
    }

    /// Reads and decrypts [`VfsFile`] bytes from the DB.
    async fn read_bytes(
        &self,
        file_id: &VfsFileId,
    ) -> anyhow::Result<Option<Vec<u8>>> {
        match self.read_file(file_id).await? {
            Some(file) => {
                let data = self.decrypt_file(file_id, file)?;
                Ok(Some(data))
            }
            None => Ok(None),
        }
    }

    /// Reads and decrypts all files in the given [`VfsDirectory`] from the DB,
    /// returning the [`VfsFileId`] and plaintext bytes for each file.
    async fn read_dir_bytes(
        &self,
        dir: &VfsDirectory,
    ) -> anyhow::Result<Vec<(VfsFileId, Vec<u8>)>> {
        let files = self.read_dir_files(dir).await?;
        let file_ids_and_bytes = files
            .into_iter()
            .map(|file| {
                // Get the expected dirname from params but filename from DB
                let expected_file_id = VfsFileId::new(
                    dir.dirname.clone(),
                    file.id.filename.clone(),
                );
                let bytes = self.decrypt_file(&expected_file_id, file)?;
                Ok((expected_file_id, bytes))
            })
            .collect::<anyhow::Result<Vec<(VfsFileId, Vec<u8>)>>>()?;
        Ok(file_ids_and_bytes)
    }

    /// Wraps [`Vfs::get_file`] to add logging and error context.
    async fn read_file(
        &self,
        file_id: &VfsFileId,
    ) -> anyhow::Result<Option<VfsFile>> {
        debug!("Reading file {file_id}");
        let result = self
            .get_file(file_id)
            .await
            .with_context(|| format!("Couldn't fetch file from DB: {file_id}"));

        if result.is_ok() {
            debug!("Done: Read {file_id}");
        } else {
            warn!("Error: Failed to read {file_id}");
        }
        result
    }

    /// Wraps [`Vfs::get_directory`] to add logging and error context.
    async fn read_dir_files(
        &self,
        dir: &VfsDirectory,
    ) -> anyhow::Result<Vec<VfsFile>> {
        debug!("Reading directory {dir}");
        let result = self
            .get_directory(dir)
            .await
            .with_context(|| format!("Couldn't fetch VFS dir from DB: {dir}"));

        if result.is_ok() {
            debug!("Done: Read directory {dir}");
        } else {
            warn!("Error: Failed to read directory {dir}");
        }
        result
    }

    /// Deserializes a LDK [`ReadableArgs`] of type `T` from bytes.
    fn deser_readableargs<T, A>(
        file_id: &VfsFileId,
        bytes: &[u8],
        read_args: A,
    ) -> anyhow::Result<T>
    where
        T: ReadableArgs<A>,
        A: Send,
    {
        let mut reader = Cursor::new(bytes);
        let value = T::read(&mut reader, read_args)
            .map_err(|e| anyhow!("{e:?}"))
            .with_context(|| format!("{file_id}"))
            .context("LDK ReadableArgs deserialization failed")?;
        Ok(value)
    }

    /// Serializes, encrypts, then persists a LDK [`Writeable`] to the DB.
    async fn persist_ldk_writeable<W: Writeable + Send + Sync>(
        &self,
        file_id: VfsFileId,
        writeable: &W,
        retries: usize,
    ) -> anyhow::Result<()> {
        let file = self.encrypt_ldk_writeable(file_id, writeable);
        self.persist_file(&file, retries).await
    }

    /// JSON-serializes, encrypts, then persists a type `T` to the DB.
    async fn persist_json<T: Serialize + Send + Sync>(
        &self,
        file_id: VfsFileId,
        value: &T,
        retries: usize,
    ) -> anyhow::Result<()> {
        let file = self.encrypt_json::<T>(file_id, value);
        self.persist_file(&file, retries).await
    }

    /// Wraps [`Vfs::upsert_file`] to add logging and error context.
    async fn persist_file(
        &self,
        file: &VfsFile,
        retries: usize,
    ) -> anyhow::Result<()> {
        let file_id = &file.id;
        let bytes = file.data.len();
        debug!("Persisting file {file_id} <{bytes} bytes>");

        let result = self
            .upsert_file(file, retries)
            .await
            .map(|_| ())
            .with_context(|| format!("Couldn't persist file to DB: {file_id}"));

        if result.is_ok() {
            debug!("Done: Persisted {file_id} <{bytes} bytes>");
        } else {
            warn!("Error: Failed to persist {file_id}  <{bytes} bytes>");
        }
        result
    }

    /// Wraps [`Vfs::delete_file`] to add logging and error context.
    async fn remove_file(&self, file_id: &VfsFileId) -> anyhow::Result<()> {
        debug!("Deleting file {file_id}");
        let result = self
            .delete_file(file_id)
            .await
            .map(|_| ())
            .with_context(|| format!("{file_id}"))
            .context("Couldn't delete file from DB");

        if result.is_ok() {
            debug!("Done: Deleted {file_id}");
        } else {
            warn!("Error: Failed to delete {file_id}");
        }
        result
    }
}

// --- Types --- //

/// Uniquely identifies a directory in the virtual file system.
///
/// This struct exists mainly so that `serde_qs` can use it as a query parameter
/// struct to fetch files by directory.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct VfsDirectory {
    pub dirname: Cow<'static, str>,
}

/// Uniquely identifies a file in the virtual file system.
///
/// This struct exists mainly so that `serde_qs` can use it as a query parameter
/// struct to fetch files by id.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct VfsFileId {
    // Flattened because serde_qs requires non-nested structs
    #[serde(flatten)]
    pub dir: VfsDirectory,
    pub filename: Cow<'static, str>,
}

/// Represents a file in the virtual file system. The `data` field is almost
/// always encrypted.
#[derive(Clone, Debug, Eq, PartialEq)]
#[derive(Serialize, Deserialize)]
pub struct VfsFile {
    #[serde(flatten)]
    pub id: VfsFileId,
    #[serde(with = "hexstr_or_bytes")]
    pub data: Vec<u8>,
}

/// An upgradeable version of [`Option<VfsFile>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaybeVfsFile {
    pub maybe_file: Option<VfsFile>,
}

/// An upgradeable version of [`Vec<VfsFile>`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecVfsFile {
    pub files: Vec<VfsFile>,
}

/// A list of all filenames within a [`VfsDirectory`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VfsDirectoryList {
    pub dirname: Cow<'static, str>,
    pub filenames: Vec<String>,
}

/// An upgradeable version of [`Vec<VfsFileId>`].
// TODO(max): Use basically VfsDirectory but with a Vec of filenames
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VecVfsFileId {
    pub file_ids: Vec<VfsFileId>,
}

impl VfsDirectory {
    pub fn new(dirname: impl Into<Cow<'static, str>>) -> Self {
        Self {
            dirname: dirname.into(),
        }
    }

    pub const fn new_const(dirname: &'static str) -> Self {
        Self {
            dirname: Cow::Borrowed(dirname),
        }
    }
}

impl VfsFileId {
    pub fn new(
        dirname: impl Into<Cow<'static, str>>,
        filename: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            dir: VfsDirectory {
                dirname: dirname.into(),
            },
            filename: filename.into(),
        }
    }

    pub const fn new_const(
        dirname: &'static str,
        filename: &'static str,
    ) -> Self {
        Self {
            dir: VfsDirectory {
                dirname: Cow::Borrowed(dirname),
            },
            filename: Cow::Borrowed(filename),
        }
    }
}

impl VfsFile {
    pub fn new(
        dirname: impl Into<Cow<'static, str>>,
        filename: impl Into<Cow<'static, str>>,
        data: Vec<u8>,
    ) -> Self {
        Self {
            id: VfsFileId {
                dir: VfsDirectory {
                    dirname: dirname.into(),
                },
                filename: filename.into(),
            },
            data,
        }
    }

    /// Prefer to use this constructor because `Into<Vec<u8>>` may have useful
    /// optimizations. For example, [`bytes::Bytes`] avoids a copy if the
    /// refcount is 1, but AIs like to use `bytes.to_vec()` which always copies.
    pub fn from_parts(id: VfsFileId, data: impl Into<Vec<u8>>) -> Self {
        Self {
            id,
            data: data.into(),
        }
    }
}

impl Display for VfsDirectory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{dirname}", dirname = self.dirname)
    }
}

impl Display for VfsFileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dirname = &self.dir.dirname;
        let filename = &self.filename;
        write!(f, "{dirname}/{filename}")
    }
}

// --- impl Arbitrary --- //

#[cfg(any(test, feature = "test-utils"))]
mod prop {
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for VfsDirectory {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            arbitrary::any_string().prop_map(VfsDirectory::new).boxed()
        }
    }

    impl Arbitrary for VfsFileId {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<VfsDirectory>(), arbitrary::any_string())
                .prop_map(|(dir, filename)| VfsFileId {
                    dir,
                    filename: filename.into(),
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn vfs_directory_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<VfsDirectory>();
    }

    #[test]
    fn vfs_file_id_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<VfsFileId>();
    }
}
