#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use bitcoin::secp256k1::PublicKey;
use common::api::provision::{
    Instance, Node, NodeInstanceSeed, ProvisionedSecrets, SealedSeed,
    SealedSeedId,
};
use common::api::runner::UserPorts;
use common::api::vfs::{Directory, File, FileId};
use common::api::UserPk;
use common::enclave::{self, Measurement};
use common::rng::SysRng;
use common::root_seed::RootSeed;
use once_cell::sync::Lazy;
use secrecy::{ExposeSecret, Secret};
use tokio::sync::mpsc;

use crate::api::{ApiClient, ApiError};
use crate::lexe::persister;

type FileName = String;
type Data = Vec<u8>;

// --- test fixtures --- //

fn make_seed(bytes: [u8; 32]) -> RootSeed {
    RootSeed::new(Secret::new(bytes))
}
fn make_node_pk(seed: &RootSeed) -> PublicKey {
    PublicKey::from_keypair(&seed.derive_node_key_pair(&mut SysRng::new()))
}
fn make_sealed_seed(seed: &RootSeed) -> Vec<u8> {
    let seed = make_seed(*seed.expose_secret());
    let provisioned_secrets = ProvisionedSecrets { root_seed: seed };
    let sealed_secrets = provisioned_secrets.seal(&mut SysRng::new()).unwrap();
    sealed_secrets.serialize()
}

static SEED1: Lazy<RootSeed> = Lazy::new(|| make_seed([0x42; 32]));
static SEED2: Lazy<RootSeed> = Lazy::new(|| make_seed([0x69; 32]));

static NODE_PK1: Lazy<PublicKey> = Lazy::new(|| make_node_pk(&SEED1));
static NODE_PK2: Lazy<PublicKey> = Lazy::new(|| make_node_pk(&SEED2));

static SEALED_SEED1: Lazy<Vec<u8>> = Lazy::new(|| make_sealed_seed(&SEED1));
static SEALED_SEED2: Lazy<Vec<u8>> = Lazy::new(|| make_sealed_seed(&SEED2));

pub fn sealed_seed(node_pk: &PublicKey) -> Vec<u8> {
    if node_pk == &*NODE_PK1 {
        SEALED_SEED1.clone()
    } else if node_pk == &*NODE_PK2 {
        SEALED_SEED2.clone()
    } else {
        todo!("TODO(max): Programmatically generate for new users")
    }
}

fn node_pk(user_pk: UserPk) -> PublicKey {
    match user_pk.to_i64() {
        1 => *NODE_PK1,
        2 => *NODE_PK2,
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
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, ApiError> {
        let sealed_seed = SealedSeed::new(
            data.node_pk,
            data.measurement,
            data.machine_id,
            data.min_cpusvn,
            sealed_seed(&data.node_pk),
        );
        Ok(Some(sealed_seed))
    }

    async fn create_node_instance_seed(
        &self,
        data: NodeInstanceSeed,
    ) -> Result<NodeInstanceSeed, ApiError> {
        Ok(data)
    }

    async fn get_file(
        &self,
        file_id: &FileId,
    ) -> Result<Option<File>, ApiError> {
        let file_opt = self.vfs.lock().unwrap().get(file_id.clone());
        Ok(file_opt)
    }

    async fn create_file(&self, file: &File) -> Result<File, ApiError> {
        let file_opt = self.vfs.lock().unwrap().insert(file.clone());
        assert!(file_opt.is_none());
        Ok(file.clone())
    }

    async fn create_file_with_retries(
        &self,
        file: &File,
        _retries: usize,
    ) -> Result<File, ApiError> {
        self.create_file(file).await
    }

    async fn upsert_file(&self, file: &File) -> Result<File, ApiError> {
        self.vfs.lock().unwrap().insert(file.clone());
        Ok(file.clone())
    }

    async fn upsert_file_with_retries(
        &self,
        file: &File,
        _retries: usize,
    ) -> Result<File, ApiError> {
        self.upsert_file(file).await
    }

    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(&self, file_id: &FileId) -> Result<String, ApiError> {
        let file_opt = self.vfs.lock().unwrap().remove(file_id.clone());
        assert!(file_opt.is_none());
        Ok(String::from("OK"))
    }

    async fn get_directory(
        &self,
        dir: &Directory,
    ) -> Result<Vec<File>, ApiError> {
        let files_vec = self.vfs.lock().unwrap().get_dir(dir.clone());
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
