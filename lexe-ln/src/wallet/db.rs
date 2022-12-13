//! Lexe's checked copy of BDK's [`MemoryDatabase`], modified to support
//! serialization of the entire DB to be persisted.
//!
//! [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase

#![allow(dead_code)] // TODO(max): Remove

use std::cmp::{Ord, Ordering, PartialOrd};
use std::collections::BTreeMap;
use std::mem;

use bdk::database::{BatchDatabase, BatchOperations, Database, SyncTime};
use bdk::{KeychainKind, LocalUtxo, TransactionDetails};
use bitcoin::{OutPoint, Script, Transaction, Txid};

/// Implements the DB traits required by BDK. similar to [`MemoryDatabase`], but
/// adds the ability to serialize the entire DB for persisting.
///
/// [`MemoryDatabase`]: bdk::database::memory::MemoryDatabase
struct WalletDb {
    path_to_script: BTreeMap<Path, Script>,
    script_to_path: BTreeMap<Script, Path>,
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

// --- impl WalletDb --- //

impl WalletDb {
    pub(super) fn new() -> Self {
        let path_to_script = BTreeMap::new();
        let script_to_path = BTreeMap::new();
        Self {
            path_to_script,
            script_to_path,
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
        todo!()
    }

    fn iter_raw_txs(&self) -> Result<Vec<Transaction>, bdk::Error> {
        todo!()
    }

    fn iter_txs(&self, _: bool) -> Result<Vec<TransactionDetails>, bdk::Error> {
        todo!()
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

    fn get_utxo(&self, _: &OutPoint) -> Result<Option<LocalUtxo>, bdk::Error> {
        todo!()
    }

    fn get_raw_tx(&self, _: &Txid) -> Result<Option<Transaction>, bdk::Error> {
        todo!()
    }

    fn get_tx(
        &self,
        _: &Txid,
        _: bool,
    ) -> Result<Option<TransactionDetails>, bdk::Error> {
        todo!()
    }

    fn get_last_index(
        &self,
        _: KeychainKind,
    ) -> Result<Option<u32>, bdk::Error> {
        todo!()
    }

    fn get_sync_time(&self) -> Result<Option<SyncTime>, bdk::Error> {
        todo!()
    }

    fn increment_last_index(
        &mut self,
        _: KeychainKind,
    ) -> Result<u32, bdk::Error> {
        todo!()
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

    fn set_utxo(&mut self, _: &LocalUtxo) -> Result<(), bdk::Error> {
        todo!()
    }

    fn set_raw_tx(&mut self, _: &Transaction) -> Result<(), bdk::Error> {
        todo!()
    }

    fn set_tx(&mut self, _: &TransactionDetails) -> Result<(), bdk::Error> {
        todo!()
    }

    fn set_last_index(
        &mut self,
        _: KeychainKind,
        _: u32,
    ) -> Result<(), bdk::Error> {
        todo!()
    }

    fn set_sync_time(&mut self, _: SyncTime) -> Result<(), bdk::Error> {
        todo!()
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
        _: &OutPoint,
    ) -> Result<Option<LocalUtxo>, bdk::Error> {
        todo!()
    }

    fn del_raw_tx(
        &mut self,
        _: &Txid,
    ) -> Result<Option<Transaction>, bdk::Error> {
        todo!()
    }

    fn del_tx(
        &mut self,
        _: &Txid,
        _: bool,
    ) -> Result<Option<TransactionDetails>, bdk::Error> {
        todo!()
    }

    fn del_last_index(
        &mut self,
        _: KeychainKind,
    ) -> Result<Option<u32>, bdk::Error> {
        todo!()
    }

    fn del_sync_time(&mut self) -> Result<Option<SyncTime>, bdk::Error> {
        todo!()
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
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::proptest;
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;

    #[derive(Debug)]
    enum DbOp {
        SetPathScript(u8),
        DelByPath(u8),
        DelByScript(u8),
    }

    impl DbOp {
        /// Returns the [`u8`] contained within.
        fn index(&self) -> u8 {
            match self {
                Self::SetPathScript(i) => *i,
                Self::DelByPath(i) => *i,
                Self::DelByScript(i) => *i,
            }
        }

        /// Executes the operation and asserts op-related invariants.
        fn do_op_and_check_op_invariants(&self, db: &mut WalletDb) {
            // Generate some intermediates used throughout. Each i produces a
            // unique and corresponding Script, KeychainKind, and u32 (child).
            let i = self.index();
            let script = Script::from(vec![i]);
            let keychain = if i % 2 == 0 {
                KeychainKind::External
            } else {
                KeychainKind::Internal
            };
            let child = u32::from(i);

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
            }
        }
    }

    impl Arbitrary for DbOp {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            use DbOp::*;
            // If you were brought here by a compilation error, make sure to add
            // the new enum variant to the prop_oneof! below.
            match SetPathScript(0) {
                SetPathScript(_) | DelByPath(_) | DelByScript(_) => {}
            }
            proptest::prop_oneof![
                any::<u8>().prop_map(Self::SetPathScript),
                any::<u8>().prop_map(Self::DelByPath),
                any::<u8>().prop_map(Self::DelByScript),
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

    // TODO(max): Write some proptests
}
