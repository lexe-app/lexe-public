//! This module contains the [`WalletDb`], which implements [`PersistBackend`]
//! as required by [`bdk::Wallet`].
//!
//! ## [`ChangeSet`]s
//!
//! [`bdk::wallet::ChangeSet`] is the top-level data struct given to us by BDK,
//! and is the main thing that need to be persisted. It implements [`Serialize`]
//! / [`Deserialize`], and [`bdk_chain::Append`], which allows changesets to be
//! aggregated together. The [`ChangeSet`]s may be persisted in aggregated form,
//! or they can be persisted separately and reaggregated when (re-)initializing
//! our [`bdk::Wallet`].
//!
//! ## [`PersistBackend`] implementation
//!
//! The [`PersistBackend`] methods are intended to reflect reading / writing to
//! disk, but they are blocking, which doesn't work with our async persistence
//! paradigm. So instead, when [`PersistBackend::write_changes`] is called, we
//! simply aggregate the new changes into our existing [`ChangeSet`], then
//! notify our wallet db persister task to re-persist the [`WalletDb`]'s inner
//! [`ChangeSet`]. Likewise, [`PersistBackend::load_from_persistence`] simply
//! returns the [`WalletDb`]'s contained [`ChangeSet`]. This breaks the
//! contract, but our usage of BDK isn't security-critical so it's OK.
//!
//! [`WalletDb`]: crate::wallet::db::WalletDb
//! [`ChangeSet`]: bdk::wallet::ChangeSet
//! [`Serialize`]: serde::Serialize
//! [`Deserialize`]: serde::Deserialize
//! [`PersistBackend`]: bdk_chain::PersistBackend
//! [`PersistBackend::write_changes`]: bdk_chain::PersistBackend::write_changes
//! [`PersistBackend::load_from_persistence`]: bdk_chain::PersistBackend::load_from_persistence

use std::{convert::Infallible, sync::Arc};

pub use bdk::wallet::ChangeSet;
use bdk_chain::{Append, PersistBackend};
use common::notify;

/// See module docs.
#[derive(Clone)]
pub struct WalletDb {
    /// NOTE: This is the full, *aggregated* changeset, not an intermediate
    /// state diff, contrary to what the name of "[`ChangeSet`]" might suggest.
    changeset: Arc<std::sync::Mutex<ChangeSet>>,
    wallet_db_persister_tx: notify::Sender,
}

impl WalletDb {
    /// Initialize a new, empty [`WalletDb`].
    pub fn empty(wallet_db_persister_tx: notify::Sender) -> Self {
        Self {
            changeset: Arc::new(std::sync::Mutex::new(Default::default())),
            wallet_db_persister_tx,
        }
    }

    /// Initialize a [`WalletDb`] from an existing [`ChangeSet`].
    pub fn from_changeset(
        changeset: ChangeSet,
        wallet_db_persister_tx: notify::Sender,
    ) -> Self {
        Self {
            changeset: Arc::new(std::sync::Mutex::new(changeset)),
            wallet_db_persister_tx,
        }
    }

    /// Get a clone of the contained [`ChangeSet`].
    pub fn changeset(&self) -> ChangeSet {
        self.changeset.lock().unwrap().clone()
    }
}

// This is the exact bound required by `bdk::Wallet` methods
impl PersistBackend<ChangeSet> for WalletDb {
    // Required by `bdk::Wallet` methods
    type WriteError = Infallible;

    type LoadError = Infallible;

    /// We're supposed to write the new changes here, but this method is
    /// blocking, so we just append the data and notify the DB persister.
    fn write_changes(
        &mut self,
        changeset: &ChangeSet,
    ) -> Result<(), Self::WriteError> {
        let mut locked_changeset = self.changeset.lock().unwrap();
        locked_changeset.append(changeset.clone());
        self.wallet_db_persister_tx.send();
        Ok(())
    }

    /// We're supposed to read from disk here, but this method is blocking, so
    /// instead we just return the already-loaded value (if non-empty).
    fn load_from_persistence(
        &mut self,
    ) -> Result<Option<ChangeSet>, Self::LoadError> {
        let locked_changeset = self.changeset.lock().unwrap();
        if locked_changeset.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locked_changeset.clone()))
        }
    }
}

#[cfg(test)]
mod arbitrary_impl {
    use bdk::{wallet::ChangeSet, KeychainKind};
    use bdk_chain::{
        indexed_tx_graph, keychain, local_chain, tx_graph,
        ConfirmationTimeHeightAnchor,
    };
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::any,
        prop_oneof,
        strategy::{Just, Strategy},
    };

    type KeychainChangeset = keychain::ChangeSet<KeychainKind>;
    type TxGraphChangeset = tx_graph::ChangeSet<ConfirmationTimeHeightAnchor>;
    type IndexedTxGraphChangeset = indexed_tx_graph::ChangeSet<
        ConfirmationTimeHeightAnchor,
        KeychainChangeset,
    >;

    pub(super) fn any_changeset() -> impl Strategy<Value = ChangeSet> {
        (
            any_localchain_changeset(),
            any_indexedtxgraph_changeset(),
            proptest::option::of(arbitrary::any_network()),
        )
            .prop_map(|(chain, indexed_tx_graph, network)| ChangeSet {
                chain,
                indexed_tx_graph,
                network,
            })
    }

    fn any_indexedtxgraph_changeset(
    ) -> impl Strategy<Value = IndexedTxGraphChangeset> {
        (any_txgraph_changeset(), any_keychain_changeset()).prop_map(
            |(graph, indexer)| IndexedTxGraphChangeset { graph, indexer },
        )
    }

    fn any_txgraph_changeset() -> impl Strategy<Value = TxGraphChangeset> {
        // BTreeSet<Transaction>
        let any_txs =
            proptest::collection::btree_set(arbitrary::any_raw_tx(), 0..4);
        // BTreeMap<OutPoint, TxOut>
        let any_txouts = proptest::collection::btree_map(
            arbitrary::any_outpoint(),
            arbitrary::any_txout(),
            0..4,
        );
        // BTreeSet<(ConfirmationTimeHeightAnchor, Txid)>
        let anchors = proptest::collection::btree_set(
            (any_confirmationtimeheightanchor(), arbitrary::any_txid()),
            0..4,
        );
        // BTreeMap<Txid, u64>,
        let last_seen = proptest::collection::btree_map(
            arbitrary::any_txid(),
            any::<u64>(),
            0..4,
        );

        (any_txs, any_txouts, anchors, last_seen).prop_map(
            |(txs, txouts, anchors, last_seen)| TxGraphChangeset {
                txs,
                txouts,
                anchors,
                last_seen,
            },
        )
    }

    // This is just `BTreeMap<KeychainKind, u32>`
    fn any_keychain_changeset() -> impl Strategy<Value = KeychainChangeset> {
        proptest::collection::btree_map(
            any_keychain_kind(),
            any::<u32>(),
            0..16,
        )
        .prop_map(keychain::ChangeSet)
    }

    // This is just `BTreeMap<u32, Option<BlockHash>>`
    fn any_localchain_changeset(
    ) -> impl Strategy<Value = local_chain::ChangeSet> {
        proptest::collection::btree_map(
            any::<u32>(),
            proptest::option::of(arbitrary::any_blockhash()),
            0..16,
        )
    }

    fn any_confirmationtimeheightanchor(
    ) -> impl Strategy<Value = ConfirmationTimeHeightAnchor> {
        (any::<u32>(), any::<u64>(), any_blockid()).prop_map(
            |(confirmation_height, confirmation_time, anchor_block)| {
                ConfirmationTimeHeightAnchor {
                    confirmation_height,
                    confirmation_time,
                    anchor_block,
                }
            },
        )
    }

    fn any_blockid() -> impl Strategy<Value = bdk_chain::BlockId> {
        (any::<u32>(), arbitrary::any_blockhash())
            .prop_map(|(height, hash)| bdk_chain::BlockId { height, hash })
    }

    fn any_keychain_kind() -> impl Strategy<Value = KeychainKind> {
        prop_oneof![Just(KeychainKind::External), Just(KeychainKind::Internal)]
    }
}

#[cfg(test)]
mod test {
    use common::{
        rng::WeakRng,
        test_utils::{arbitrary, roundtrip},
    };
    use proptest::test_runner::Config;

    use super::*;

    // Snapshot taken 2024-10-30
    const CHANGESET_SNAPSHOT: &str =
        include_str!("../../data/changeset-snapshot.json");

    #[test]
    fn default_changeset_is_empty() {
        assert!(ChangeSet::default().is_empty());
    }

    #[test]
    fn changeset_roundtrip_proptest() {
        roundtrip::json_value_custom(
            arbitrary_impl::any_changeset(),
            Config::default(),
        );
    }

    #[test]
    fn test_changeset_snapshot() {
        serde_json::from_str::<Vec<ChangeSet>>(CHANGESET_SNAPSHOT).unwrap();
    }

    /// Dumps a JSON array of three `ChangeSet`s using the proptest strategy.
    ///
    /// ```bash
    /// $ cargo test -p lexe-ln -- --ignored dump_changesets --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_changesets() {
        let mut rng = WeakRng::from_u64(20241030);
        let strategy = arbitrary_impl::any_changeset();
        let changesets = arbitrary::gen_value_iter(&mut rng, strategy)
            .take(3)
            .collect::<Vec<ChangeSet>>();
        println!("---");
        println!("{}", serde_json::to_string_pretty(&changesets).unwrap());
        println!("---");
    }
}
