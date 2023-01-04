use std::cmp::{Ord, Ordering, PartialOrd};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::mem;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context};
use bdk::database::{BatchDatabase, BatchOperations, Database, SyncTime};
use bdk::{BlockTime, KeychainKind, LocalUtxo, TransactionDetails};
use bitcoin::{OutPoint, Script, Transaction, Txid};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::formats::Lowercase;
use serde_with::hex::Hex;
use serde_with::{serde_as, DisplayFromStr};
use tracing::{info, warn};

/// BDK's wallet database test suite.
#[cfg(test)]
mod bdk_test_suite;

type BdkResult<T> = Result<T, bdk::Error>;

/// Implements the DB traits required by BDK. Similar to [`MemoryDatabase`], but
/// implements batching correctly and can be entirely serialized for persisting.
/// Holds an [`Arc`] internally, so can be cloned and used directly.
///
/// [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase
#[derive(Clone, Debug)]
pub(super) struct WalletDb(Arc<Mutex<DbData>>);

#[serde_as]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct DbData {
    #[serde_as(as = "BTreeMap<DisplayFromStr, Hex<Lowercase>>")]
    path_to_script: BTreeMap<Path, Script>,
    #[serde_as(as = "BTreeMap<Hex<Lowercase>, DisplayFromStr>")]
    script_to_path: BTreeMap<Script, Path>,
    #[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
    utxos: BTreeMap<OutPoint, LocalUtxo>,
    #[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
    raw_txs: BTreeMap<Txid, Transaction>,
    #[serde_as(as = "BTreeMap<DisplayFromStr, _>")]
    tx_metas: BTreeMap<Txid, TransactionMetadata>,
    last_external_index: Option<u32>,
    last_internal_index: Option<u32>,
    sync_time: Option<SyncTime>,
    external_checksum: Option<Vec<u8>>,
    internal_checksum: Option<Vec<u8>>,
}

/// Represents a [`KeychainKind`] and corresponding child path.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
struct Path {
    keychain: KeychainKind,
    child: u32,
}

/// [`TransactionDetails`], but without the `Option<Transaction>` field.
/// This type-enforces that the raw txns (i.e. [`Transaction`]s) can only be
/// stored in the `raw_txs` map. This is what BDK's provided databases do
/// internally.
///
/// It is important to stick to this semantic because [`get_tx`] and [`del_tx`]
/// include a `include_raw` parameter which affects whether the raw tx is
/// returned or deleted respectively, and [`set_tx`] can set a raw tx if the
/// [`transaction`] field is [`Some`]. (These are in addition to the more direct
/// [`get_raw_tx`], [`set_raw_tx`], and [`del_raw_tx`] methods). BDK may rely on
/// these functions returning a specific result after a sequence of mutations,
/// so we should ensure our implementation exactly matches theirs.
///
/// [`get_tx`]: Database::get_tx
/// [`set_tx`]: BatchOperations::set_tx
/// [`del_tx`]: BatchOperations::del_tx
/// [`get_raw_tx`]: Database::get_raw_tx
/// [`set_raw_tx`]: BatchOperations::set_raw_tx
/// [`del_raw_tx`]: BatchOperations::del_raw_tx
/// [`transaction`]: TransactionDetails::transaction
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct TransactionMetadata {
    pub txid: Txid,
    pub received: u64,
    pub sent: u64,
    pub fee: Option<u64>,
    pub confirmation_time: Option<BlockTime>,
}

/// Represents a batch of `DbOp`s. Operations can be queued up for execution via
/// the [`BatchOperations`] trait methods. The underlying [`WalletDb`] is
/// guaranteed not to be affected until [`commit_batch`] is called.
/// The queued operations can be aborted by simply dropping the [`DbBatch`]
/// object.
///
/// NOTE: The `BatchOperations::del_*` methods will always return `None`
/// regardless of what has been inserted because this is what BDK's keyvalue
/// (`sled`) DB does.
///
/// [`commit_batch`]: BatchDatabase::commit_batch
pub(super) struct DbBatch(Vec<DbOp>);

/// Enumerates all database operations which can mutate the DB.
#[derive(Clone, Debug)]
enum DbOp {
    // -- BatchOperations methods -- //
    SetPathScript { path: Path, script: Script },
    DelByPath(Path),
    DelByScript(Script),
    SetUtxo(LocalUtxo),
    DelUtxo(OutPoint),
    SetRawTx(Transaction),
    DelRawTx(Txid),
    SetTx(TransactionDetails),
    DelTx { txid: Txid, include_raw: bool },
    SetLastIndex(Path),
    DelLastIndex(KeychainKind),
    SetSyncTime(SyncTime),
    DelSyncTime,
}

// --- impl Path --- //

// External = 0, Internal = 1; External < Internal
impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use KeychainKind::{External, Internal};
        match (self.keychain, other.keychain) {
            (External, Internal) => Some(Ordering::Less),
            (Internal, External) => Some(Ordering::Greater),
            // When keychain is equal, compare the child index
            (External, External) | (Internal, Internal) => {
                self.child.partial_cmp(&other.child)
            }
        }
    }
}

// External = 0, Internal = 1; External < Internal
impl Ord for Path {
    fn cmp(&self, other: &Self) -> Ordering {
        use KeychainKind::{External, Internal};
        match (self.keychain, other.keychain) {
            (External, Internal) => Ordering::Less,
            (Internal, External) => Ordering::Greater,
            // When keychain is equal, compare the child index
            (External, External) | (Internal, Internal) => {
                self.child.cmp(&other.child)
            }
        }
    }
}

impl Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keychain_str = match self.keychain {
            KeychainKind::External => "external",
            KeychainKind::Internal => "internal",
        };
        let child = &self.child;
        write!(f, "{keychain_str}@{child}")
    }
}

impl FromStr for Path {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> anyhow::Result<Self> {
        let mut parts = s.split('@');
        let (keychain_str, child_str) =
            match (parts.next(), parts.next(), parts.next()) {
                (Some(k_str), Some(c_str), None) => (k_str, c_str),
                _ => bail!("Should be in format <keychain>@<child>"),
            };

        let keychain = match keychain_str {
            "external" => KeychainKind::External,
            "internal" => KeychainKind::Internal,
            _ => bail!("Keychain should be 'external' or 'internal'"),
        };
        let child = u32::from_str(child_str).context("Invalid u32 child")?;

        Ok(Self { keychain, child })
    }
}

// --- impl TransactionMetadata --- //

impl From<TransactionDetails> for TransactionMetadata {
    fn from(tx: TransactionDetails) -> Self {
        Self {
            txid: tx.txid,
            received: tx.received,
            sent: tx.sent,
            fee: tx.fee,
            confirmation_time: tx.confirmation_time,
        }
    }
}

impl TransactionMetadata {
    fn into_tx(self, maybe_raw_tx: Option<Transaction>) -> TransactionDetails {
        TransactionDetails {
            transaction: maybe_raw_tx,
            txid: self.txid,
            received: self.received,
            sent: self.sent,
            fee: self.fee,
            confirmation_time: self.confirmation_time,
        }
    }
}

// --- impl DbBatch --- //

impl DbBatch {
    fn new() -> Self {
        Self(Vec::new())
    }
}

impl BatchOperations for DbBatch {
    fn set_script_pubkey(
        &mut self,
        script: &Script,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<()> {
        let path = Path { keychain, child };
        let script = script.clone();
        self.0.push(DbOp::SetPathScript { path, script });
        Ok(())
    }

    fn set_utxo(&mut self, utxo: &LocalUtxo) -> BdkResult<()> {
        self.0.push(DbOp::SetUtxo(utxo.clone()));
        Ok(())
    }

    fn set_raw_tx(&mut self, raw_tx: &Transaction) -> BdkResult<()> {
        self.0.push(DbOp::SetRawTx(raw_tx.clone()));
        Ok(())
    }

    fn set_tx(&mut self, tx: &TransactionDetails) -> BdkResult<()> {
        self.0.push(DbOp::SetTx(tx.clone()));
        Ok(())
    }

    fn set_last_index(
        &mut self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<()> {
        self.0.push(DbOp::SetLastIndex(Path { keychain, child }));
        Ok(())
    }

    fn set_sync_time(&mut self, time: SyncTime) -> BdkResult<()> {
        self.0.push(DbOp::SetSyncTime(time));
        Ok(())
    }

    fn del_script_pubkey_from_path(
        &mut self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<Option<Script>> {
        self.0.push(DbOp::DelByPath(Path { keychain, child }));
        Ok(None)
    }

    fn del_path_from_script_pubkey(
        &mut self,
        script: &Script,
    ) -> BdkResult<Option<(KeychainKind, u32)>> {
        self.0.push(DbOp::DelByScript(script.clone()));
        Ok(None)
    }

    fn del_utxo(
        &mut self,
        outpoint: &OutPoint,
    ) -> BdkResult<Option<LocalUtxo>> {
        self.0.push(DbOp::DelUtxo(*outpoint));
        Ok(None)
    }

    fn del_raw_tx(&mut self, txid: &Txid) -> BdkResult<Option<Transaction>> {
        self.0.push(DbOp::DelRawTx(*txid));
        Ok(None)
    }

    fn del_tx(
        &mut self,
        &txid: &Txid,
        include_raw: bool,
    ) -> BdkResult<Option<TransactionDetails>> {
        self.0.push(DbOp::DelTx { txid, include_raw });
        Ok(None)
    }

    fn del_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> BdkResult<Option<u32>> {
        self.0.push(DbOp::DelLastIndex(keychain));
        Ok(None)
    }

    fn del_sync_time(&mut self) -> BdkResult<Option<SyncTime>> {
        self.0.push(DbOp::DelSyncTime);
        Ok(None)
    }
}

// --- impl DbOp --- //

impl DbOp {
    /// Executes the operation.
    #[allow(unused_must_use)]
    fn do_op<DB: BatchOperations>(self, db: &mut DB) {
        match self {
            DbOp::SetPathScript {
                path: Path { keychain, child },
                script,
            } => {
                db.set_script_pubkey(&script, keychain, child);
            }
            DbOp::DelByPath(Path { keychain, child }) => {
                db.del_script_pubkey_from_path(keychain, child);
            }
            DbOp::DelByScript(script) => {
                db.del_path_from_script_pubkey(&script);
            }
            DbOp::SetUtxo(utxo) => {
                db.set_utxo(&utxo);
            }
            DbOp::DelUtxo(outpoint) => {
                db.del_utxo(&outpoint);
            }
            DbOp::SetRawTx(raw_tx) => {
                db.set_raw_tx(&raw_tx);
            }
            DbOp::DelRawTx(txid) => {
                db.del_raw_tx(&txid);
            }
            DbOp::SetTx(tx) => {
                db.set_tx(&tx);
            }
            DbOp::DelTx { txid, include_raw } => {
                db.del_tx(&txid, include_raw);
            }
            DbOp::SetLastIndex(Path { keychain, child }) => {
                db.set_last_index(keychain, child);
            }
            DbOp::DelLastIndex(keychain) => {
                db.del_last_index(keychain);
            }
            DbOp::SetSyncTime(time) => {
                db.set_sync_time(time);
            }
            DbOp::DelSyncTime => {
                db.del_sync_time();
            }
        };
    }

    /// Executes the operation and asserts op-specific invariants. Requires a
    /// [`Database`] impl in order to check the op-specific invariants
    #[cfg(test)]
    fn do_op_and_assert<DB: BatchOperations + Database>(self, db: &mut DB) {
        match self {
            DbOp::SetPathScript {
                path: Path { keychain, child },
                script,
            } => {
                let out = db.set_script_pubkey(&script, keychain, child);

                assert!(out.is_ok());
                let get_script = db
                    .get_script_pubkey_from_path(keychain, child)
                    .unwrap()
                    .unwrap();
                let (get_keychain, get_child) =
                    db.get_path_from_script_pubkey(&script).unwrap().unwrap();
                assert_eq!(get_script, script);
                assert_eq!(get_keychain, keychain);
                assert_eq!(get_child, child);
            }
            DbOp::DelByPath(Path { keychain, child }) => {
                let out = db.del_script_pubkey_from_path(keychain, child);

                if let Some(script) = out.as_ref().unwrap() {
                    assert!(db
                        .get_path_from_script_pubkey(script)
                        .unwrap()
                        .is_none());
                }
                assert!(db
                    .get_script_pubkey_from_path(keychain, child)
                    .unwrap()
                    .is_none());
            }
            DbOp::DelByScript(script) => {
                let out = db.del_path_from_script_pubkey(&script);

                if let Some((_keychain, _child)) = out.as_ref().unwrap() {
                    // TODO(max): Bug report filed (BDK#829), confirm what
                    // the intended behavior is then fix
                    // assert!(db
                    //     .get_script_pubkey_from_path(*keychain, *child)
                    //     .unwrap()
                    //     .is_none());
                }
                assert!(db
                    .get_path_from_script_pubkey(&script)
                    .unwrap()
                    .is_none());
            }
            DbOp::SetUtxo(utxo) => {
                let out = db.set_utxo(&utxo);

                assert!(out.is_ok());
                let get_utxo = db.get_utxo(&utxo.outpoint).unwrap().unwrap();
                assert_eq!(get_utxo, utxo);
            }
            DbOp::DelUtxo(outpoint) => {
                let out = db.del_utxo(&outpoint);

                assert!(out.is_ok());
                assert!(db.get_utxo(&outpoint).unwrap().is_none());
            }
            DbOp::SetRawTx(raw_tx) => {
                let txid = raw_tx.txid();
                let out = db.set_raw_tx(&raw_tx);

                assert!(out.is_ok());
                let get_raw_tx = db.get_raw_tx(&txid).unwrap().unwrap();
                assert_eq!(get_raw_tx, raw_tx);
            }
            DbOp::DelRawTx(txid) => {
                let out = db.del_raw_tx(&txid);

                assert!(out.is_ok());
                assert!(db.get_raw_tx(&txid).unwrap().is_none());
            }
            DbOp::SetTx(tx) => {
                let include_raw = tx.transaction.is_some();
                let txid = &tx.txid;

                let out = db.set_tx(&tx);

                assert!(out.is_ok());
                let get_tx = db.get_tx(txid, include_raw).unwrap().unwrap();
                assert_eq!(get_tx, tx);
                if include_raw {
                    let raw_tx = tx.transaction.unwrap();
                    let get_raw_tx = db.get_raw_tx(txid).unwrap().unwrap();
                    assert_eq!(get_raw_tx, raw_tx);
                }
            }
            DbOp::DelTx { txid, include_raw } => {
                let out = db.del_tx(&txid, include_raw);

                assert!(out.is_ok());
                assert!(db.get_tx(&txid, include_raw).unwrap().is_none());
                if include_raw {
                    assert!(db.get_raw_tx(&txid).unwrap().is_none());
                }
            }
            DbOp::SetLastIndex(Path { keychain, child }) => {
                let out = db.set_last_index(keychain, child);

                assert!(out.is_ok());
                let after = db.get_last_index(keychain).unwrap().unwrap();
                assert_eq!(after, child);
            }
            DbOp::DelLastIndex(keychain) => {
                let out = db.del_last_index(keychain);

                assert!(out.is_ok());
                assert!(db.get_last_index(keychain).unwrap().is_none());
            }
            DbOp::SetSyncTime(time) => {
                let out = db.set_sync_time(time.clone());

                assert!(out.is_ok());
                let get_time = db.get_sync_time().unwrap().unwrap();
                // SyncTime doesn't derive PartialEq for some reason
                // TODO(max): Submit PR upstream to derive PartialEq
                assert_eq!(get_time.block_time, time.block_time);
            }
            DbOp::DelSyncTime => {
                let out = db.del_sync_time();

                assert!(out.is_ok());
                assert!(db.get_sync_time().unwrap().is_none());
            }
        }
    }
}

// --- impl WalletDb --- //

impl WalletDb {
    pub(super) fn new() -> Self {
        let path_to_script = BTreeMap::new();
        let script_to_path = BTreeMap::new();
        let utxos = BTreeMap::new();
        let raw_txs = BTreeMap::new();
        let tx_metas = BTreeMap::new();
        let last_external_index = None;
        let last_internal_index = None;
        let sync_time = None;
        let external_checksum = None;
        let internal_checksum = None;

        let inner = DbData {
            path_to_script,
            script_to_path,
            utxos,
            raw_txs,
            tx_metas,
            last_external_index,
            last_internal_index,
            sync_time,
            external_checksum,
            internal_checksum,
        };

        Self(Arc::new(Mutex::new(inner)))
    }

    #[cfg(test)]
    fn assert_invariants(&self) {
        // FIXME(max): Right now the commented code breaks the proptest.
        // Currently awaiting clarification from BDK on what the expected
        // behavior is when multiple paths map to the same key.

        // Everything in path_to_script must be in script_to_path and vice versa
        let db = self.0.lock().unwrap();
        // for (path1, script1) in db.path_to_script.iter() {
        //     let path2 = db.script_to_path.get(script1).unwrap();
        //     assert_eq!(path1, path2);
        // }
        for (script2, path2) in db.script_to_path.iter() {
            let script1 = db.path_to_script.get(path2).unwrap();
            assert_eq!(script1, script2);
        }
    }
}

#[cfg(test)]
impl PartialEq for WalletDb {
    fn eq(&self, other: &WalletDb) -> bool {
        let self_lock = self.0.lock().unwrap();
        let other_lock = other.0.lock().unwrap();
        self_lock.eq(&other_lock)
    }
}

impl Database for WalletDb {
    // BDK wants us to store the first checksum we see, then check all future
    // given checksums against it. Sure, we can do that...
    fn check_descriptor_checksum<B: AsRef<[u8]>>(
        &mut self,
        keychain: KeychainKind,
        given_checksum: B,
    ) -> BdkResult<()> {
        self.0
            .lock()
            .unwrap()
            .check_descriptor_checksum(keychain, given_checksum)
    }

    fn iter_script_pubkeys(
        &self,
        maybe_filter_keychain: Option<KeychainKind>,
    ) -> BdkResult<Vec<Script>> {
        self.0
            .lock()
            .unwrap()
            .iter_script_pubkeys(maybe_filter_keychain)
    }

    fn iter_utxos(&self) -> BdkResult<Vec<LocalUtxo>> {
        self.0.lock().unwrap().iter_utxos()
    }

    fn iter_raw_txs(&self) -> BdkResult<Vec<Transaction>> {
        self.0.lock().unwrap().iter_raw_txs()
    }

    fn iter_txs(
        &self,
        include_raw: bool,
    ) -> BdkResult<Vec<TransactionDetails>> {
        self.0.lock().unwrap().iter_txs(include_raw)
    }

    fn get_script_pubkey_from_path(
        &self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<Option<Script>> {
        self.0
            .lock()
            .unwrap()
            .get_script_pubkey_from_path(keychain, child)
    }

    fn get_path_from_script_pubkey(
        &self,
        script: &Script,
    ) -> BdkResult<Option<(KeychainKind, u32)>> {
        self.0.lock().unwrap().get_path_from_script_pubkey(script)
    }

    fn get_utxo(&self, outpoint: &OutPoint) -> BdkResult<Option<LocalUtxo>> {
        self.0.lock().unwrap().get_utxo(outpoint)
    }

    fn get_raw_tx(&self, txid: &Txid) -> BdkResult<Option<Transaction>> {
        self.0.lock().unwrap().get_raw_tx(txid)
    }

    fn get_tx(
        &self,
        txid: &Txid,
        include_raw: bool,
    ) -> BdkResult<Option<TransactionDetails>> {
        self.0.lock().unwrap().get_tx(txid, include_raw)
    }

    fn get_last_index(&self, keychain: KeychainKind) -> BdkResult<Option<u32>> {
        self.0.lock().unwrap().get_last_index(keychain)
    }

    fn get_sync_time(&self) -> BdkResult<Option<SyncTime>> {
        self.0.lock().unwrap().get_sync_time()
    }

    fn increment_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> BdkResult<u32> {
        self.0.lock().unwrap().increment_last_index(keychain)
    }
}

impl BatchOperations for WalletDb {
    // Weird that the set_* methods take ref, but ok
    fn set_script_pubkey(
        &mut self,
        script: &Script,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<()> {
        self.0
            .lock()
            .unwrap()
            .set_script_pubkey(script, keychain, child)
    }

    fn set_utxo(&mut self, utxo: &LocalUtxo) -> BdkResult<()> {
        self.0.lock().unwrap().set_utxo(utxo)
    }

    fn set_raw_tx(&mut self, raw_tx: &Transaction) -> BdkResult<()> {
        self.0.lock().unwrap().set_raw_tx(raw_tx)
    }

    fn set_tx(&mut self, tx: &TransactionDetails) -> BdkResult<()> {
        self.0.lock().unwrap().set_tx(tx)
    }

    fn set_last_index(
        &mut self,
        keychain: KeychainKind,
        index: u32,
    ) -> BdkResult<()> {
        self.0.lock().unwrap().set_last_index(keychain, index)
    }

    fn set_sync_time(&mut self, time: SyncTime) -> BdkResult<()> {
        self.0.lock().unwrap().set_sync_time(time)
    }

    fn del_script_pubkey_from_path(
        &mut self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<Option<Script>> {
        self.0
            .lock()
            .unwrap()
            .del_script_pubkey_from_path(keychain, child)
    }

    fn del_path_from_script_pubkey(
        &mut self,
        script: &Script,
    ) -> BdkResult<Option<(KeychainKind, u32)>> {
        self.0.lock().unwrap().del_path_from_script_pubkey(script)
    }

    fn del_utxo(
        &mut self,
        outpoint: &OutPoint,
    ) -> BdkResult<Option<LocalUtxo>> {
        self.0.lock().unwrap().del_utxo(outpoint)
    }

    fn del_raw_tx(&mut self, txid: &Txid) -> BdkResult<Option<Transaction>> {
        self.0.lock().unwrap().del_raw_tx(txid)
    }

    fn del_tx(
        &mut self,
        txid: &Txid,
        include_raw: bool,
    ) -> BdkResult<Option<TransactionDetails>> {
        self.0.lock().unwrap().del_tx(txid, include_raw)
    }

    fn del_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> BdkResult<Option<u32>> {
        self.0.lock().unwrap().del_last_index(keychain)
    }

    fn del_sync_time(&mut self) -> BdkResult<Option<SyncTime>> {
        self.0.lock().unwrap().del_sync_time()
    }
}

impl BatchDatabase for WalletDb {
    type Batch = DbBatch;

    fn begin_batch(&self) -> Self::Batch {
        info!("Beginning WalletDb batch");
        DbBatch::new()
    }

    fn commit_batch(&mut self, batch: Self::Batch) -> BdkResult<()> {
        info!("Committing WalletDb batch");
        for op in batch.0 {
            op.do_op(self);
        }
        // TODO(max): Serialize then persist the WalletDb
        Ok(())
    }
}

impl Serialize for WalletDb {
    fn serialize<S: Serializer>(
        &self,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let data = self.0.lock().unwrap();
        DbData::serialize(&*data, serializer)
    }
}

impl<'de> Deserialize<'de> for WalletDb {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        DbData::deserialize(deserializer)
            .map(Mutex::new)
            .map(Arc::new)
            .map(WalletDb)
    }
}

// --- impl DbData --- //

impl Database for DbData {
    // BDK wants us to store the first checksum we see, then check all future
    // given checksums against it. Sure, we can do that...
    fn check_descriptor_checksum<B: AsRef<[u8]>>(
        &mut self,
        keychain: KeychainKind,
        given_checksum: B,
    ) -> BdkResult<()> {
        // First, get a &mut Option<Vec<u8>> for the correct keychain
        let mut_checksum = match keychain {
            KeychainKind::External => &mut self.external_checksum,
            KeychainKind::Internal => &mut self.internal_checksum,
        };

        // Get the saved checksum, lazily inserting the given one if it was None
        let saved_checksum = mut_checksum
            .get_or_insert_with(|| given_checksum.as_ref().to_vec());

        // Check the saved checksum against the given one
        if saved_checksum.as_slice() == given_checksum.as_ref() {
            Ok(())
        } else {
            Err(bdk::Error::ChecksumMismatch)
        }
    }

    fn iter_script_pubkeys(
        &self,
        maybe_filter_keychain: Option<KeychainKind>,
    ) -> BdkResult<Vec<Script>> {
        let vec = match maybe_filter_keychain {
            Some(filter_keychain) => self
                .path_to_script
                .iter()
                .filter(|(p, _s)| {
                    mem::discriminant(&p.keychain)
                        == mem::discriminant(&filter_keychain)
                })
                .map(|(_p, s)| s)
                .cloned()
                .collect(),
            None => self.path_to_script.values().cloned().collect(),
        };
        Ok(vec)
    }

    fn iter_utxos(&self) -> BdkResult<Vec<LocalUtxo>> {
        Ok(self.utxos.values().cloned().collect())
    }

    fn iter_raw_txs(&self) -> BdkResult<Vec<Transaction>> {
        Ok(self.raw_txs.values().cloned().collect())
    }

    fn iter_txs(
        &self,
        include_raw: bool,
    ) -> BdkResult<Vec<TransactionDetails>> {
        let mut txs = self
            .tx_metas
            .values()
            .cloned()
            .map(|meta| meta.into_tx(None))
            .collect::<Vec<_>>();

        if include_raw {
            // Include any known raw_txs
            for tx in txs.iter_mut() {
                let maybe_raw_tx = self.raw_txs.get(&tx.txid).cloned();
                tx.transaction = maybe_raw_tx;
            }
        }

        Ok(txs)
    }

    fn get_script_pubkey_from_path(
        &self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<Option<Script>> {
        let path = Path { keychain, child };
        Ok(self.path_to_script.get(&path).cloned())
    }

    fn get_path_from_script_pubkey(
        &self,
        script: &Script,
    ) -> BdkResult<Option<(KeychainKind, u32)>> {
        self.script_to_path
            .get(script)
            .map(|path| (path.keychain, path.child))
            .map(Ok)
            .transpose()
    }

    fn get_utxo(&self, outpoint: &OutPoint) -> BdkResult<Option<LocalUtxo>> {
        Ok(self.utxos.get(outpoint).cloned())
    }

    fn get_raw_tx(&self, txid: &Txid) -> BdkResult<Option<Transaction>> {
        Ok(self.raw_txs.get(txid).cloned())
    }

    fn get_tx(
        &self,
        txid: &Txid,
        include_raw: bool,
    ) -> BdkResult<Option<TransactionDetails>> {
        let maybe_raw_tx = if include_raw {
            self.raw_txs.get(txid).cloned()
        } else {
            None
        };

        self.tx_metas
            .get(txid)
            .cloned()
            .map(|meta| meta.into_tx(maybe_raw_tx))
            .map(Ok)
            .transpose()
    }

    fn get_last_index(&self, keychain: KeychainKind) -> BdkResult<Option<u32>> {
        match keychain {
            KeychainKind::External => Ok(self.last_external_index),
            KeychainKind::Internal => Ok(self.last_internal_index),
        }
    }

    fn get_sync_time(&self) -> BdkResult<Option<SyncTime>> {
        Ok(self.sync_time.clone())
    }

    fn increment_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> BdkResult<u32> {
        // Get a &mut Option<u32> corresponding to the appropriate field
        let mut_last_index = match keychain {
            KeychainKind::External => &mut self.last_external_index,
            KeychainKind::Internal => &mut self.last_internal_index,
        };

        // Increment if the index existed
        if let Some(index) = mut_last_index {
            *index += 1;
        }

        // Get the index, inserting 0 if it was None
        let last_index = *mut_last_index.get_or_insert(0);

        Ok(last_index)
    }
}

impl BatchOperations for DbData {
    // Weird that the set_* methods give ref, but ok
    fn set_script_pubkey(
        &mut self,
        script: &Script,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<()> {
        let new_path = Path { keychain, child };
        self.path_to_script.insert(new_path.clone(), script.clone());
        self.script_to_path
            .insert(script.clone(), new_path.clone())
            .inspect(|old_path| {
                if *old_path != new_path {
                    warn!(
                        "Old {old_path:?} and new {new_path:?} map to the same \
                        script; Querying the path by script will return the \
                        new path."
                    )
                }
            });

        Ok(())
    }

    fn set_utxo(&mut self, utxo: &LocalUtxo) -> BdkResult<()> {
        self.utxos.insert(utxo.outpoint, utxo.clone());
        Ok(())
    }

    fn set_raw_tx(&mut self, raw_tx: &Transaction) -> BdkResult<()> {
        self.raw_txs.insert(raw_tx.txid(), raw_tx.clone());
        Ok(())
    }

    fn set_tx(&mut self, tx: &TransactionDetails) -> BdkResult<()> {
        let mut tx = tx.clone();
        // take() the raw tx, inserting it into the raw_txs map if it existed
        if let Some(raw_tx) = tx.transaction.take() {
            self.raw_txs.insert(tx.txid, raw_tx);
        }

        // Convert to metadata and store the metadata
        let meta = TransactionMetadata::from(tx);
        self.tx_metas.insert(meta.txid, meta);

        Ok(())
    }

    fn set_last_index(
        &mut self,
        keychain: KeychainKind,
        index: u32,
    ) -> BdkResult<()> {
        match keychain {
            KeychainKind::External => self.last_external_index.insert(index),
            KeychainKind::Internal => self.last_internal_index.insert(index),
        };
        Ok(())
    }

    fn set_sync_time(&mut self, time: SyncTime) -> BdkResult<()> {
        self.sync_time = Some(time);
        Ok(())
    }

    fn del_script_pubkey_from_path(
        &mut self,
        keychain: KeychainKind,
        child: u32,
    ) -> BdkResult<Option<Script>> {
        let path = Path { keychain, child };

        self.path_to_script
            .remove(&path)
            .inspect(|script| {
                self.script_to_path.remove(script);
            })
            .map(Ok)
            .transpose()
    }

    fn del_path_from_script_pubkey(
        &mut self,
        script: &Script,
    ) -> BdkResult<Option<(KeychainKind, u32)>> {
        self.script_to_path
            .remove(script)
            .inspect(|path| {
                self.path_to_script.remove(path);
            })
            .map(|path| (path.keychain, path.child))
            .map(Ok)
            .transpose()
    }

    fn del_utxo(
        &mut self,
        outpoint: &OutPoint,
    ) -> BdkResult<Option<LocalUtxo>> {
        Ok(self.utxos.remove(outpoint))
    }

    fn del_raw_tx(&mut self, txid: &Txid) -> BdkResult<Option<Transaction>> {
        Ok(self.raw_txs.remove(txid))
    }

    fn del_tx(
        &mut self,
        txid: &Txid,
        include_raw: bool,
    ) -> BdkResult<Option<TransactionDetails>> {
        // Delete the raw tx if include_raw == true, then return the raw tx with
        // the tx if one existed.
        let maybe_raw_tx = if include_raw {
            self.raw_txs.remove(txid)
        } else {
            None
        };

        self.tx_metas
            .remove(txid)
            .map(|meta| meta.into_tx(maybe_raw_tx))
            .map(Ok)
            .transpose()
    }

    fn del_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> BdkResult<Option<u32>> {
        match keychain {
            KeychainKind::External => self.last_external_index.take(),
            KeychainKind::Internal => self.last_internal_index.take(),
        }
        .map(Ok)
        .transpose()
    }

    fn del_sync_time(&mut self) -> BdkResult<Option<SyncTime>> {
        Ok(self.sync_time.take())
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::{arbitrary, roundtrip};
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Just, Strategy};
    use proptest::test_runner::Config;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    /// An `Arbitrary`-like [`Strategy`] for [`KeychainKind`].
    fn any_keychain() -> BoxedStrategy<KeychainKind> {
        any::<bool>()
            .prop_map(|external| {
                if external {
                    KeychainKind::External
                } else {
                    KeychainKind::Internal
                }
            })
            .boxed()
    }

    /// An `Arbitrary`-like [`Strategy`] for [`LocalUtxo`].
    fn any_utxo() -> BoxedStrategy<LocalUtxo> {
        (
            arbitrary::any_outpoint(),
            arbitrary::any_txout(),
            any_keychain(),
            any::<bool>(),
        )
            .prop_map(|(outpoint, txout, keychain, is_spent)| LocalUtxo {
                outpoint,
                txout,
                keychain,
                is_spent,
            })
            .boxed()
    }

    /// An `Arbitrary`-like [`Strategy`] for [`BlockTime`].
    fn any_block_time() -> BoxedStrategy<BlockTime> {
        (any::<u32>(), any::<u64>())
            .prop_map(|(height, timestamp)| BlockTime { height, timestamp })
            .boxed()
    }

    /// An `Arbitrary`-like [`Strategy`] for [`SyncTime`].
    fn any_sync_time() -> BoxedStrategy<SyncTime> {
        any_block_time()
            .prop_map(|block_time| SyncTime { block_time })
            .boxed()
    }

    /// An `Arbitrary`-like [`Strategy`] for [`TransactionDetails`].
    fn any_tx() -> BoxedStrategy<TransactionDetails> {
        (
            arbitrary::any_raw_tx(),
            any::<bool>(),
            any::<u64>(),
            any::<u64>(),
            any::<Option<u64>>(),
            any_block_time(),
            any::<bool>(),
        )
            .prop_map(
                |(
                    raw_tx,
                    include_raw_tx,
                    received,
                    sent,
                    fee,
                    block_time,
                    include_block_time,
                )| {
                    let txid = raw_tx.txid();
                    let transaction =
                        if include_raw_tx { Some(raw_tx) } else { None };
                    let confirmation_time = if include_block_time {
                        Some(block_time)
                    } else {
                        None
                    };

                    TransactionDetails {
                        transaction,
                        txid,
                        received,
                        sent,
                        fee,
                        confirmation_time,
                    }
                },
            )
            .boxed()
    }

    impl Arbitrary for WalletDb {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            // Apply an arbitrary vec of operations to generate the db
            let any_op = any::<DbOp>();
            proptest::collection::vec(any_op, 0..20)
                .prop_map(|vec_of_ops| {
                    let mut db = WalletDb::new();
                    for op in vec_of_ops {
                        op.do_op_and_assert(&mut db);
                    }
                    db
                })
                .boxed()
        }
    }

    impl Arbitrary for Path {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_keychain = any_keychain();
            let any_child = any::<u32>();
            (any_keychain, any_child)
                .prop_map(|(keychain, child)| Self { keychain, child })
                .boxed()
        }
    }

    impl Arbitrary for TransactionMetadata {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any_tx().prop_map(Self::from).boxed()
        }
    }

    impl Arbitrary for DbOp {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            use DbOp::*;
            match DelSyncTime {
                SetPathScript { .. }
                | DelByPath(_)
                | DelByScript(_)
                | SetUtxo(_)
                | DelUtxo(_)
                | SetRawTx(_)
                | DelRawTx(_)
                | SetTx(_)
                | DelTx { .. }
                | SetLastIndex(_)
                | DelLastIndex(_)
                | SetSyncTime(_)
                | DelSyncTime => {
                    "This match statement was written to remind you to add the \
                    new enum variant you just created to the prop_oneof below!"
                }
            };

            proptest::prop_oneof![
                // SetRawTx, DelByPath, DelByScript
                (any::<Path>(), arbitrary::any_script())
                    .prop_map(|(path, script)| SetPathScript { path, script }),
                any::<Path>().prop_map(Self::DelByPath),
                arbitrary::any_script().prop_map(Self::DelByScript),
                // SetUtxo, DelUtxo
                any_utxo().prop_map(Self::SetUtxo),
                arbitrary::any_outpoint().prop_map(Self::DelUtxo),
                // SetRawTx, DelRawTx, SetTx, DelTx
                arbitrary::any_raw_tx().prop_map(Self::SetRawTx),
                arbitrary::any_txid().prop_map(Self::DelRawTx),
                any_tx().prop_map(Self::SetTx),
                (arbitrary::any_txid(), any::<bool>()).prop_map(
                    |(txid, include_raw)| Self::DelTx { txid, include_raw }
                ),
                // Individual fields
                any::<Path>().prop_map(Self::SetLastIndex),
                any_keychain().prop_map(Self::DelLastIndex),
                any_sync_time().prop_map(Self::SetSyncTime),
                Just(Self::DelSyncTime),
            ]
            .boxed()
        }
    }

    /// Tests that [`WalletDb::iter_script_pubkeys`] filters according to
    /// [`KeychainKind`].
    #[test]
    fn iter_script_pubkeys_filters() {
        use KeychainKind::{External, Internal};
        let mut wallet_db = WalletDb::new();

        // Populate the db
        let script1 = Script::from(vec![1]);
        let script2 = Script::from(vec![2]);
        let script3 = Script::from(vec![3]);
        wallet_db.set_script_pubkey(&script1, External, 1).unwrap();
        wallet_db.set_script_pubkey(&script2, External, 2).unwrap();
        wallet_db.set_script_pubkey(&script3, Internal, 3).unwrap();

        // Giving no filter should return all 3 elements
        let mut iter = wallet_db.iter_script_pubkeys(None).unwrap().into_iter();
        match (iter.next(), iter.next(), iter.next(), iter.next()) {
            (Some(s1), Some(s2), Some(s3), None) => {
                assert_eq!(script1, s1);
                assert_eq!(script2, s2);
                assert_eq!(script3, s3);
            }
            _ => panic!("Unexpected"),
        }

        // Filtering by External should return 2 elements (script 1 and 2)
        let mut iter = wallet_db
            .iter_script_pubkeys(Some(External))
            .unwrap()
            .into_iter();
        match (iter.next(), iter.next(), iter.next()) {
            (Some(s1), Some(s2), None) => {
                assert_eq!(script1, s1);
                assert_eq!(script2, s2);
            }
            _ => panic!("Unexpected"),
        }

        // Filtering by Internal should return 1 element (script 3)
        let mut iter = wallet_db
            .iter_script_pubkeys(Some(Internal))
            .unwrap()
            .into_iter();
        match (iter.next(), iter.next()) {
            (Some(s3), None) => assert_eq!(script3, s3),
            _ => panic!("Unexpected"),
        }
    }

    /// Checks that increment_last_index() actually increments the index. Since
    /// `Option<u32>` is Copy it's easy to accidentally mutate a copy (instead
    /// of the original) in e.g. an Option chain.
    #[test]
    fn increment_actually_increments() {
        let mut db = WalletDb::new();
        let keychain = KeychainKind::Internal;

        assert_eq!(db.get_last_index(keychain).unwrap(), None);
        db.increment_last_index(keychain).unwrap();
        assert_eq!(db.get_last_index(keychain).unwrap(), Some(0));
        db.increment_last_index(keychain).unwrap();
        assert_eq!(db.get_last_index(keychain).unwrap(), Some(1));
        db.increment_last_index(keychain).unwrap();
        assert_eq!(db.get_last_index(keychain).unwrap(), Some(2));
        db.increment_last_index(keychain).unwrap();
        assert_eq!(db.get_last_index(keychain).unwrap(), Some(3));
    }

    /// Generates an arbitrary `Vec<DbOp>` and executes each op,
    /// checking op invariants as well as db invariants in between.
    #[test]
    fn wallet_db_maintains_invariants() {
        let any_op = any::<DbOp>();
        let any_vec_of_ops = proptest::collection::vec(any_op, 0..20);
        // We only test one case, otherwise this test takes several minutes.
        proptest!(Config::with_cases(1), |(vec_of_ops in any_vec_of_ops)| {
            let mut db = WalletDb::new();

            db.assert_invariants();

            for op in vec_of_ops {
                op.do_op_and_assert(&mut db);

                db.assert_invariants();
            }
        })
    }

    /// Tests that the [`FromStr`] / [`Display`], [`FromStr`] / [`LowerHex`],
    /// and [`Serialize`] / [`Deserialize`] impls of [`WalletDb`] fields
    /// roundtrip, because these impls are used when serializing the
    /// [`WalletDb`] as a whole. See the [`serde_as`] annotations on [`DbData`]
    /// for more information.
    #[test]
    fn wallet_db_fields_roundtrips() {
        use roundtrip::*;

        // This test takes a while, so we only try 16 cases for each field.
        let config = Config::with_cases(16);

        // Path
        fromstr_display_custom(any::<Path>(), config.clone());
        // Script
        fromstr_lowerhex_custom(arbitrary::any_script(), config.clone());
        // OutPoint
        fromstr_display_custom(arbitrary::any_outpoint(), config.clone());
        // LocalUtxo
        json_value_custom(any_utxo(), config.clone());
        // Txid
        fromstr_display_custom(arbitrary::any_txid(), config.clone());
        // Transaction
        json_value_custom(arbitrary::any_raw_tx(), config.clone());
        // TransactionMetadata
        json_value_custom(any::<TransactionMetadata>(), config.clone());
        // SyncTime
        json_value_custom(any_sync_time(), config);
    }

    /// Tests that the [`WalletDb`] as a whole roundtrips to/from a JSON object
    #[test]
    fn wallet_db_serde_json_roundtrip() {
        // Configure this test to run only one iteration,
        // otherwise this test alone takes several minutes.
        let config = Config::with_cases(1);
        roundtrip::json_value_custom(any::<WalletDb>(), config);
    }

    /// After uncommenting out the contents of `assert_invariants`, this test
    /// reproduces the test failure caused by nonbijective path -> script data.
    // TODO(max): Clarify with BDK on guarantees / expected behavior, then fix
    #[test]
    fn regression_nonbijective_path_script_mapping() {
        let mut db = WalletDb::new();
        let keychain = KeychainKind::External;
        let path1 = Path { keychain, child: 0 };
        let path2 = Path { keychain, child: 1 };
        let script = Script::new();
        let op1 = DbOp::SetPathScript {
            path: path1,
            script: script.clone(),
        };
        let op2 = DbOp::SetPathScript {
            path: path2,
            script,
        };

        db.assert_invariants();

        op1.do_op_and_assert(&mut db);
        println!("Post OP1: {}", serde_json::to_string_pretty(&db).unwrap());
        db.assert_invariants();

        op2.do_op_and_assert(&mut db);
        println!("Post OP2: {}", serde_json::to_string_pretty(&db).unwrap());
        db.assert_invariants();
    }

    /// Tests that possibly-updated deserialization logic can deserialize a
    /// [`WalletDb`] that was serialized on 2022-12-24 (backwards-compatibility
    /// test). This test can be removed if all nodes have migrated to the newer
    /// serialization scheme.
    ///
    /// NOTE: The data in the serialized wallet db is not guaranteed to be
    /// consensus-valid, or even valid enough to be propagated. If this test
    /// broke, it is possible that it was due to increased validation in a
    /// [`serde::Deserialize`] impl used for one of the contained data types.
    /// TODO(max): Generate a snapshot with more "realistic" data.
    #[test]
    fn deserialize_2022_12_24_snapshot() {
        // The following code generated the db_json_str below.
        /*
        let mut runner = proptest::test_runner::TestRunner::default();
        let mut db = WalletDb::new();

        // To ensure each field of the WalletDb contains at least one element,
        // sample DbOps until we've executed at least one of each of the below:
        // SetPathScript, SetUtxo, SetRawTx, SetTx, SetLastIndex, SetSyncTime.
        // We mark a slot as Some after we have executed that op.
        let mut seen: [Option<()>; 6] = [None; 6];
        while seen.contains(&None) {
            let op = any::<DbOp>().new_tree(&mut runner).unwrap().current();
            let maybe_index = match op {
                DbOp::SetPathScript { .. } => Some(0),
                DbOp::SetUtxo(_) => Some(1),
                DbOp::SetRawTx(_) => Some(2),
                DbOp::SetTx(_) => Some(3),
                DbOp::SetLastIndex(_) => Some(4),
                DbOp::SetSyncTime(_) => Some(5),
                _ => None,
            };
            if let Some(index) = maybe_index {
                if seen[index].replace(()).is_none() {
                    op.do_op_and_assert(&mut db)
                }
            }
        }
        let json_str = serde_json::to_string(&db).unwrap();
        println!("{json_str}");
        panic!();
        */

        let db_json_str = "{\"path_to_script\":{\"internal@2431873833\":\"08f48768401aa152500006ca1ac0aa1d272103a1e61d1211e949668e3fd57b6f79d668b89ed6a37ff7ac5561f8fdb0e78361620854e9c93cf102c7e521037521401037d7cf567da4315b8c46a851d243c603a142e6c066d2c2b58a57b24d\"},\"script_to_path\":{\"08f48768401aa152500006ca1ac0aa1d272103a1e61d1211e949668e3fd57b6f79d668b89ed6a37ff7ac5561f8fdb0e78361620854e9c93cf102c7e521037521401037d7cf567da4315b8c46a851d243c603a142e6c066d2c2b58a57b24d\":\"internal@2431873833\"},\"utxos\":{\"630a8e1c3d2d2eb8b317e8269a87a0390a7d6dd4ada3b71da859207ccaae14b1:1110281271\":{\"outpoint\":\"630a8e1c3d2d2eb8b317e8269a87a0390a7d6dd4ada3b71da859207ccaae14b1:1110281271\",\"txout\":{\"value\":15591741407262660305,\"script_pubkey\":\"08ce6d76826e5f34e120442a604a0079c9df52ff67a2960707388bf0456bf9baff42b3f43f5744af9af4209e27673841d3ebc161079749d14efc8a165d84a2c1df0c6826305cbde5db7d4b\"},\"keychain\":\"Internal\",\"is_spent\":true}},\"raw_txs\":{\"f953c0395ab3dafaeaf276591a163a31189e901a2febac896aa22b469accbffd\":{\"version\":1,\"lock_time\":749390219,\"input\":[{\"previous_output\":\"098d2e099c903f57a3ec3470677684849086a15c91b9b3ff629aa78d9200be96:2102777305\",\"script_sig\":\"086b407c6efe2faa0906ddb4fbb17568210219f208d2f62f5a8a8bbbb9bc1f08766bec126196e95867e174c7ba6070c0891008f67934282fa8bbd801560756d7722c4b5b14032d19e7\",\"sequence\":3781032586,\"witness\":[\"973459ab835d62\",\"678e410862\"]}],\"output\":[{\"value\":15449077679960011960,\"script_pubkey\":\"204e3cbe79accb76a477f54fd0db3d6c7b50cba3fc4f5d37978144418a114ef4fa1120795bb47c01e56b8201a2218e61a4b0ac8ee70f090153e4ef5257a87ea76b4fbc08cda18f5d069e06965ebd\"}]}},\"tx_metas\":{\"363005278de3fca6d992810833ef412b23ca35841aa5db29003ed9629b4f4292\":{\"txid\":\"363005278de3fca6d992810833ef412b23ca35841aa5db29003ed9629b4f4292\",\"received\":4152075928798363952,\"sent\":3817630852809344414,\"fee\":null,\"confirmation_time\":{\"height\":1333097909,\"timestamp\":12654208677788822518}}},\"last_external_index\":206074427,\"last_internal_index\":null,\"sync_time\":{\"block_time\":{\"height\":1247739046,\"timestamp\":6738928675946799964}},\"external_checksum\":null,\"internal_checksum\":null}";

        serde_json::from_str::<WalletDb>(db_json_str)
            .expect("Failed to deserialize old serialized WalletDb");
    }

    /// Run BDK's test suite.
    #[test]
    fn bdk_tests() {
        bdk_test_suite::test_script_pubkey(WalletDb::new());
        bdk_test_suite::test_batch_script_pubkey(WalletDb::new());
        bdk_test_suite::test_iter_script_pubkey(WalletDb::new());
        bdk_test_suite::test_del_script_pubkey(WalletDb::new());
        bdk_test_suite::test_utxo(WalletDb::new());
        bdk_test_suite::test_raw_tx(WalletDb::new());
        bdk_test_suite::test_tx(WalletDb::new());
        bdk_test_suite::test_list_transaction(WalletDb::new());
        bdk_test_suite::test_last_index(WalletDb::new());
        bdk_test_suite::test_sync_time(WalletDb::new());
        bdk_test_suite::test_iter_raw_txs(WalletDb::new());
        bdk_test_suite::test_del_path_from_script_pubkey(WalletDb::new());
        bdk_test_suite::test_iter_script_pubkeys(WalletDb::new());
        bdk_test_suite::test_del_utxo(WalletDb::new());
        bdk_test_suite::test_del_raw_tx(WalletDb::new());
        bdk_test_suite::test_del_tx(WalletDb::new());
        bdk_test_suite::test_del_last_index(WalletDb::new());
        bdk_test_suite::test_check_descriptor_checksum(WalletDb::new());
    }

    /// This test tests the following properties:
    ///
    /// 1) An arbitrary sequence of operations executed on a [`DbBatch`] do not
    ///    affect the underlying [`WalletDb`] until [`commit_batch`] is called.
    /// 2) An arbitrary sequence of operations executed on a [`DbBatch`] (and
    ///    committed at the end) vs the same sequence of operations executed
    ///    directly on a [`WalletDb`] results in the same [`WalletDb`] state.
    ///
    /// [`commit_batch`]: BatchDatabase::commit_batch
    #[test]
    fn wallet_db_batching() {
        let any_op = any::<DbOp>();
        let any_vec_of_ops = proptest::collection::vec(any_op, 0..20);
        proptest!(|(vec_of_ops in any_vec_of_ops)| {
            let empty_db = WalletDb::new();
            let mut batch_db = WalletDb::new();
            let mut batch = batch_db.begin_batch();
            let mut normal_db = WalletDb::new();

            for op in vec_of_ops {
                // Execute the op on both the batch DB and the "normal" DB which
                // does not batch operations
                op.clone().do_op(&mut batch);
                op.do_op_and_assert(&mut normal_db);

                // The database backing the batch should still be empty because
                // the values have not been committed yet.
                prop_assert_eq!(&batch_db, &empty_db);
            }

            // Commit the batch and check that the batch db and normal db have
            // resulted in an identical state.
            batch_db.commit_batch(batch).unwrap();
            prop_assert_eq!(&batch_db, &normal_db);
        })
    }

    /// Reproduces some bugs in BDK's [`MemoryDatabase`].
    ///
    /// These bugs have been reported in [BDK#829]; awaiting clarification.
    ///
    /// [`BDK#829`]: https://github.com/bitcoindevkit/bdk/issues/829
    /// [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase
    #[test]
    fn bdk_bugs() {
        // use bitcoin::hashes::hex::FromHex;

        // First bug
        /*
        {
            let mut db = MemoryDatabase::new();

            let script = Script::from(
                Vec::<u8>::from_hex(
                    "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
                )
                .unwrap(),
            );
            let keychain = KeychainKind::External;
            let child = 0;

            db.set_script_pubkey(&script, keychain, child).unwrap();
            let del_script = db
                .del_script_pubkey_from_path(keychain, child)
                .unwrap()
                .unwrap();
            assert_eq!(del_script, script);

            // We deleted by path, but it still exists when we query by script
            assert!(db.get_path_from_script_pubkey(&script).unwrap().is_none());
        }
        */

        // Second bug
        /*
        {
            let mut db = MemoryDatabase::new();

            let script = Script::from(
                Vec::<u8>::from_hex(
                    "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
                )
                .unwrap(),
            );
            let keychain = KeychainKind::External;
            let child = 0;

            db.set_script_pubkey(&script, keychain, child).unwrap();
            let (del_chain, del_child) =
                db.del_path_from_script_pubkey(&script).unwrap().unwrap();
            assert_eq!(del_chain, keychain);
            assert_eq!(del_child, child);

            // We deleted by script, but it still exists when queried by path
            assert!(db
                .get_script_pubkey_from_path(keychain, child)
                .unwrap()
                .is_none());
        }
        */
    }
}
