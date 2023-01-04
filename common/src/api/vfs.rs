use serde::{Deserialize, Serialize};

use crate::api::UserPk;

/// Contains the basic components common to [`NodeFile`] and `LspFile`,
/// useful whenever it is necessary to abstract over both types.
pub struct BasicFile {
    pub dirname: String,
    pub filename: String,
    pub data: Vec<u8>,
}

/// Uniquely identifies a directory in the node's virtual file system.
#[derive(Clone, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct NodeDirectory {
    pub user_pk: UserPk,
    pub dirname: String,
}

/// Uniquely identifies a file in the node's virtual file system.
#[derive(Clone, Deserialize, Serialize)]
pub struct NodeFileId {
    // Flattened because serde_qs doesn't play well with nested structs
    #[serde(flatten)]
    pub dir: NodeDirectory,
    pub filename: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NodeFile {
    // Flattened because serde_qs doesn't play well with nested structs
    #[serde(flatten)]
    pub id: NodeFileId,
    pub data: Vec<u8>,
}

impl NodeFileId {
    pub fn new(user_pk: UserPk, dirname: String, filename: String) -> Self {
        Self {
            dir: NodeDirectory { user_pk, dirname },
            filename,
        }
    }
}

impl NodeFile {
    pub fn new(
        user_pk: UserPk,
        dirname: String,
        filename: String,
        data: Vec<u8>,
    ) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory { user_pk, dirname },
                filename,
            },
            data,
        }
    }

    pub fn from_basic(
        BasicFile {
            dirname,
            filename,
            data,
        }: BasicFile,
        user_pk: UserPk,
    ) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory { user_pk, dirname },
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
