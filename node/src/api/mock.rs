#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use common::api::auth::{
    UserAuthRequest, UserAuthResponse, UserAuthToken, UserSignupRequest,
};
use common::api::def::{
    NodeBackendApi, NodeLspApi, NodeRunnerApi, UserBackendApi,
};
use common::api::error::{BackendApiError, LspApiError, RunnerApiError};
use common::api::ports::UserPorts;
use common::api::provision::{SealedSeed, SealedSeedId};
use common::api::vfs::{NodeDirectory, NodeFile, NodeFileId};
use common::api::{NodePk, Scid, User, UserPk};
use common::byte_str::ByteStr;
use common::constants::SINGLETON_DIRECTORY;
use common::ed25519;
use common::enclave::{self, Measurement};
use common::rng::SysRng;
use common::root_seed::RootSeed;
use once_cell::sync::Lazy;
use tokio::sync::mpsc;

use crate::api::BackendApiClient;
use crate::persister;

type FileName = String;
type Data = Vec<u8>;

// --- test fixtures --- //

fn make_user_pk(root_seed: &RootSeed) -> UserPk {
    root_seed.derive_user_pk()
}
fn make_node_pk(root_seed: &RootSeed) -> NodePk {
    root_seed.derive_node_pk(&mut SysRng::new())
}
fn make_sealed_seed(root_seed: &RootSeed) -> SealedSeed {
    SealedSeed::seal_from_root_seed(
        &mut SysRng::new(),
        root_seed,
        enclave::measurement(),
        enclave::machine_id(),
        enclave::MIN_SGX_CPUSVN,
    )
    .expect("Failed to seal test root seed")
}

static SEED1: Lazy<RootSeed> = Lazy::new(|| RootSeed::from_u64(1));
static SEED2: Lazy<RootSeed> = Lazy::new(|| RootSeed::from_u64(2));

pub static USER_PK1: Lazy<UserPk> = Lazy::new(|| make_user_pk(&SEED1));
pub static USER_PK2: Lazy<UserPk> = Lazy::new(|| make_user_pk(&SEED2));

static NODE_PK1: Lazy<NodePk> = Lazy::new(|| make_node_pk(&SEED1));
static NODE_PK2: Lazy<NodePk> = Lazy::new(|| make_node_pk(&SEED2));

static SEALED_SEED1: Lazy<SealedSeed> = Lazy::new(|| make_sealed_seed(&SEED1));
static SEALED_SEED2: Lazy<SealedSeed> = Lazy::new(|| make_sealed_seed(&SEED2));

const DUMMY_SCID: Scid = Scid(0);

pub fn sealed_seed(user_pk: &UserPk) -> SealedSeed {
    if user_pk == &*USER_PK1 {
        SEALED_SEED1.clone()
    } else if user_pk == &*USER_PK2 {
        SEALED_SEED2.clone()
    } else {
        todo!("TODO(max): Programmatically generate for new users")
    }
}

fn node_pk(user_pk: UserPk) -> NodePk {
    if user_pk == *USER_PK1 {
        *NODE_PK1
    } else if user_pk == *USER_PK2 {
        *NODE_PK2
    } else {
        todo!("TODO(max): Programmatically generate for new users")
    }
}

fn measurement(_user_pk: UserPk) -> Measurement {
    // It's the same for now but we may want to use different ones later
    enclave::measurement()
}

// --- The mock clients --- //

pub(crate) struct MockRunnerClient {
    notifs_tx: mpsc::Sender<UserPorts>,
    notifs_rx: Mutex<Option<mpsc::Receiver<UserPorts>>>,
}

impl MockRunnerClient {
    pub(crate) fn new() -> Self {
        let (notifs_tx, notifs_rx) = mpsc::channel(8);
        let notifs_rx = Mutex::new(Some(notifs_rx));
        Self {
            notifs_tx,
            notifs_rx,
        }
    }

    pub(crate) fn notifs_rx(&self) -> mpsc::Receiver<UserPorts> {
        self.notifs_rx
            .lock()
            .unwrap()
            .take()
            .expect("Someone already subscribed")
    }
}

#[async_trait]
impl NodeRunnerApi for MockRunnerClient {
    async fn ready(
        &self,
        user_ports: UserPorts,
    ) -> Result<UserPorts, RunnerApiError> {
        let _ = self.notifs_tx.try_send(user_ports);
        Ok(user_ports)
    }
}

pub(super) struct MockLspClient;

#[async_trait]
impl NodeLspApi for MockLspClient {
    async fn get_new_scid(
        &self,
        _node_pk: NodePk,
    ) -> Result<Scid, LspApiError> {
        Ok(DUMMY_SCID)
    }
}

pub(crate) struct MockBackendClient(Mutex<VirtualFileSystem>);

impl MockBackendClient {
    pub(crate) fn new() -> Self {
        Self(Mutex::new(VirtualFileSystem::new()))
    }
}

#[async_trait]
impl BackendApiClient for MockBackendClient {
    async fn create_file_with_retries(
        &self,
        data: &NodeFile,
        auth: UserAuthToken,
        _retries: usize,
    ) -> Result<NodeFile, BackendApiError> {
        self.create_file(data, auth).await
    }

    async fn upsert_file_with_retries(
        &self,
        data: &NodeFile,
        auth: UserAuthToken,
        _retries: usize,
    ) -> Result<(), BackendApiError> {
        self.upsert_file(data, auth).await
    }
}

#[async_trait]
impl UserBackendApi for MockBackendClient {
    async fn signup(
        &self,
        _signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError> {
        Ok(())
    }

    async fn user_auth(
        &self,
        _signed_req: ed25519::Signed<UserAuthRequest>,
    ) -> Result<UserAuthResponse, BackendApiError> {
        // TODO(phlip9): return something we can verify
        Ok(UserAuthResponse {
            user_auth_token: UserAuthToken(ByteStr::new()),
        })
    }
}

#[async_trait]
impl NodeBackendApi for MockBackendClient {
    /// Always return the dummy version
    async fn get_user(
        &self,
        user_pk: UserPk,
    ) -> Result<Option<User>, BackendApiError> {
        Ok(Some(User {
            user_pk,
            node_pk: node_pk(user_pk),
        }))
    }

    /// Always return the dummy version
    async fn get_sealed_seed(
        &self,
        data: SealedSeedId,
    ) -> Result<Option<SealedSeed>, BackendApiError> {
        Ok(Some(sealed_seed(&data.user_pk)))
    }

    async fn create_sealed_seed(
        &self,
        _data: SealedSeed,
        _auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        Ok(())
    }

    async fn get_scid(
        &self,
        _node_pk: NodePk,
    ) -> Result<Option<Scid>, BackendApiError> {
        Ok(Some(DUMMY_SCID))
    }

    async fn get_file(
        &self,
        file_id: &NodeFileId,
        _auth: UserAuthToken,
    ) -> Result<Option<NodeFile>, BackendApiError> {
        let file_opt = self.0.lock().unwrap().get(file_id.clone());
        Ok(file_opt)
    }

    async fn create_file(
        &self,
        file: &NodeFile,
        _auth: UserAuthToken,
    ) -> Result<NodeFile, BackendApiError> {
        let file_opt = self.0.lock().unwrap().insert(file.clone());
        assert!(file_opt.is_none());
        Ok(file.clone())
    }

    async fn upsert_file(
        &self,
        file: &NodeFile,
        _auth: UserAuthToken,
    ) -> Result<(), BackendApiError> {
        self.0.lock().unwrap().insert(file.clone());
        Ok(())
    }

    /// Returns "OK" if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &NodeFileId,
        _auth: UserAuthToken,
    ) -> Result<String, BackendApiError> {
        let file_opt = self.0.lock().unwrap().remove(file_id.clone());
        assert!(file_opt.is_none());
        Ok(String::from("OK"))
    }

    async fn get_directory(
        &self,
        dir: &NodeDirectory,
        _auth: UserAuthToken,
    ) -> Result<Vec<NodeFile>, BackendApiError> {
        let files_vec = self.0.lock().unwrap().get_dir(dir.clone());
        Ok(files_vec)
    }
}

struct VirtualFileSystem {
    inner: HashMap<NodeDirectory, HashMap<FileName, Data>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        let mut inner = HashMap::new();

        // For each user, insert all directories used by the persister
        for user_pk in [*USER_PK1, *USER_PK2] {
            let singleton_dir = NodeDirectory {
                user_pk,
                dirname: SINGLETON_DIRECTORY.into(),
            };
            let channel_monitors_dir = NodeDirectory {
                user_pk,
                dirname: persister::CHANNEL_MONITORS_DIRECTORY.into(),
            };
            inner.insert(singleton_dir, HashMap::new());
            inner.insert(channel_monitors_dir, HashMap::new());
        }

        Self { inner }
    }

    fn get(&self, file_id: NodeFileId) -> Option<NodeFile> {
        let dir = NodeDirectory {
            user_pk: file_id.dir.user_pk,
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .get(&file_id.filename)
            .map(|data| {
                NodeFile::new(
                    dir.user_pk,
                    dir.dirname,
                    file_id.filename,
                    data.clone(),
                )
            })
    }

    fn insert(&mut self, file: NodeFile) -> Option<NodeFile> {
        let dir = NodeDirectory {
            user_pk: file.id.dir.user_pk,
            dirname: file.id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .insert(file.id.filename.clone(), file.data)
            .map(|data| {
                NodeFile::new(dir.user_pk, dir.dirname, file.id.filename, data)
            })
    }

    fn remove(&mut self, file_id: NodeFileId) -> Option<NodeFile> {
        let dir = NodeDirectory {
            user_pk: file_id.dir.user_pk,
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .remove(&file_id.filename)
            .map(|data| {
                NodeFile::new(dir.user_pk, dir.dirname, file_id.filename, data)
            })
    }

    fn get_dir(&self, dir: NodeDirectory) -> Vec<NodeFile> {
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .iter()
            .map(|(name, data)| {
                NodeFile::new(
                    dir.user_pk,
                    dir.dirname.clone(),
                    name.clone(),
                    data.clone(),
                )
            })
            .collect()
    }
}
