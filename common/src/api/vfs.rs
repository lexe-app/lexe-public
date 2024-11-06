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
    fmt::{self, Display},
    io::Cursor,
    time::Instant,
};

use anyhow::{anyhow, Context};
use async_trait::async_trait;
use lightning::util::ser::{ReadableArgs, Writeable};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::{debug, warn};

use super::{error::BackendApiError, Empty};
use crate::hexstr_or_bytes;

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

        let mut reader = Cursor::new(&bytes);
        let value = T::read(&mut reader, read_args)
            .map_err(|e| anyhow!("{e:?}"))
            .with_context(|| format!("{file_id}"))
            .context("LDK deserialization failed")?;

        Ok(Some(value))
    }

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

    /// Wraps [`Vfs::get_file`] to add logging and error context.
    async fn read_file(
        &self,
        file_id: &VfsFileId,
    ) -> anyhow::Result<Option<VfsFile>> {
        let start = Instant::now();

        debug!("Reading file {file_id}");
        let result = self
            .get_file(file_id)
            .await
            .with_context(|| format!("Couldn't fetch file from DB: {file_id}"));

        let elapsed = start.elapsed();
        if result.is_ok() {
            debug!("Done: Read {file_id} <{elapsed:?}>");
        } else {
            warn!("Error: Failed to read {file_id} <{elapsed:?}>");
        }
        result
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
        let start = Instant::now();

        let file_id = &file.id;
        let bytes = file.data.len();
        debug!("Persisting file {file_id} <{bytes} bytes>");

        let result = self
            .upsert_file(file, retries)
            .await
            .map(|_| ())
            .with_context(|| format!("Couldn't persist file to DB: {file_id}"));

        let elapsed = start.elapsed();
        if result.is_ok() {
            debug!("Done: Persisted {file_id} <{elapsed:?}> <{bytes} bytes>");
        } else {
            warn!(
                "Error: Failed to persist {file_id} \
                <{elapsed:?}> <{bytes} bytes>"
            );
        }
        result
    }
}

/// Uniquely identifies a directory in the virtual file system.
///
/// This struct exists mainly so that `serde_qs` can use it as a query parameter
/// struct to fetch files by directory.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct VfsDirectory {
    pub dirname: String,
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
    pub filename: String,
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

impl VfsDirectory {
    pub fn new(dirname: impl Into<String>) -> Self {
        Self {
            dirname: dirname.into(),
        }
    }
}

impl VfsFileId {
    pub fn new(
        dirname: impl Into<String>,
        filename: impl Into<String>,
    ) -> Self {
        Self {
            dir: VfsDirectory {
                dirname: dirname.into(),
            },
            filename: filename.into(),
        }
    }
}

impl VfsFile {
    pub fn new(
        dirname: impl Into<String>,
        filename: impl Into<String>,
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
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::test_utils::arbitrary;

    impl Arbitrary for VfsDirectory {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            arbitrary::any_string()
                .prop_map(|dirname| VfsDirectory { dirname })
                .boxed()
        }
    }

    impl Arbitrary for VfsFileId {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<VfsDirectory>(), arbitrary::any_string())
                .prop_map(|(dir, filename)| VfsFileId { dir, filename })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn vfs_directory_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<VfsDirectory>();
    }

    #[test]
    fn vfs_file_id_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<VfsFileId>();
    }
}
