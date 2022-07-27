#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use bitcoin::secp256k1::PublicKey;
use common::enclave::{self, Measurement};
use common::hex;
use tokio::sync::mpsc;

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

pub const PUBKEY1: [u8; 33] = hex::decode_const(
    b"02692f6894d5cb51bb785cc3c54f457889faf674fedea54a906f7ec99e88832d18",
);
pub const PUBKEY2: [u8; 33] = hex::decode_const(
    b"025336702e1317fcb55cdce19b26bd154b5d5612b87d04ff41f807372513f02b6a",
);
pub const HEX_SEED1: [u8; 32] = hex::decode_const(
    b"39ee00e3e23a9cd7e6509f56ff66daaf021cb5502e4ab3c6c393b522a6782d03",
);
pub const HEX_SEED2: [u8; 32] = hex::decode_const(
    b"2a784ea82ef7002ec929b435e1af283a1998878575e8ccbad73e5d0cb3a95f59",
);

fn pubkey(user_id: UserId) -> PublicKey {
    match user_id {
        1 => PublicKey::from_slice(&PUBKEY1).unwrap(),
        2 => PublicKey::from_slice(&PUBKEY2).unwrap(),
        _ => todo!("TODO(max): Programmatically generate for new users"),
    }
}

pub fn seed(user_id: UserId) -> Vec<u8> {
    match user_id {
        1 => HEX_SEED1.to_vec(),
        2 => HEX_SEED2.to_vec(),
        _ => todo!("TODO(max): Programmatically generate for new users"),
    }
}

fn instance_id(user_id: UserId) -> InstanceId {
    let measurement = enclave::measurement();
    let node_public_key = pubkey(user_id);
    convert::get_instance_id(&node_public_key, &measurement)
}

fn enclave_id(user_id: UserId) -> EnclaveId {
    let instance_id = instance_id(user_id);
    let machine_id = enclave::machine_id();
    convert::get_enclave_id(instance_id.as_str(), machine_id)
}

// --- The MockApiClient ---

pub struct MockApiClient {
    vfs: Mutex<VirtualFileSystem>,
    notifs_tx: mpsc::Sender<UserPort>,
    notifs_rx: Mutex<Option<mpsc::Receiver<UserPort>>>,
}

impl MockApiClient {
    pub fn new() -> Self {
        let vfs = Mutex::new(VirtualFileSystem::new());
        let (notifs_tx, notifs_rx) = mpsc::channel(8);
        let notifs_rx = Mutex::new(Some(notifs_rx));
        Self {
            vfs,
            notifs_tx,
            notifs_rx,
        }
    }

    pub fn notifs_rx(&self) -> mpsc::Receiver<UserPort> {
        self.notifs_rx
            .lock()
            .unwrap()
            .take()
            .expect("Someone already subscribed")
    }
}

#[async_trait]
impl ApiClient for MockApiClient {
    /// Always return the dummy version
    async fn get_node(
        &self,
        user_id: UserId,
    ) -> Result<Option<Node>, ApiError> {
        let node = Node {
            public_key: pubkey(user_id),
            user_id,
        };
        Ok(Some(node))
    }

    /// Always return the dummy version
    async fn get_instance(
        &self,
        user_id: UserId,
        _measurement: Measurement,
    ) -> Result<Option<Instance>, ApiError> {
        let instance = Instance {
            id: instance_id(user_id),
            measurement: enclave::measurement(),
            node_public_key: pubkey(user_id),
        };

        Ok(Some(instance))
    }

    /// Always return the dummy version
    async fn get_enclave(
        &self,
        user_id: UserId,
        _measurement: Measurement,
    ) -> Result<Option<Enclave>, ApiError> {
        let enclave = Enclave {
            id: enclave_id(user_id),
            seed: seed(user_id),
            instance_id: instance_id(user_id),
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
        let file_opt = self.vfs.lock().unwrap().get(file_id);
        Ok(file_opt)
    }

    async fn create_file(&self, file: File) -> Result<File, ApiError> {
        let file_opt = self.vfs.lock().unwrap().insert(file.clone());
        assert!(file_opt.is_none());
        Ok(file)
    }

    async fn upsert_file(&self, file: File) -> Result<File, ApiError> {
        self.vfs.lock().unwrap().insert(file.clone());
        Ok(file)
    }

    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: FileId) -> Result<String, ApiError> {
        let file_opt = self.vfs.lock().unwrap().remove(file_id);
        assert!(file_opt.is_none());
        Ok(String::from("OK"))
    }

    async fn get_directory(
        &self,
        dir_id: DirectoryId,
    ) -> Result<Vec<File>, ApiError> {
        let files_vec = self.vfs.lock().unwrap().get_dir(dir_id);
        Ok(files_vec)
    }

    async fn notify_runner(
        &self,
        user_port: UserPort,
    ) -> Result<UserPort, ApiError> {
        let _ = self.notifs_tx.try_send(user_port.clone());
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
            instance_id: instance_id(1),
            directory: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = DirectoryId {
            instance_id: instance_id(1),
            directory: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = DirectoryId {
            instance_id: instance_id(1),
            directory: persister::CHANNEL_MONITORS_DIRECTORY.into(),
        };
        inner.insert(singleton_dir, HashMap::new());
        inner.insert(channel_peers_dir, HashMap::new());
        inner.insert(channel_monitors_dir, HashMap::new());

        // Insert all directories used by the persister
        let singleton_dir = DirectoryId {
            instance_id: instance_id(2),
            directory: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = DirectoryId {
            instance_id: instance_id(2),
            directory: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = DirectoryId {
            instance_id: instance_id(2),
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
