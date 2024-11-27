use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
    sync::{LazyLock, Mutex},
};

use async_trait::async_trait;
use common::{
    api::{
        auth::{
            BearerAuthRequest, BearerAuthResponse, BearerAuthToken,
            UserSignupRequest,
        },
        command::{GetNewPayments, PaymentIndexStruct, PaymentIndexes},
        def::{
            AppBackendApi, BearerAuthBackendApi, NodeBackendApi, NodeLspApi,
            NodeRunnerApi,
        },
        error::{
            BackendApiError, BackendErrorKind, LspApiError, RunnerApiError,
        },
        ports::Ports,
        provision::{MaybeSealedSeed, SealedSeed, SealedSeedId},
        user::{MaybeScid, MaybeUser, NodePk, Scid, User, UserPk},
        vfs::{MaybeVfsFile, VecVfsFile, VfsDirectory, VfsFile, VfsFileId},
        Empty,
    },
    byte_str::ByteStr,
    constants, ed25519,
    enclave::{self, Measurement},
    env::DeployEnv,
    ln::{
        network::LxNetwork,
        payments::{
            DbPayment, LxPaymentId, MaybeDbPayment, PaymentIndex, PaymentStatus,
        },
    },
    rng::SysRng,
    root_seed::RootSeed,
    time::TimestampMs,
};
use tokio::sync::mpsc;

use crate::api::BackendApiClient;

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
        DeployEnv::Dev,
        LxNetwork::Regtest,
        enclave::measurement(),
        enclave::machine_id(),
    )
    .expect("Failed to seal test root seed")
}

static SEED1: LazyLock<RootSeed> = LazyLock::new(|| RootSeed::from_u64(1));
static SEED2: LazyLock<RootSeed> = LazyLock::new(|| RootSeed::from_u64(2));

pub static USER_PK1: LazyLock<UserPk> = LazyLock::new(|| make_user_pk(&SEED1));
pub static USER_PK2: LazyLock<UserPk> = LazyLock::new(|| make_user_pk(&SEED2));

static NODE_PK1: LazyLock<NodePk> = LazyLock::new(|| make_node_pk(&SEED1));
static NODE_PK2: LazyLock<NodePk> = LazyLock::new(|| make_node_pk(&SEED2));

static SEALED_SEED1: LazyLock<SealedSeed> =
    LazyLock::new(|| make_sealed_seed(&SEED1));
static SEALED_SEED2: LazyLock<SealedSeed> =
    LazyLock::new(|| make_sealed_seed(&SEED2));

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
    notifs_tx: mpsc::Sender<Ports>,
    notifs_rx: Mutex<Option<mpsc::Receiver<Ports>>>,
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

    #[allow(dead_code)] // TODO(max): Remove
    pub(crate) fn notifs_rx(&self) -> mpsc::Receiver<Ports> {
        self.notifs_rx
            .lock()
            .unwrap()
            .take()
            .expect("Someone already subscribed")
    }
}

#[async_trait]
impl NodeRunnerApi for MockRunnerClient {
    async fn ready(&self, ports: &Ports) -> Result<Empty, RunnerApiError> {
        let _ = self.notifs_tx.try_send(*ports);
        Ok(Empty {})
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
    async fn upsert_file_with_retries(
        &self,
        data: &VfsFile,
        auth: BearerAuthToken,
        _retries: usize,
    ) -> Result<Empty, BackendApiError> {
        self.upsert_file(data, auth).await
    }
}

#[async_trait]
impl AppBackendApi for MockBackendClient {
    async fn signup(
        &self,
        _signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<Empty, BackendApiError> {
        Ok(Empty {})
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
    ) -> Result<MaybeUser, BackendApiError> {
        let user = User {
            user_pk,
            node_pk: node_pk(user_pk),
        };
        Ok(MaybeUser {
            maybe_user: Some(user),
        })
    }

    /// Always return the dummy version
    async fn get_sealed_seed(
        &self,
        data: &SealedSeedId,
    ) -> Result<MaybeSealedSeed, BackendApiError> {
        Ok(MaybeSealedSeed {
            maybe_seed: Some(sealed_seed(&data.user_pk)),
        })
    }

    async fn create_sealed_seed(
        &self,
        _data: &SealedSeed,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        Ok(Empty {})
    }

    async fn delete_sealed_seeds(
        &self,
        _measurement: Measurement,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        Ok(Empty {})
    }

    async fn get_scid(
        &self,
        _node_pk: NodePk,
        _auth: BearerAuthToken,
    ) -> Result<MaybeScid, BackendApiError> {
        Ok(MaybeScid {
            maybe_scid: Some(DUMMY_SCID),
        })
    }

    async fn get_file(
        &self,
        file_id: &VfsFileId,
        _auth: BearerAuthToken,
    ) -> Result<MaybeVfsFile, BackendApiError> {
        let maybe_file = self.vfs.lock().unwrap().get(file_id.clone());
        Ok(MaybeVfsFile { maybe_file })
    }

    async fn create_file(
        &self,
        file: &VfsFile,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let mut locked_vfs = self.vfs.lock().unwrap();
        if locked_vfs.get(file.id.clone()).is_some() {
            return Err(BackendApiError {
                kind: BackendErrorKind::Duplicate,
                msg: String::new(),
            });
        }

        let file_opt = locked_vfs.insert(file.clone());
        assert!(file_opt.is_none());
        Ok(Empty {})
    }

    async fn upsert_file(
        &self,
        file: &VfsFile,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        self.vfs.lock().unwrap().insert(file.clone());
        Ok(Empty {})
    }

    /// Returns [`Ok`] if exactly one row was deleted.
    async fn delete_file(
        &self,
        file_id: &VfsFileId,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let file_opt = self.vfs.lock().unwrap().remove(file_id.clone());
        if file_opt.is_some() {
            Ok(Empty {})
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
    ) -> Result<VecVfsFile, BackendApiError> {
        let files = self.vfs.lock().unwrap().get_dir(dir.clone());
        Ok(VecVfsFile { files })
    }

    async fn get_payment(
        &self,
        req: PaymentIndexStruct,
        _auth: BearerAuthToken,
    ) -> Result<MaybeDbPayment, BackendApiError> {
        let maybe_payment = self
            .payments
            .lock()
            .unwrap()
            .iter()
            .find(|(k, _v)| k.id == req.index.id)
            .map(|(_k, v)| v)
            .cloned();
        Ok(MaybeDbPayment { maybe_payment })
    }

    async fn create_payment(
        &self,
        payment: DbPayment,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
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
        Ok(Empty {})
    }

    async fn upsert_payment(
        &self,
        payment: DbPayment,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let created_at = TimestampMs::try_from(payment.created_at).unwrap();
        let id = LxPaymentId::from_str(&payment.id).unwrap();
        let key = PaymentIndex { created_at, id };
        self.payments.lock().unwrap().insert(key, payment);
        Ok(Empty {})
    }

    async fn upsert_payment_batch(
        &self,
        payments: Vec<DbPayment>,
        _auth: BearerAuthToken,
    ) -> Result<Empty, BackendApiError> {
        let mut locked_payments = self.payments.lock().unwrap();
        for payment in payments {
            let created_at = TimestampMs::try_from(payment.created_at).unwrap();
            let id = LxPaymentId::from_str(&payment.id).unwrap();
            let key = PaymentIndex { created_at, id };
            locked_payments.insert(key, payment);
        }
        Ok(Empty {})
    }

    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
        _auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
        let payments_lock = self.payments.lock().unwrap();
        let payments = req
            .indexes
            .into_iter()
            .filter_map(|idx| payments_lock.get(&idx).cloned())
            .collect::<Vec<_>>();
        Ok(payments)
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
        _auth: BearerAuthToken,
    ) -> Result<Vec<DbPayment>, BackendApiError> {
        let limit = req.limit.unwrap_or(constants::DEFAULT_PAYMENTS_BATCH_SIZE);
        if limit > constants::MAX_PAYMENTS_BATCH_SIZE {
            return Err(BackendApiError::batch_size_too_large());
        }

        let payments = self
            .payments
            .lock()
            .unwrap()
            .iter()
            .filter(|(index, _p)| match req.start_index {
                Some(ref start_index) => *index > start_index,
                None => true,
            })
            .take(usize::from(limit))
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
        let inner = HashMap::new();
        Self { inner }
    }

    fn get(&mut self, file_id: VfsFileId) -> Option<VfsFile> {
        let dirname = file_id.dir.dirname;
        let dir = VfsDirectory {
            dirname: dirname.clone(),
        };
        self.inner
            .entry(dir)
            .or_default()
            .get(&file_id.filename)
            .map(|data| VfsFile::new(dirname, file_id.filename, data.clone()))
    }

    fn insert(&mut self, file: VfsFile) -> Option<VfsFile> {
        let dirname = file.id.dir.dirname;
        let dir = VfsDirectory {
            dirname: dirname.clone(),
        };
        self.inner
            .entry(dir)
            .or_default()
            .insert(file.id.filename.clone(), file.data)
            .map(|data| VfsFile::new(dirname, file.id.filename, data))
    }

    fn remove(&mut self, file_id: VfsFileId) -> Option<VfsFile> {
        let dirname = file_id.dir.dirname;
        let dir = VfsDirectory {
            dirname: dirname.clone(),
        };
        self.inner
            .entry(dir)
            .or_default()
            .remove(&file_id.filename)
            .map(|data| VfsFile::new(dirname, file_id.filename, data))
    }

    fn get_dir(&mut self, dir: VfsDirectory) -> Vec<VfsFile> {
        let dirname = dir.dirname.clone();
        self.inner
            .entry(dir)
            .or_default()
            .iter()
            .map(|(name, data)| {
                VfsFile::new(dirname.clone(), name.clone(), data.clone())
            })
            .collect()
    }
}
