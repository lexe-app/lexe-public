use serde::{Deserialize, Serialize};

use crate::types::{Port, UserId};

/// Query parameter struct for fetching with no data attached
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Query parameter struct for fetching by user id
#[derive(Serialize)]
pub struct GetByUserId {
    pub user_id: UserId,
}

/// Query parameter struct for fetching by user id and measurement
#[derive(Serialize)]
pub struct GetByUserIdAndMeasurement {
    pub user_id: UserId,
    pub measurement: String,
}

/// Query parameter struct for fetching by instance id
#[derive(Serialize)]
pub struct GetByInstanceId {
    pub instance_id: String,
}

/// Uniquely identifies a file in the node's virtual file system.
#[derive(Serialize)]
pub struct FileId {
    pub instance_id: String,
    pub directory: String,
    pub name: String,
}

/// Uniquely identifies a directory in the node's virtual file system.
#[derive(Serialize)]
pub struct DirectoryId {
    pub instance_id: String,
    pub directory: String,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub public_key: String,
    pub user_id: UserId,
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub measurement: String,
    pub node_public_key: String,
}

#[derive(Serialize, Deserialize)]
pub struct Enclave {
    pub id: String,
    pub seed: Vec<u8>,
    pub instance_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct File {
    pub instance_id: String,
    pub directory: String,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceEnclave {
    pub node: Node,
    pub instance: Instance,
    pub enclave: Enclave,
}

#[derive(Serialize, Deserialize)]
pub struct ChannelMonitor {
    pub instance_id: String,
    pub tx_id: String,
    pub tx_index: i16,
    pub state: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct ChannelManager {
    pub instance_id: String,
    pub state: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct ProbabilisticScorer {
    pub instance_id: String,
    pub state: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct NetworkGraph {
    pub instance_id: String,
    pub state: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct ChannelPeer {
    pub instance_id: String,
    pub peer_public_key: String,
    pub peer_address: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserPort {
    pub user_id: UserId,
    pub port: Port,
}
