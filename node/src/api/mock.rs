#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use bitcoin::secp256k1::PublicKey;
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, SealedSeed, SealedSeedId,
};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::{self, Measurement};
use common::hex;
use tokio::sync::mpsc;

use crate::api::{ApiClient, ApiError};
use crate::lexe::persister;

type FileName = String;
type Data = Vec<u8>;

const HEX_SEED1: [u8; 32] = hex::decode_const(
    b"39ee00e3e23a9cd7e6509f56ff66daaf021cb5502e4ab3c6c393b522a6782d03",
);
const HEX_SEED2: [u8; 32] = hex::decode_const(
    b"2a784ea82ef7002ec929b435e1af283a1998878575e8ccbad73e5d0cb3a95f59",
);

pub fn seed(node_pk: PublicKey) -> Vec<u8> {
    let node_pk_bytes = node_pk.serialize();

    if node_pk_bytes == NODE_PK1 {
        HEX_SEED1.to_vec()
    } else if node_pk_bytes == NODE_PK2 {
        HEX_SEED2.to_vec()
    } else {
        todo!("TODO(max): Programmatically generate for new users")
    }
}

const NODE_PK1: [u8; 33] = hex::decode_const(
    b"02692f6894d5cb51bb785cc3c54f457889faf674fedea54a906f7ec99e88832d18",
);
const NODE_PK2: [u8; 33] = hex::decode_const(
    b"025336702e1317fcb55cdce19b26bd154b5d5612b87d04ff41f807372513f02b6a",
);

fn node_pk(user_pk: UserPk) -> PublicKey {
    match user_pk.to_i64() {
        1 => PublicKey::from_slice(&NODE_PK1).unwrap(),
        2 => PublicKey::from_slice(&NODE_PK2).unwrap(),
        _ => todo!("TODO(max): Programmatically generate for new users"),
    }
}

fn measurement(_user_pk: UserPk) -> Measurement {
    // It's the same for now but we may want to use different ones later
    enclave::measurement()
}

// --- The MockApiClient ---

pub struct MockApiClient {
    vfs: Mutex<VirtualFileSystem>,
    notifs_tx: mpsc::Sender<UserPorts>,
    notifs_rx: Mutex<Option<mpsc::Receiver<UserPorts>>>,
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

    pub fn notifs_rx(&self) -> mpsc::Receiver<UserPorts> {
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
        user_pk: UserPk,
    ) -> Result<Option<Node>, ApiError> {
        let node = Node {
            node_pk: node_pk(user_pk),
            user_pk,
        };
        Ok(Some(node))
    }

    /// Always return the dummy version
    async fn get_instance(
        &self,
        user_pk: UserPk,
        _measurement: Measurement,
    ) -> Result<Option<Instance>, ApiError> {
        let instance = Instance {
            node_pk: node_pk(user_pk),
            measurement: enclave::measurement(),
        };

        Ok(Some(instance))
    }

    /// Always return the dummy version
    async fn get_sealed_seed(
        &self,
        req: SealedSeedId,
    ) -> Result<Option<SealedSeed>, ApiError> {
        let sealed_seed = SealedSeed::new(
            req.node_pk,
            req.measurement,
            req.machine_id,
            req.min_cpusvn,
            seed(req.node_pk),
        );
        Ok(Some(sealed_seed))
    }

    async fn create_node_instance_seed(
        &self,
        req: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, ApiError> {
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
        dir: Directory,
    ) -> Result<Vec<File>, ApiError> {
        let files_vec = self.vfs.lock().unwrap().get_dir(dir);
        Ok(files_vec)
    }

    async fn notify_runner(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, ApiError> {
        let _ = self.notifs_tx.try_send(user_ports);
        Ok(user_ports)
    }
}

struct VirtualFileSystem {
    inner: HashMap<Directory, HashMap<FileName, Data>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        let mut inner = HashMap::new();

        // TODO(max): Generalize this

        // Insert all directories used by the persister
        let user_pk1 = UserPk::from_i64(1);
        let singleton_dir = Directory {
            node_pk: node_pk(user_pk1),
            measurement: measurement(user_pk1),
            dirname: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = Directory {
            node_pk: node_pk(user_pk1),
            measurement: measurement(user_pk1),
            dirname: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = Directory {
            node_pk: node_pk(user_pk1),
            measurement: measurement(user_pk1),
            dirname: persister::CHANNEL_MONITORS_DIRECTORY.into(),
        };
        inner.insert(singleton_dir, HashMap::new());
        inner.insert(channel_peers_dir, HashMap::new());
        inner.insert(channel_monitors_dir, HashMap::new());

        // Insert all directories used by the persister
        let user_pk2 = UserPk::from_i64(2);
        let singleton_dir = Directory {
            node_pk: node_pk(user_pk2),
            measurement: measurement(user_pk2),
            dirname: persister::SINGLETON_DIRECTORY.into(),
        };
        let channel_peers_dir = Directory {
            node_pk: node_pk(user_pk2),
            measurement: measurement(user_pk2),
            dirname: persister::CHANNEL_PEERS_DIRECTORY.into(),
        };
        let channel_monitors_dir = Directory {
            node_pk: node_pk(user_pk2),
            measurement: measurement(user_pk2),
            dirname: persister::CHANNEL_MONITORS_DIRECTORY.into(),
        };
        inner.insert(singleton_dir, HashMap::new());
        inner.insert(channel_peers_dir, HashMap::new());
        inner.insert(channel_monitors_dir, HashMap::new());

        Self { inner }
    }

    fn get(&self, file_id: FileId) -> Option<File> {
        let dir = Directory {
            node_pk: file_id.dir.node_pk,
            measurement: file_id.dir.measurement,
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .get(&file_id.filename)
            .map(|data| {
                File::new(
                    dir.node_pk,
                    dir.measurement,
                    dir.dirname,
                    file_id.filename,
                    data.clone(),
                )
            })
    }

    fn insert(&mut self, file: File) -> Option<File> {
        let dir = Directory {
            node_pk: file.id.dir.node_pk,
            measurement: file.id.dir.measurement,
            dirname: file.id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .insert(file.id.filename.clone(), file.data)
            .map(|data| {
                File::new(
                    dir.node_pk,
                    dir.measurement,
                    dir.dirname,
                    file.id.filename,
                    data,
                )
            })
    }

    fn remove(&mut self, file_id: FileId) -> Option<File> {
        let dir = Directory {
            node_pk: file_id.dir.node_pk,
            measurement: file_id.dir.measurement,
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .remove(&file_id.filename)
            .map(|data| {
                File::new(
                    dir.node_pk,
                    dir.measurement,
                    dir.dirname,
                    file_id.filename,
                    data,
                )
            })
    }

    fn get_dir(&self, dir: Directory) -> Vec<File> {
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .iter()
            .map(|(name, data)| {
                File::new(
                    dir.node_pk,
                    dir.measurement,
                    dir.dirname.clone(),
                    name.clone(),
                    data.clone(),
                )
            })
            .collect()
    }
}
