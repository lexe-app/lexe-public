// #![allow(dead_code)] // TODO(max): Remove and replace with SGX cfgs

use std::collections::{BTreeMap, HashMap, HashSet};
use std::str::FromStr;
use std::sync::Mutex;

use async_trait::async_trait;
use common::api::auth::{
    BearerAuthRequest, BearerAuthResponse, BearerAuthToken, UserSignupRequest,
};
use common::api::def::{
    AppBackendApi, BearerAuthBackendApi, NodeBackendApi, NodeLspApi,
    NodeRunnerApi,
};
use common::api::error::{
    BackendApiError, BackendErrorKind, LspApiError, RunnerApiError,
};
use common::api::ports::UserPorts;
use common::api::provision::{SealedSeed, SealedSeedId};
use common::api::qs::{GetNewPayments, GetPaymentByIndex, GetPaymentsByIds};
use common::api::vfs::{VfsDirectory, VfsFile, VfsFileId};
use common::api::{NodePk, Scid, User, UserPk};
use common::byte_str::ByteStr;
use common::constants::SINGLETON_DIRECTORY;
use common::ln::payments::{
    DbPayment, LxPaymentId, PaymentIndex, PaymentStatus,
};
use common::rng::SysRng;
use common::root_seed::RootSeed;
use common::time::TimestampMs;
use common::{ed25519, enclave};
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

// --- The mock clients --- //

pub(crate) struct MockRunnerClient {
    notifs_tx: mpsc::Sender<UserPorts>,
    #[allow(dead_code)] // Used in tests
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

    #[allow(dead_code)] // Used in tests
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

pub(crate) struct MockBackendClient {
    vfs: Mutex<VirtualFileSystem>,
    payments: Mutex<BTreeMap<PaymentIndex, DbPayment>>,
}

impl MockBackendClient {
    pub(crate) fn new() -> Self {
        let vfs = Mutex::new(VirtualFileSystem::new());
        let payments = Mutex::new(BTreeMap::new());
        Self { vfs, payments }
    }
}

#[async_trait]
impl BackendApiClient for MockBackendClient {
    async fn create_file_with_retries(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
        _retries: usize,
    ) -> Result<(), BackendApiError> {
        self.create_file(data, auth).await
    }

    async fn upsert_file_with_retries(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
        _retries: usize,
    ) -> Result<(), BackendApiError> {
        self.upsert_file(data, auth).await
    }
}

#[async_trait]
impl AppBackendApi for MockBackendClient {
    async fn signup(
        &self,
        _signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError> {
        Ok(())
    }
}

#[async_trait]
impl BearerAuthBackendApi for MockBackendClient {
    async fn bearer_auth(
        &self,
        _signed_req: ed25519::Signed<BearerAuthRequest>,
    ) -> Result<BearerAuthResponse, BackendApiError> {
        // TODO(phlip9): return something we can verify
        Ok(BearerAuthResponse {
            bearer_auth_token: BearerAuthToken(ByteStr::new()),
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
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        Ok(())
    }

    async fn get_scid(
        &self,
        _node_pk: NodePk,
        _auth: BearerAuthToken,
    ) -> Result<Option<Scid>, BackendApiError> {
        Ok(Some(DUMMY_SCID))
    }

    async fn get_file(
        &self,
        file_id: &VfsFileId,
        _auth: BearerAuthToken,
    ) -> Result<Option<VfsFile>, BackendApiError> {
        let file_opt = self.vfs.lock().unwrap().get(file_id.clone());
        Ok(file_opt)
    }

    async fn create_file(
        &self,
        file: &VfsFile,
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        let mut locked_vfs = self.vfs.lock().unwrap();
        if locked_vfs.get(file.id.clone()).is_some() {
            return Err(BackendApiError {
                kind: BackendErrorKind::Duplicate,
                msg: String::new(),
            });
        }

        let file_opt = locked_vfs.insert(file.clone());
        assert!(file_opt.is_none());
        Ok(())
    }

    async fn upsert_file(
        &self,
        file: &VfsFile,
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        self.vfs.lock().unwrap().insert(file.clone());
        Ok(())
    }

    /// Returns [`Ok`] if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        let file_opt = self.vfs.lock().unwrap().remove(file_id.clone());
        if file_opt.is_some() {
            Ok(())
        } else {
            Err(BackendApiError {
                kind: BackendErrorKind::NotFound,
                msg: String::new(),
            })
        }
    }

    async fn get_directory(
        &self,
        dir: &VfsDirectory,
        _auth: BearerAuthToken,
    ) -> Result<Vec<VfsFile>, BackendApiError> {
        let files_vec = self.vfs.lock().unwrap().get_dir(dir.clone());
        Ok(files_vec)
    }

    async fn get_payment(
        &self,
        req: GetPaymentByIndex,
        _auth: BearerAuthToken,
    ) -> Result<Option<DbPayment>, BackendApiError> {
        self.payments
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _v)| k.id == req.index.id)
            .map(|(_k, v)| v)
            .cloned()
            .map(Ok)
            .transpose()
    }

    async fn create_payment(
        &self,
        payment: DbPayment,
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        let mut locked_payments = self.payments.lock().unwrap();
        let created_at = TimestampMs::try_from(payment.created_at).unwrap();
        let id = LxPaymentId::from_str(&payment.id).unwrap();
        let key = PaymentIndex { created_at, id };

        if locked_payments.get(&key).is_some() {
            return Err(BackendApiError {
                kind: BackendErrorKind::Duplicate,
                msg: String::new(),
            });
        }
        let maybe_payment = locked_payments.insert(key, payment);
        assert!(maybe_payment.is_none());
        Ok(())
    }

    async fn upsert_payment(
        &self,
        payment: DbPayment,
        _auth: BearerAuthToken,
    ) -> Result<(), BackendApiError> {
        let created_at = TimestampMs::try_from(payment.created_at).unwrap();
        let id = LxPaymentId::from_str(&payment.id).unwrap();
        let key = PaymentIndex { created_at, id };
        self.payments.lock().unwrap().insert(key, payment);
        Ok(())
    }

    async fn get_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
        _auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
        let ids = req.ids.into_iter().collect::<HashSet<_>>();
        let payments = self
            .payments
            .lock()
            .unwrap()
            .values()
            .filter(|p| ids.contains(&p.id))
            .cloned()
            .collect::<Vec<_>>();

        Ok(payments)
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        _auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
        let limit = req.limit.map(usize::from).unwrap_or(usize::MAX);
        let payments = self
            .payments
            .lock()
            .unwrap()
            .iter()
            .filter(|(index, _p)| match req.start_index {
                Some(ref start_index) => *index > start_index,
                None => true,
            })
            .take(limit)
            .map(|(_idx, p)| p)
            .cloned()
            .collect::<Vec<DbPayment>>();

        Ok(payments)
    }

    async fn get_pending_payments(
        &self,
        _auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
        let pending_status_str = PaymentStatus::Pending.to_string();
        let payments = self
            .payments
            .lock()
            .unwrap()
            .values()
            .filter(|p| p.status == pending_status_str)
            .cloned()
            .collect::<Vec<DbPayment>>();

        Ok(payments)
    }

    async fn get_finalized_payment_ids(
        &self,
        _auth: BearerAuthToken,
    ) -> Result<Vec<LxPaymentId>, BackendApiError> {
        let completed_status_str = PaymentStatus::Completed.to_string();
        let failed_status_str = PaymentStatus::Failed.to_string();
        let payments = self
            .payments
            .lock()
            .unwrap()
            .iter()
            .filter(|(_key, p)| {
                if p.status == completed_status_str {
                    return true;
                }
                if p.status == failed_status_str {
                    return true;
                }
                false
            })
            .map(|(PaymentIndex { id, .. }, _payment)| id)
            .cloned()
            .collect::<Vec<_>>();

        Ok(payments)
    }
}

struct VirtualFileSystem {
    inner: HashMap<VfsDirectory, HashMap<FileName, Data>>,
}

impl VirtualFileSystem {
    fn new() -> Self {
        let mut inner = HashMap::new();

        // For each user, insert all directories used by the persister
        for _ in [*USER_PK1, *USER_PK2] {
            let singleton_dir = VfsDirectory {
                dirname: SINGLETON_DIRECTORY.into(),
            };
            let channel_monitors_dir = VfsDirectory {
                dirname: persister::CHANNEL_MONITORS_DIRECTORY.into(),
            };
            inner.insert(singleton_dir, HashMap::new());
            inner.insert(channel_monitors_dir, HashMap::new());
        }

        Self { inner }
    }

    fn get(&self, file_id: VfsFileId) -> Option<VfsFile> {
        let dir = VfsDirectory {
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .get(&file_id.filename)
            .map(|data| {
                VfsFile::new(dir.dirname, file_id.filename, data.clone())
            })
    }

    fn insert(&mut self, file: VfsFile) -> Option<VfsFile> {
        let dir = VfsDirectory {
            dirname: file.id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .insert(file.id.filename.clone(), file.data)
            .map(|data| VfsFile::new(dir.dirname, file.id.filename, data))
    }

    fn remove(&mut self, file_id: VfsFileId) -> Option<VfsFile> {
        let dir = VfsDirectory {
            dirname: file_id.dir.dirname,
        };
        self.inner
            .get_mut(&dir)
            .expect("Missing directory")
            .remove(&file_id.filename)
            .map(|data| VfsFile::new(dir.dirname, file_id.filename, data))
    }

    fn get_dir(&self, dir: VfsDirectory) -> Vec<VfsFile> {
        self.inner
            .get(&dir)
            .expect("Missing directory")
            .iter()
            .map(|(name, data)| {
                VfsFile::new(dir.dirname.clone(), name.clone(), data.clone())
            })
            .collect()
    }
}
