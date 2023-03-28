
use serde::{Deserialize, Serialize};

/// Contains the basic components common to [`NodeFile`] and `LspFile`,
/// useful whenever it is necessary to abstract over both types.
pub struct BasicFile {
    pub dirname: String,
    pub filename: String,
    pub data: Vec<u8>,
}

/// Uniquely identifies a directory in the virtual file system.
///
/// This struct exists mainly so that `serde_qs` can use it as a query parameter
/// struct to fetch files by directory.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Deserialize, Serialize)]
pub struct NodeDirectory {
    pub dirname: String,
}

/// Uniquely identifies a file in the virtual file system.
///
/// This struct exists mainly so that `serde_qs` can use it as a query parameter
/// struct to fetch files by id.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct NodeFileId {
    // Flattened because serde_qs requires non-nested structs
    #[serde(flatten)]
    pub dir: NodeDirectory,
    pub filename: String,
}

/// Represents a file in the virtual file system. The `data` field is almost
/// always encrypted.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeFile {
    // Flattened because serde_qs requires non-nested structs
    #[serde(flatten)]
    pub id: NodeFileId,
    pub data: Vec<u8>,
}

impl NodeFileId {
    pub fn new(dirname: String, filename: String) -> Self {
        Self {
            dir: NodeDirectory { dirname },
            filename,
        }
    }
}

impl NodeFile {
    pub fn new(dirname: String, filename: String, data: Vec<u8>) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory { dirname },
                filename,
            },
            data,
        }
    }
}

impl From<BasicFile> for NodeFile {
    fn from(
        BasicFile {
            dirname,
            filename,
            data,
        }: BasicFile,
    ) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory { dirname },
                filename,
            },
            data,
        }
    }
}

impl From<NodeFile> for BasicFile {
    fn from(node_file: NodeFile) -> Self {
        Self {
            dirname: node_file.id.dir.dirname,
            filename: node_file.id.filename,
            data: node_file.data,
        }
    }
}
