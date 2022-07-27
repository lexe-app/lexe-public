use bitcoin::secp256k1::PublicKey;
use common::api::UserPk;
use common::enclave::Measurement;
use serde::{Deserialize, Serialize};

use crate::types::{EnclaveId, InstanceId, Port};

/// Query parameter struct for fetching with no data attached
///
/// Is defined with {} otherwise serde_qs vomits
#[derive(Serialize)]
pub struct EmptyData {}

/// Query parameter struct for fetching by user pk
#[derive(Serialize)]
pub struct GetByUserPk {
    pub user_pk: UserPk,
}

/// Query parameter struct for fetching by user pk and measurement
#[derive(Serialize)]
pub struct GetByUserPkAndMeasurement {
    pub user_pk: UserPk,
    pub measurement: Measurement,
}

/// Query parameter struct for fetching by instance id
#[derive(Serialize)]
pub struct GetByInstanceId {
    pub instance_id: InstanceId,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub node_pk: PublicKey,
    pub user_pk: UserPk,
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub id: InstanceId,
    pub measurement: Measurement,
    pub node_pk: PublicKey,
}

#[derive(Serialize, Deserialize)]
pub struct Enclave {
    pub id: EnclaveId,
    pub seed: Vec<u8>,
    pub instance_id: InstanceId,
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceEnclave {
    pub node: Node,
    pub instance: Instance,
    pub enclave: Enclave,
}

/// Uniquely identifies a file in the node's virtual file system.
#[derive(Clone, Deserialize, Serialize)]
pub struct FileId {
    pub instance_id: InstanceId,
    pub directory: String,
    pub name: String,
}

/// Uniquely identifies a directory in the node's virtual file system.
#[derive(Hash, Eq, PartialEq, Deserialize, Serialize)]
pub struct DirectoryId {
    pub instance_id: InstanceId,
    pub directory: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct File {
    pub instance_id: InstanceId,
    pub directory: String,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserPort {
    pub user_pk: UserPk,
    pub port: Port,
}
