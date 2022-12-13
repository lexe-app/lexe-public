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
struct LexeWalletDb {
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

// --- impl LexeWalletDb --- //

impl LexeWalletDb {
    pub(super) fn new() -> Self {
        let path_to_script = BTreeMap::new();
        let script_to_path = BTreeMap::new();
        Self {
            path_to_script,
            script_to_path,
        }
    }

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

impl Database for LexeWalletDb {
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
        let k = Path { keychain, child };
        Ok(self.path_to_script.get(&k).cloned())
    }

    fn get_path_from_script_pubkey(
        &self,
        _: &Script,
    ) -> Result<Option<(KeychainKind, u32)>, bdk::Error> {
        todo!()
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

impl BatchOperations for LexeWalletDb {
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

impl BatchDatabase for LexeWalletDb {
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

// TODO(max): Fuzz / proptest the db
