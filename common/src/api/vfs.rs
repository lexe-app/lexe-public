use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::Measurement;

/// Uniquely identifies a directory in the node's virtual file system.
#[derive(Clone, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct NodeDirectory {
    pub user_pk: UserPk,
    pub measurement: Measurement,
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
    pub fn new(
        user_pk: UserPk,
        measurement: Measurement,
        dirname: String,
        filename: String,
    ) -> Self {
        Self {
            dir: NodeDirectory {
                user_pk,
                measurement,
                dirname,
            },
            filename,
        }
    }
}

impl NodeFile {
    pub fn new(
        user_pk: UserPk,
        measurement: Measurement,
        dirname: String,
        filename: String,
        data: Vec<u8>,
    ) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory {
                    user_pk,
                    measurement,
                    dirname,
                },
                filename,
            },
            data,
        }
    }
}
