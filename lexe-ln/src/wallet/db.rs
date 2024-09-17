//! This module contains the [`WalletDb`], which implements [`PersistBackend`]
//! as required by [`bdk::Wallet`].
//!
//! ## [`ChangeSet`]s
//!
//! [`bdk::wallet::ChangeSet`] is the top-level data struct given to us by BDK,
//! and is the main thing that need to be persisted.h It implements
//! [`Serialize`] / [`Deserialize`], and [`bdk_chain::Append`], which allows
//! changesets to be aggregated together. The [`ChangeSet`]s may be persisted in
//! aggregated form, or they can be persisted separately and reaggregated when
//! (re-)initializing our [`bdk::Wallet`].
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
mod test {
    use super::*;

    #[test]
    fn default_changeset_is_empty() {
        assert!(ChangeSet::default().is_empty());
    }

    // TODO(max): Add some snapshot tests
    // TODO(max): Add some arbitrary impls and roundtrip tests
}
