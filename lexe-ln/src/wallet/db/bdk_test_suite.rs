// Bitcoin Dev Kit
// Written in 2020 by Alekos Filini <alekos.filini@gmail.com>
//
// Copyright (c) 2020-2021 Bitcoin Dev Kit Developers
//
// This file is licensed under the Apache License, Version 2.0 <LICENSE-APACHE
// or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.
// You may not use this file except in accordance with one or both of these
// licenses.

//! This file contains a copy of BDK's wallet database test suite. We run BDK's
//! tests in order to ensure that we uphold the invariants that BDK expects.
//!
//! The following test suite was copied on 2022-12-24 from BDK's 0.25 release:
//! <https://github.com/bitcoindevkit/bdk/blob/d288cbbbbc223355ca3a8f13375b97366ed6edd7/src/database/mod.rs#L216>

use std::str::FromStr;

use bitcoin::consensus::encode::deserialize;
use bitcoin::consensus::serialize;
use bitcoin::hashes::hex::*;
use bitcoin::*;

use super::*;

pub fn test_script_pubkey<D: Database>(mut db: D) {
    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;
    let keychain = KeychainKind::External;

    db.set_script_pubkey(&script, keychain, path).unwrap();

    assert_eq!(
        db.get_script_pubkey_from_path(keychain, path).unwrap(),
        Some(script.clone())
    );
    assert_eq!(
        db.get_path_from_script_pubkey(&script).unwrap(),
        Some((keychain, path))
    );
}

pub fn test_batch_script_pubkey<D: BatchDatabase>(mut db: D) {
    let mut batch = db.begin_batch();

    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;
    let keychain = KeychainKind::External;

    batch.set_script_pubkey(&script, keychain, path).unwrap();

    assert_eq!(
        db.get_script_pubkey_from_path(keychain, path).unwrap(),
        None
    );
    assert_eq!(db.get_path_from_script_pubkey(&script).unwrap(), None);

    db.commit_batch(batch).unwrap();

    assert_eq!(
        db.get_script_pubkey_from_path(keychain, path).unwrap(),
        Some(script.clone())
    );
    assert_eq!(
        db.get_path_from_script_pubkey(&script).unwrap(),
        Some((keychain, path))
    );
}

pub fn test_iter_script_pubkey<D: Database>(mut db: D) {
    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;
    let keychain = KeychainKind::External;

    db.set_script_pubkey(&script, keychain, path).unwrap();

    assert_eq!(db.iter_script_pubkeys(None).unwrap().len(), 1);
}

pub fn test_del_script_pubkey<D: Database>(mut db: D) {
    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;
    let keychain = KeychainKind::External;

    db.set_script_pubkey(&script, keychain, path).unwrap();
    assert_eq!(db.iter_script_pubkeys(None).unwrap().len(), 1);

    db.del_script_pubkey_from_path(keychain, path).unwrap();
    assert_eq!(db.iter_script_pubkeys(None).unwrap().len(), 0);
}

pub fn test_utxo<D: Database>(mut db: D) {
    let outpoint = OutPoint::from_str(
        "5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456:0",
    )
    .unwrap();
    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let txout = TxOut {
        value: 133742,
        script_pubkey: script,
    };
    let utxo = LocalUtxo {
        txout,
        outpoint,
        keychain: KeychainKind::External,
        is_spent: true,
    };

    db.set_utxo(&utxo).unwrap();
    db.set_utxo(&utxo).unwrap();
    assert_eq!(db.iter_utxos().unwrap().len(), 1);
    assert_eq!(db.get_utxo(&outpoint).unwrap(), Some(utxo));
}

pub fn test_raw_tx<D: Database>(mut db: D) {
    let hex_tx = Vec::<u8>::from_hex("02000000000101f58c18a90d7a76b30c7e47d4e817adfdd79a6a589a615ef36e360f913adce2cd0000000000feffffff0210270000000000001600145c9a1816d38db5cbdd4b067b689dc19eb7d930e2cf70aa2b080000001600140f48b63160043047f4f60f7f8f551f80458f693f024730440220413f42b7bc979945489a38f5221e5527d4b8e3aa63eae2099e01945896ad6c10022024ceec492d685c31d8adb64e935a06933877c5ae0e21f32efe029850914c5bad012102361caae96f0e9f3a453d354bb37a5c3244422fb22819bf0166c0647a38de39f21fca2300").unwrap();
    let mut tx: Transaction = deserialize(&hex_tx).unwrap();

    db.set_raw_tx(&tx).unwrap();

    let txid = tx.txid();

    assert_eq!(db.get_raw_tx(&txid).unwrap(), Some(tx.clone()));

    // mutate transaction's witnesses
    for tx_in in tx.input.iter_mut() {
        tx_in.witness = Witness::new();
    }

    let updated_hex_tx = serialize(&tx);

    // verify that mutation was successful
    assert_ne!(hex_tx, updated_hex_tx);

    db.set_raw_tx(&tx).unwrap();

    let txid = tx.txid();

    assert_eq!(db.get_raw_tx(&txid).unwrap(), Some(tx));
}

pub fn test_tx<D: Database>(mut db: D) {
    let hex_tx = Vec::<u8>::from_hex("0100000001a15d57094aa7a21a28cb20b59aab8fc7d1149a3bdbcddba9c622e4f5f6a99ece010000006c493046022100f93bb0e7d8db7bd46e40132d1f8242026e045f03a0efe71bbb8e3f475e970d790221009337cd7f1f929f00cc6ff01f03729b069a7c21b59b1736ddfee5db5946c5da8c0121033b9b137ee87d5a812d6f506efdd37f0affa7ffc310711c06c7f3e097c9447c52ffffffff0100e1f505000000001976a9140389035a9225b3839e2bbf32d826a1e222031fd888ac00000000").unwrap();
    let tx: Transaction = deserialize(&hex_tx).unwrap();
    let txid = tx.txid();
    let mut tx_details = TransactionDetails {
        transaction: Some(tx),
        txid,
        received: 1337,
        sent: 420420,
        fee: Some(140),
        confirmation_time: Some(BlockTime {
            timestamp: 123456,
            height: 1000,
        }),
    };

    db.set_tx(&tx_details).unwrap();

    // get with raw tx too
    assert_eq!(
        db.get_tx(&tx_details.txid, true).unwrap(),
        Some(tx_details.clone())
    );
    // get only raw_tx
    assert_eq!(
        db.get_raw_tx(&tx_details.txid).unwrap(),
        tx_details.transaction
    );

    // now get without raw_tx
    tx_details.transaction = None;
    assert_eq!(
        db.get_tx(&tx_details.txid, false).unwrap(),
        Some(tx_details)
    );
}

pub fn test_list_transaction<D: Database>(mut db: D) {
    let hex_tx = Vec::<u8>::from_hex("0100000001a15d57094aa7a21a28cb20b59aab8fc7d1149a3bdbcddba9c622e4f5f6a99ece010000006c493046022100f93bb0e7d8db7bd46e40132d1f8242026e045f03a0efe71bbb8e3f475e970d790221009337cd7f1f929f00cc6ff01f03729b069a7c21b59b1736ddfee5db5946c5da8c0121033b9b137ee87d5a812d6f506efdd37f0affa7ffc310711c06c7f3e097c9447c52ffffffff0100e1f505000000001976a9140389035a9225b3839e2bbf32d826a1e222031fd888ac00000000").unwrap();
    let tx: Transaction = deserialize(&hex_tx).unwrap();
    let txid = tx.txid();
    let mut tx_details = TransactionDetails {
        transaction: Some(tx),
        txid,
        received: 1337,
        sent: 420420,
        fee: Some(140),
        confirmation_time: Some(BlockTime {
            timestamp: 123456,
            height: 1000,
        }),
    };

    db.set_tx(&tx_details).unwrap();

    // get raw tx
    assert_eq!(db.iter_txs(true).unwrap(), vec![tx_details.clone()]);

    // now get without raw tx
    tx_details.transaction = None;

    // get not raw tx
    assert_eq!(db.iter_txs(false).unwrap(), vec![tx_details.clone()]);
}

pub fn test_last_index<D: Database>(mut db: D) {
    db.set_last_index(KeychainKind::External, 1337).unwrap();

    assert_eq!(
        db.get_last_index(KeychainKind::External).unwrap(),
        Some(1337)
    );
    assert_eq!(db.get_last_index(KeychainKind::Internal).unwrap(), None);

    let res = db.increment_last_index(KeychainKind::External).unwrap();
    assert_eq!(res, 1338);
    let res = db.increment_last_index(KeychainKind::Internal).unwrap();
    assert_eq!(res, 0);

    assert_eq!(
        db.get_last_index(KeychainKind::External).unwrap(),
        Some(1338)
    );
    assert_eq!(db.get_last_index(KeychainKind::Internal).unwrap(), Some(0));
}

pub fn test_sync_time<D: Database>(mut db: D) {
    assert!(db.get_sync_time().unwrap().is_none());

    db.set_sync_time(SyncTime {
        block_time: BlockTime {
            height: 100,
            timestamp: 1000,
        },
    })
    .unwrap();

    let extracted = db.get_sync_time().unwrap();
    assert!(extracted.is_some());
    assert_eq!(extracted.as_ref().unwrap().block_time.height, 100);
    assert_eq!(extracted.as_ref().unwrap().block_time.timestamp, 1000);

    db.del_sync_time().unwrap();
    assert!(db.get_sync_time().unwrap().is_none());
}

pub fn test_iter_raw_txs<D: Database>(mut db: D) {
    let txs = db.iter_raw_txs().unwrap();
    assert!(txs.is_empty());

    let hex_tx = Vec::<u8>::from_hex("0100000001a15d57094aa7a21a28cb20b59aab8fc7d1149a3bdbcddba9c622e4f5f6a99ece010000006c493046022100f93bb0e7d8db7bd46e40132d1f8242026e045f03a0efe71bbb8e3f475e970d790221009337cd7f1f929f00cc6ff01f03729b069a7c21b59b1736ddfee5db5946c5da8c0121033b9b137ee87d5a812d6f506efdd37f0affa7ffc310711c06c7f3e097c9447c52ffffffff0100e1f505000000001976a9140389035a9225b3839e2bbf32d826a1e222031fd888ac00000000").unwrap();
    let first_tx: Transaction = deserialize(&hex_tx).unwrap();

    let hex_tx = Vec::<u8>::from_hex("02000000000101f58c18a90d7a76b30c7e47d4e817adfdd79a6a589a615ef36e360f913adce2cd0000000000feffffff0210270000000000001600145c9a1816d38db5cbdd4b067b689dc19eb7d930e2cf70aa2b080000001600140f48b63160043047f4f60f7f8f551f80458f693f024730440220413f42b7bc979945489a38f5221e5527d4b8e3aa63eae2099e01945896ad6c10022024ceec492d685c31d8adb64e935a06933877c5ae0e21f32efe029850914c5bad012102361caae96f0e9f3a453d354bb37a5c3244422fb22819bf0166c0647a38de39f21fca2300").unwrap();
    let second_tx: Transaction = deserialize(&hex_tx).unwrap();

    db.set_raw_tx(&first_tx).unwrap();
    db.set_raw_tx(&second_tx).unwrap();

    let txs = db.iter_raw_txs().unwrap();

    assert!(txs.contains(&first_tx));
    assert!(txs.contains(&second_tx));
    assert_eq!(txs.len(), 2);
}

pub fn test_del_path_from_script_pubkey<D: Database>(mut db: D) {
    let keychain = KeychainKind::External;

    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;

    let res = db.del_path_from_script_pubkey(&script).unwrap();

    assert!(res.is_none());

    let _res = db.set_script_pubkey(&script, keychain, path);
    let (chain, child) =
        db.del_path_from_script_pubkey(&script).unwrap().unwrap();

    assert_eq!(chain, keychain);
    assert_eq!(child, path);

    let res = db.get_path_from_script_pubkey(&script).unwrap();
    assert!(res.is_none());
}

pub fn test_iter_script_pubkeys<D: Database>(mut db: D) {
    let keychain = KeychainKind::External;
    let scripts = db.iter_script_pubkeys(Some(keychain)).unwrap();
    assert!(scripts.is_empty());

    let first_script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let path = 42;

    db.set_script_pubkey(&first_script, keychain, path).unwrap();

    let second_script = Script::from(
        Vec::<u8>::from_hex("00145c9a1816d38db5cbdd4b067b689dc19eb7d930e2")
            .unwrap(),
    );
    let path = 57;

    db.set_script_pubkey(&second_script, keychain, path)
        .unwrap();
    let scripts = db.iter_script_pubkeys(Some(keychain)).unwrap();

    assert!(scripts.contains(&first_script));
    assert!(scripts.contains(&second_script));
    assert_eq!(scripts.len(), 2);
}

pub fn test_del_utxo<D: Database>(mut db: D) {
    let outpoint = OutPoint::from_str(
        "5df6e0e2761359d30a8275058e299fcc0381534545f55cf43e41983f5d4c9456:0",
    )
    .unwrap();
    let script = Script::from(
        Vec::<u8>::from_hex(
            "76a91402306a7c23f3e8010de41e9e591348bb83f11daa88ac",
        )
        .unwrap(),
    );
    let txout = TxOut {
        value: 133742,
        script_pubkey: script,
    };
    let utxo = LocalUtxo {
        txout,
        outpoint,
        keychain: KeychainKind::External,
        is_spent: true,
    };

    let res = db.del_utxo(&outpoint).unwrap();
    assert!(res.is_none());

    db.set_utxo(&utxo).unwrap();

    let res = db.del_utxo(&outpoint).unwrap();

    assert_eq!(res.unwrap(), utxo);

    let res = db.get_utxo(&outpoint).unwrap();
    assert!(res.is_none());
}

pub fn test_del_raw_tx<D: Database>(mut db: D) {
    let hex_tx = Vec::<u8>::from_hex("02000000000101f58c18a90d7a76b30c7e47d4e817adfdd79a6a589a615ef36e360f913adce2cd0000000000feffffff0210270000000000001600145c9a1816d38db5cbdd4b067b689dc19eb7d930e2cf70aa2b080000001600140f48b63160043047f4f60f7f8f551f80458f693f024730440220413f42b7bc979945489a38f5221e5527d4b8e3aa63eae2099e01945896ad6c10022024ceec492d685c31d8adb64e935a06933877c5ae0e21f32efe029850914c5bad012102361caae96f0e9f3a453d354bb37a5c3244422fb22819bf0166c0647a38de39f21fca2300").unwrap();
    let tx: Transaction = deserialize(&hex_tx).unwrap();

    let res = db.del_raw_tx(&tx.txid()).unwrap();

    assert!(res.is_none());

    db.set_raw_tx(&tx).unwrap();

    let res = db.del_raw_tx(&tx.txid()).unwrap();

    assert_eq!(res.unwrap(), tx);

    let res = db.get_raw_tx(&tx.txid()).unwrap();
    assert!(res.is_none());
}

pub fn test_del_tx<D: Database>(mut db: D) {
    let hex_tx = Vec::<u8>::from_hex("0100000001a15d57094aa7a21a28cb20b59aab8fc7d1149a3bdbcddba9c622e4f5f6a99ece010000006c493046022100f93bb0e7d8db7bd46e40132d1f8242026e045f03a0efe71bbb8e3f475e970d790221009337cd7f1f929f00cc6ff01f03729b069a7c21b59b1736ddfee5db5946c5da8c0121033b9b137ee87d5a812d6f506efdd37f0affa7ffc310711c06c7f3e097c9447c52ffffffff0100e1f505000000001976a9140389035a9225b3839e2bbf32d826a1e222031fd888ac00000000").unwrap();
    let tx: Transaction = deserialize(&hex_tx).unwrap();
    let txid = tx.txid();
    let mut tx_details = TransactionDetails {
        transaction: Some(tx.clone()),
        txid,
        received: 1337,
        sent: 420420,
        fee: Some(140),
        confirmation_time: Some(BlockTime {
            timestamp: 123456,
            height: 1000,
        }),
    };

    let res = db.del_tx(&tx.txid(), true).unwrap();

    assert!(res.is_none());

    db.set_tx(&tx_details).unwrap();

    let res = db.del_tx(&tx.txid(), false).unwrap();
    tx_details.transaction = None;
    assert_eq!(res.unwrap(), tx_details);

    let res = db.get_tx(&tx.txid(), true).unwrap();
    assert!(res.is_none());

    let res = db.get_raw_tx(&tx.txid()).unwrap();
    assert_eq!(res.unwrap(), tx);

    db.set_tx(&tx_details).unwrap();
    let res = db.del_tx(&tx.txid(), true).unwrap();
    tx_details.transaction = Some(tx.clone());
    assert_eq!(res.unwrap(), tx_details);

    let res = db.get_tx(&tx.txid(), true).unwrap();
    assert!(res.is_none());

    let res = db.get_raw_tx(&tx.txid()).unwrap();
    assert!(res.is_none());
}

pub fn test_del_last_index<D: Database>(mut db: D) {
    let keychain = KeychainKind::External;

    let _res = db.increment_last_index(keychain);

    let res = db.get_last_index(keychain).unwrap().unwrap();

    assert_eq!(res, 0);

    let _res = db.increment_last_index(keychain);

    let res = db.del_last_index(keychain).unwrap().unwrap();

    assert_eq!(res, 1);

    let res = db.get_last_index(keychain).unwrap();
    assert!(res.is_none());
}

pub fn test_check_descriptor_checksum<D: Database>(mut db: D) {
    // insert checksum associated to keychain
    let checksum = "1cead456".as_bytes();
    let keychain = KeychainKind::External;
    let _res = db.check_descriptor_checksum(keychain, checksum);

    // check if `check_descriptor_checksum` throws
    // `Error::ChecksumMismatch` error if the
    // function is passed a checksum that does
    // not match the one initially inserted
    let checksum = "1cead454".as_bytes();
    let keychain = KeychainKind::External;
    let res = db.check_descriptor_checksum(keychain, checksum);

    assert!(res.is_err());
}

// TODO: more tests...
