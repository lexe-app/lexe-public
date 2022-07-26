use std::collections::HashMap;

use async_trait::async_trait;
use bitcoin::secp256k1::PublicKey;
use common::enclave::{self, Measurement};
use common::hex;
use tokio::sync::Mutex;

use crate::api::{
    ApiClient, ApiError, DirectoryId, Enclave, File, FileId, Instance, Node,
    NodeInstanceEnclave, UserPort,
};
use crate::convert;
use crate::lexe::persister;
use crate::types::{EnclaveId, InstanceId, UserId};

type FileName = String;
type Data = Vec<u8>;

// --- Consts used in the MockApiClient ---

pub const USER_ID: i64 = 1;
pub const PUBKEY: [u8; 33] = hex::decode_const(
    b"02692f6894d5cb51bb785cc3c54f457889faf674fedea54a906f7ec99e88832d18",
);
pub const HEX_SEED: [u8; 32] = hex::decode_const(
    b"39ee00e3e23a9cd7e6509f56ff66daaf021cb5502e4ab3c6c393b522a6782d03",
);
pub const CPU_ID: &str = "my_cpu_id";

fn pubkey() -> PublicKey {
    PublicKey::from_slice(&PUBKEY).unwrap()
}

fn instance() -> Instance {
    let measurement = enclave::measurement();
    let node_public_key = pubkey();
    Instance {
        id: convert::get_instance_id(&node_public_key, &measurement),
        measurement,
        node_public_key,
    }
}

fn instance_id() -> InstanceId {
    let measurement = enclave::measurement();
    let node_public_key = pubkey();
    convert::get_instance_id(&node_public_key, &measurement)
}

fn enclave_id() -> EnclaveId {
    convert::get_enclave_id(&instance_id(), CPU_ID)
}

// --- The MockApiClient ---

pub struct MockApiClient {
    vfs: Mutex<VirtualFileSystem>,
}

impl MockApiClient {
    pub fn new() -> Self {
        let vfs = Mutex::new(VirtualFileSystem::new());
        Self { vfs }
    }
}

#[async_trait]
impl ApiClient for MockApiClient {
    /// Always return the dummy version
    async fn get_node(
        &self,
        _user_id: UserId,
    ) -> Result<Option<Node>, ApiError> {
        let node = Node {
            public_key: pubkey(),
            user_id: USER_ID,
        };
        Ok(Some(node))
    }

    /// Always return the dummy version
    async fn get_instance(
        &self,
        _user_id: UserId,
        _measurement: Measurement,
    ) -> Result<Option<Instance>, ApiError> {
        Ok(Some(instance()))
    }

    /// Always return the dummy version
    async fn get_enclave(
        &self,
        _user_id: UserId,
        _measurement: Measurement,
    ) -> Result<Option<Enclave>, ApiError> {
        let enclave = Enclave {
            id: enclave_id(),
            seed: HEX_SEED.to_vec(),
            instance_id: instance_id(),
        };
        Ok(Some(enclave))
    }

    async fn create_node_instance_enclave(
        &self,
        req: NodeInstanceEnclave,
    ) -> Result<NodeInstanceEnclave, ApiError> {
        Ok(req)
    }

    async fn get_file(
        &self,
        file_id: FileId,
    ) -> Result<Option<File>, ApiError> {
        let file_opt = self.vfs.lock().await.get(file_id.clone());
        Ok(file_opt)
    }

    async fn create_file(&self, file: File) -> Result<File, ApiError> {
        let file_opt = self.vfs.lock().await.insert(file.clone());
        assert!(file_opt.is_none());
        Ok(file)
    }

    async fn upsert_file(&self, file: File) -> Result<File, ApiError> {
        self.vfs.lock().await.insert(file.clone());
        Ok(file)
    }

    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: FileId) -> Result<String, ApiError> {
        let file_opt = self.vfs.lock().await.remove(file_id);
        assert!(file_opt.is_none());
        Ok(String::from("OK"))
    }

    async fn get_directory(
        &self,
        dir_id: DirectoryId,
    ) -> Result<Vec<File>, ApiError> {
        let files_vec = self.vfs.lock().await.get_dir(dir_id);
        Ok(files_vec)
    }

    async fn notify_runner(
        &self,
        user_port: UserPort,
    ) -> Result<UserPort, ApiError> {
        Ok(user_port)
    }
}

struct VirtualFileSystem {
    inner: HashMap<DirectoryId, HashMap<FileName, Data>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        let mut inner = HashMap::new();

        // Insert all directories used by the persister
        let singleton_dir = DirectoryId {
            instance_id: instance_id(),
            directory: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = DirectoryId {
            instance_id: instance_id(),
            directory: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = DirectoryId {
            instance_id: instance_id(),
            directory: persister::CHANNEL_MONITORS_DIRECTORY.into(),
        };
        inner.insert(singleton_dir, HashMap::new());
        inner.insert(channel_peers_dir, HashMap::new());
        inner.insert(channel_monitors_dir, HashMap::new());

        Self { inner }
    }

    fn get(&self, file_id: FileId) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file_id.instance_id,
            directory: file_id.directory,
        };
        self.inner
            .get(&dir_id)
            .expect("Missing directory")
            .get(&file_id.name)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file_id.name,
                data: data.clone(),
            })
    }

    fn insert(&mut self, file: File) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file.instance_id,
            directory: file.directory,
        };
        self.inner
            .get_mut(&dir_id)
            .expect("Missing directory")
            .insert(file.name.clone(), file.data)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file.name,
                data,
            })
    }

    fn remove(&mut self, file_id: FileId) -> Option<File> {
        let dir_id = DirectoryId {
            instance_id: file_id.instance_id,
            directory: file_id.directory,
        };
        self.inner
            .get_mut(&dir_id)
            .expect("Missing directory")
            .remove(&file_id.name)
            .map(|data| File {
                instance_id: dir_id.instance_id,
                directory: dir_id.directory,
                name: file_id.name,
                data,
            })
    }

    fn get_dir(&self, dir_id: DirectoryId) -> Vec<File> {
        self.inner
            .get(&dir_id)
            .expect("Missing directory")
            .iter()
            .map(|(name, data)| File {
                instance_id: dir_id.instance_id.clone(),
                directory: dir_id.directory.clone(),
                name: name.clone(),
                data: data.clone(),
            })
            .collect()
    }
}
