use bitcoin::secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::enclave::Measurement;

/// Uniquely identifies a directory in the node's virtual file system.
#[derive(Clone, Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct NodeDirectory {
    pub node_pk: PublicKey,
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
        node_pk: PublicKey,
        measurement: Measurement,
        dirname: String,
        filename: String,
    ) -> Self {
        Self {
            dir: NodeDirectory {
                node_pk,
                measurement,
                dirname,
            },
            filename,
        }
    }
}

impl NodeFile {
    pub fn new(
        node_pk: PublicKey,
        measurement: Measurement,
        dirname: String,
        filename: String,
        data: Vec<u8>,
    ) -> Self {
        Self {
            id: NodeFileId {
                dir: NodeDirectory {
                    node_pk,
                    measurement,
                    dirname,
                },
                filename,
            },
            data,
        }
    }
}
