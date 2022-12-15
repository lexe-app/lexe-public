//! Lexe's checked copy of BDK's [`MemoryDatabase`], modified to support
//! serialization of the entire DB to be persisted.
//!
//! [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase

use std::cmp::{Ord, Ordering, PartialOrd};
use std::collections::BTreeMap;
use std::mem;

use bdk::database::{BatchDatabase, BatchOperations, Database, SyncTime};
use bdk::{BlockTime, KeychainKind, LocalUtxo, TransactionDetails};
use bitcoin::{OutPoint, Script, Transaction, Txid};

/// Implements the DB traits required by BDK. Similar to [`MemoryDatabase`], but
/// adds the ability to serialize the entire DB for persisting.
///
/// [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase
#[allow(dead_code)] // TODO(max): Remove
struct WalletDb {
    path_to_script: BTreeMap<Path, Script>,
    script_to_path: BTreeMap<Script, Path>,
    utxos: BTreeMap<OutPoint, LocalUtxo>,
    raw_txs: BTreeMap<Txid, Transaction>,
    tx_metas: BTreeMap<Txid, TransactionMetadata>,
    last_external_index: Option<u32>,
    last_internal_index: Option<u32>,
    sync_time: Option<SyncTime>,
}

/// Represents a [`KeychainKind`] and corresponding child path.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct Path {
    keychain: KeychainKind,
    child: u32,
}

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
///
/// [`get_tx`]: Database::get_tx
/// [`set_tx`]: BatchOperations::set_tx
/// [`del_tx`]: BatchOperations::del_tx
/// [`get_raw_tx`]: Database::get_raw_tx
/// [`set_raw_tx`]: BatchOperations::set_raw_tx
/// [`del_raw_tx`]: BatchOperations::del_raw_tx
/// [`transaction`]: TransactionDetails::transaction
#[derive(Clone)]
struct TransactionMetadata {
    pub txid: Txid,
    pub received: u64,
    pub sent: u64,
    pub fee: Option<u64>,
    pub confirmation_time: Option<BlockTime>,
}

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

// --- impl WalletDb --- //

impl WalletDb {
    #[allow(dead_code)] // TODO(max): Remove
    pub(super) fn new() -> Self {
        let path_to_script = BTreeMap::new();
        let script_to_path = BTreeMap::new();
        let utxos = BTreeMap::new();
        let raw_txs = BTreeMap::new();
        let tx_metas = BTreeMap::new();
        let last_external_index = None;
        let last_internal_index = None;
        let sync_time = None;

        Self {
            path_to_script,
            script_to_path,
            utxos,
            raw_txs,
            tx_metas,
            last_external_index,
            last_internal_index,
            sync_time,
        }
    }

    #[cfg(test)]
    fn assert_invariants(&self) {
        // Everything in path_to_script must be in script_to_path and vice versa
        for (path1, script1) in self.path_to_script.iter() {
            let path2 = self.script_to_path.get(script1).unwrap();
            assert_eq!(path1, path2);
        }
        for (script2, path2) in self.script_to_path.iter() {
            let script1 = self.path_to_script.get(path2).unwrap();
            assert_eq!(script1, script2);
        }
    }
}

impl Database for WalletDb {
    fn check_descriptor_checksum<B: AsRef<[u8]>>(
        &mut self,
        _: KeychainKind,
        _: B,
    ) -> Result<(), bdk::Error> {
        todo!()
    }

    fn iter_script_pubkeys(
        &self,
        maybe_filter_keychain: Option<KeychainKind>,
    ) -> Result<Vec<Script>, bdk::Error> {
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

    fn iter_utxos(&self) -> Result<Vec<LocalUtxo>, bdk::Error> {
        Ok(self.utxos.values().cloned().collect())
    }

    fn iter_raw_txs(&self) -> Result<Vec<Transaction>, bdk::Error> {
        Ok(self.raw_txs.values().cloned().collect())
    }

    fn iter_txs(
        &self,
        include_raw: bool,
    ) -> Result<Vec<TransactionDetails>, bdk::Error> {
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
    ) -> Result<Option<Script>, bdk::Error> {
        let path = Path { keychain, child };
        Ok(self.path_to_script.get(&path).cloned())
    }

    fn get_path_from_script_pubkey(
        &self,
        script: &Script,
    ) -> Result<Option<(KeychainKind, u32)>, bdk::Error> {
        self.script_to_path
            .get(script)
            .map(|path| (path.keychain, path.child))
            .map(Ok)
            .transpose()
    }

    fn get_utxo(
        &self,
        outpoint: &OutPoint,
    ) -> Result<Option<LocalUtxo>, bdk::Error> {
        Ok(self.utxos.get(outpoint).cloned())
    }

    fn get_raw_tx(
        &self,
        txid: &Txid,
    ) -> Result<Option<Transaction>, bdk::Error> {
        Ok(self.raw_txs.get(txid).cloned())
    }

    fn get_tx(
        &self,
        txid: &Txid,
        include_raw: bool,
    ) -> Result<Option<TransactionDetails>, bdk::Error> {
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

    fn get_last_index(
        &self,
        keychain: KeychainKind,
    ) -> Result<Option<u32>, bdk::Error> {
        match keychain {
            KeychainKind::External => Ok(self.last_external_index),
            KeychainKind::Internal => Ok(self.last_internal_index),
        }
    }

    fn get_sync_time(&self) -> Result<Option<SyncTime>, bdk::Error> {
        Ok(self.sync_time.clone())
    }

    fn increment_last_index(
        &mut self,
        keychain: KeychainKind,
    ) -> Result<u32, bdk::Error> {
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

impl BatchOperations for WalletDb {
    // Weird that the set_* methods take ref, but ok
    fn set_script_pubkey(
        &mut self,
        script: &Script,
        keychain: KeychainKind,
        child: u32,
    ) -> Result<(), bdk::Error> {
        let path = Path { keychain, child };
        let script = script.clone();
        self.path_to_script.insert(path.clone(), script.clone());
        self.script_to_path.insert(script, path);
        Ok(())
    }

    fn set_utxo(&mut self, utxo: &LocalUtxo) -> Result<(), bdk::Error> {
        self.utxos.insert(utxo.outpoint, utxo.clone());
        Ok(())
    }

    fn set_raw_tx(&mut self, raw_tx: &Transaction) -> Result<(), bdk::Error> {
        self.raw_txs.insert(raw_tx.txid(), raw_tx.clone());
        Ok(())
    }

    fn set_tx(&mut self, tx: &TransactionDetails) -> Result<(), bdk::Error> {
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
    ) -> Result<(), bdk::Error> {
        match keychain {
            KeychainKind::External => self.last_external_index.insert(index),
            KeychainKind::Internal => self.last_internal_index.insert(index),
        };
        Ok(())
    }

    fn set_sync_time(&mut self, time: SyncTime) -> Result<(), bdk::Error> {
        self.sync_time = Some(time);
        Ok(())
    }

    fn del_script_pubkey_from_path(
        &mut self,
        keychain: KeychainKind,
        child: u32,
    ) -> Result<Option<Script>, bdk::Error> {
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
    ) -> Result<Option<(KeychainKind, u32)>, bdk::Error> {
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
    ) -> Result<Option<LocalUtxo>, bdk::Error> {
        Ok(self.utxos.remove(outpoint))
    }

    fn del_raw_tx(
        &mut self,
        txid: &Txid,
    ) -> Result<Option<Transaction>, bdk::Error> {
        Ok(self.raw_txs.remove(txid))
    }

    fn del_tx(
        &mut self,
        txid: &Txid,
        include_raw: bool,
    ) -> Result<Option<TransactionDetails>, bdk::Error> {
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
    ) -> Result<Option<u32>, bdk::Error> {
        match keychain {
            KeychainKind::External => self.last_external_index.take(),
            KeychainKind::Internal => self.last_internal_index.take(),
        }
        .map(Ok)
        .transpose()
    }

    fn del_sync_time(&mut self) -> Result<Option<SyncTime>, bdk::Error> {
        Ok(self.sync_time.take())
    }
}

impl BatchDatabase for WalletDb {
    type Batch = Self;

    fn begin_batch(&self) -> <Self as BatchDatabase>::Batch {
        todo!()
    }

    fn commit_batch(
        &mut self,
        _: <Self as BatchDatabase>::Batch,
    ) -> Result<(), bdk::Error> {
        todo!()
    }
}

#[cfg(test)]
mod test {
    use bitcoin::{PackedLockTime, TxOut};
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::proptest;
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;

    #[derive(Debug)]
    enum DbOp {
        SetPathScript(u8),
        DelByPath(u8),
        DelByScript(u8),
        SetUtxo(u8),
        DelUtxo(u8),
        SetRawTx(u8),
        DelRawTx(u8),
        SetTx { i: u8, include_raw: bool },
        DelTx { i: u8, include_raw: bool },
        IncLastIndex(u8),
        SetLastIndex(u8),
        DelLastIndex(u8),
        SetSyncTime(u8),
        DelSyncTime(u8),
    }

    impl DbOp {
        /// Returns the [`u8`] contained within.
        fn index(&self) -> u8 {
            match self {
                Self::SetPathScript(i) => *i,
                Self::DelByPath(i) => *i,
                Self::DelByScript(i) => *i,
                Self::SetUtxo(i) => *i,
                Self::DelUtxo(i) => *i,
                Self::SetRawTx(i) => *i,
                Self::DelRawTx(i) => *i,
                Self::SetTx { i, .. } => *i,
                Self::DelTx { i, .. } => *i,
                Self::IncLastIndex(i) => *i,
                Self::SetLastIndex(i) => *i,
                Self::DelLastIndex(i) => *i,
                Self::SetSyncTime(i) => *i,
                Self::DelSyncTime(i) => *i,
            }
        }

        /// Executes the operation and asserts op-specific invariants.
        fn do_op_and_check_op_invariants(self, db: &mut WalletDb) {
            // Generate some intermediates used throughout. Each i produces a
            // unique and corresponding set of these intermediates.
            let i = self.index();
            let script = Script::from(vec![i]);
            let keychain = if i % 2 == 0 {
                KeychainKind::External
            } else {
                KeychainKind::Internal
            };
            let child = u32::from(i);
            let raw_tx = Transaction {
                version: 1,
                lock_time: PackedLockTime(u32::from(i)),
                input: Vec::new(),
                output: Vec::new(),
            };
            let txid = raw_tx.txid();
            let outpoint = OutPoint {
                txid,
                vout: u32::from(i),
            };
            let utxo = LocalUtxo {
                outpoint,
                txout: TxOut {
                    value: u64::from(i),
                    script_pubkey: script.clone(),
                },
                keychain,
                is_spent: i % 2 == 0,
            };
            let meta = TransactionMetadata {
                txid,
                received: u64::from(i),
                sent: u64::from(i),
                fee: None,
                confirmation_time: None,
            };

            match self {
                DbOp::SetPathScript(_) => {
                    db.set_script_pubkey(&script, keychain, child).unwrap();

                    let get_script = db
                        .get_script_pubkey_from_path(keychain, child)
                        .unwrap()
                        .unwrap();
                    let (get_keychain, get_child) = db
                        .get_path_from_script_pubkey(&script)
                        .unwrap()
                        .unwrap();
                    assert_eq!(get_script, script);
                    assert_eq!(get_keychain, keychain);
                    assert_eq!(get_child, child);
                }
                DbOp::DelByPath(_) => {
                    db.del_script_pubkey_from_path(keychain, child).unwrap();

                    assert!(db
                        .get_script_pubkey_from_path(keychain, child)
                        .unwrap()
                        .is_none());
                    assert!(db
                        .get_path_from_script_pubkey(&script)
                        .unwrap()
                        .is_none());
                }
                DbOp::DelByScript(_) => {
                    db.del_path_from_script_pubkey(&script).unwrap();

                    assert!(db
                        .get_script_pubkey_from_path(keychain, child)
                        .unwrap()
                        .is_none());
                    assert!(db
                        .get_path_from_script_pubkey(&script)
                        .unwrap()
                        .is_none());
                }
                DbOp::SetUtxo(_) => {
                    db.set_utxo(&utxo).unwrap();
                    let get_utxo = db.get_utxo(&outpoint).unwrap().unwrap();
                    assert_eq!(get_utxo, utxo);
                }
                DbOp::DelUtxo(_) => {
                    db.del_utxo(&outpoint).unwrap();
                    assert!(db.get_utxo(&outpoint).unwrap().is_none());
                }
                DbOp::SetRawTx(_) => {
                    db.set_raw_tx(&raw_tx).unwrap();
                    let get_raw_tx = db.get_raw_tx(&txid).unwrap().unwrap();
                    assert_eq!(get_raw_tx, raw_tx);
                }
                DbOp::DelRawTx(_) => {
                    db.del_raw_tx(&txid).unwrap();
                    assert!(db.get_raw_tx(&txid).unwrap().is_none());
                }
                DbOp::SetTx { include_raw, .. } => {
                    // Include a raw tx if include_raw is true
                    let maybe_raw_tx = if include_raw {
                        Some(raw_tx.clone())
                    } else {
                        None
                    };
                    let tx = meta.into_tx(maybe_raw_tx);

                    db.set_tx(&tx).unwrap();

                    // Tx should exist
                    let get_tx =
                        db.get_tx(&txid, include_raw).unwrap().unwrap();
                    assert_eq!(get_tx, tx);

                    // If include_raw was true, it should be in the raw tx map
                    // too
                    if include_raw {
                        let get_raw_tx = db.get_raw_tx(&txid).unwrap().unwrap();
                        assert_eq!(get_raw_tx, raw_tx);
                    }
                }
                DbOp::DelTx { include_raw, .. } => {
                    db.del_tx(&txid, include_raw).unwrap();

                    // tx should NOT exist
                    assert!(db.get_tx(&txid, include_raw).unwrap().is_none());

                    // If include_raw was true, the raw tx should be deleted too
                    if include_raw {
                        assert!(db.get_raw_tx(&txid).unwrap().is_none());
                    }
                }
                DbOp::IncLastIndex(_) => {
                    let maybe_before = db.get_last_index(keychain).unwrap();
                    let incremented =
                        db.increment_last_index(keychain).unwrap();
                    let get_after =
                        db.get_last_index(keychain).unwrap().unwrap();
                    match maybe_before {
                        Some(get_before) => {
                            assert_eq!(get_before + 1, incremented);
                            assert_eq!(get_before + 1, get_after);
                        }
                        None => {
                            assert_eq!(incremented, 0);
                            assert_eq!(get_after, 0);
                        }
                    }
                }
                DbOp::SetLastIndex(_) => {
                    let index = u32::from(i);
                    db.set_last_index(keychain, index).unwrap();
                    let after = db.get_last_index(keychain).unwrap().unwrap();
                    assert_eq!(after, index);
                }
                DbOp::DelLastIndex(_) => {
                    db.del_last_index(keychain).unwrap();
                    assert!(db.get_last_index(keychain).unwrap().is_none());
                }
                DbOp::SetSyncTime(_) => {
                    let time = SyncTime {
                        block_time: BlockTime {
                            height: u32::from(i),
                            timestamp: u64::from(i),
                        },
                    };
                    db.set_sync_time(time.clone()).unwrap();
                    let get_time = db.get_sync_time().unwrap().unwrap();
                    // SyncTime doesn't derive PartialEq for some reason
                    assert_eq!(get_time.block_time, time.block_time);
                }
                DbOp::DelSyncTime(_) => {
                    db.del_sync_time().unwrap();
                    assert!(db.get_sync_time().unwrap().is_none());
                }
            }
        }
    }

    impl Arbitrary for DbOp {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            use DbOp::*;
            match SetPathScript(0) {
                SetPathScript(_)
                | DelByPath(_)
                | DelByScript(_)
                | SetUtxo(_)
                | DelUtxo(_)
                | SetRawTx(_)
                | DelRawTx(_)
                | SetTx { .. }
                | DelTx { .. }
                | IncLastIndex(_)
                | SetLastIndex(_)
                | DelLastIndex(_)
                | SetSyncTime(_)
                | DelSyncTime(_) => {
                    "This match statement was written to remind you to add the \
                    new enum variant you just created to the prop_oneof below!"
                }
            };
            proptest::prop_oneof![
                any::<u8>().prop_map(Self::SetPathScript),
                any::<u8>().prop_map(Self::DelByPath),
                any::<u8>().prop_map(Self::DelByScript),
                any::<u8>().prop_map(Self::SetUtxo),
                any::<u8>().prop_map(Self::DelUtxo),
                any::<u8>().prop_map(Self::SetRawTx),
                any::<u8>().prop_map(Self::DelRawTx),
                (any::<u8>(), any::<bool>()).prop_map(|(i, include_raw)| {
                    Self::SetTx { i, include_raw }
                }),
                (any::<u8>(), any::<bool>()).prop_map(|(i, include_raw)| {
                    Self::DelTx { i, include_raw }
                }),
                any::<u8>().prop_map(Self::IncLastIndex),
                any::<u8>().prop_map(Self::SetLastIndex),
                any::<u8>().prop_map(Self::DelLastIndex),
                any::<u8>().prop_map(Self::SetSyncTime),
                any::<u8>().prop_map(Self::DelSyncTime),
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
    fn fuzz_wallet_db() {
        let any_op = any::<DbOp>();
        let any_vec_of_ops = proptest::collection::vec(any_op, 0..100);
        proptest!(|(vec_of_ops in any_vec_of_ops)| {
            let mut db = WalletDb::new();

            db.assert_invariants();

            for op in vec_of_ops {
                op.do_op_and_check_op_invariants(&mut db);

                db.assert_invariants();
            }
        })
    }

    // TODO(max): Equivalence test with MemoryDatabase

    // TODO(max): Write snapshot test for serialized WalletDb in case one of the
    // value fields changed. Perhaps use a snapshot crate?
}

// TODO(max): Copy over BDK tests. Should be using latest released version, and
// have a permalink to the source on GitHub.
