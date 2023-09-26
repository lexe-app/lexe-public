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

use serde::{Deserialize, Serialize};

use crate::hexstr_or_bytes;

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

impl VfsFileId {
    pub fn new(dirname: String, filename: String) -> Self {
        Self {
            dir: VfsDirectory { dirname },
            filename,
        }
    }
}

impl VfsFile {
    pub fn new(dirname: String, filename: String, data: Vec<u8>) -> Self {
        Self {
            id: VfsFileId {
                dir: VfsDirectory { dirname },
                filename,
            },
            data,
        }
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
