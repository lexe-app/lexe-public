//! Lexe wallet database.

use std::sync::{Arc, Mutex};

use anyhow::{Context, anyhow};
use node_client::client::NodeClient;
use tracing::{info, warn};

use super::{
    ffs::{DiskFs, fsext},
    provision_history::ProvisionHistory,
};
use crate::{
    config::WalletUserDbConfig,
    payments_db::{self, PaymentSyncSummary, PaymentsDb},
};

/// Persistent wallet database which can be used with [`LexeWallet`].
///
/// [`LexeWallet`]: crate::wallet::LexeWallet
pub struct WalletDb<F> {
    user_db_config: WalletUserDbConfig,

    payments_db: PaymentsDb<F>,
    payment_sync_lock: tokio::sync::Mutex<()>,

    // TODO(max): Remove once enclaves_to_provision is calculated server-side.
    provision_ffs: F,
    provision_history: Arc<Mutex<ProvisionHistory>>,
}

// TODO(max): Rework Ffs so this impl can be generic across all Ffs impls.
// The user should just be able to give us a Ffs impl set to some base path
// which is the lexe_data_dir. From there we should be able to create sub-Ffs's
// for the wallet env, user, payments db, etc. Probably instead of one level of
// directory it should be prefix-based. As the Ffs is currently designed, end
// users have to manually create all the subdivisions, all the way down to e.g.
// `payments_db`, which is tedious and error-prone. Also, it might need to be
// renamed, since it won't be flat anymore.
impl WalletDb<DiskFs> {
    /// Create a fresh [`WalletDb`], deleting any existing data for this user.
    pub fn fresh(user_db_config: WalletUserDbConfig) -> anyhow::Result<Self> {
        let payments_ffs =
            DiskFs::create_clean_dir_all(user_db_config.payments_db_dir())
                .context("Could not create payments ffs")?;

        // Delete the old payments_db dir just in case it exists.
        for old_dir in user_db_config.old_payment_db_dirs() {
            match fsext::remove_dir_all_idempotent(&old_dir) {
                Ok(()) => info!("Deleted old payments_db dir: {old_dir:?}"),
                Err(e) => warn!(?old_dir, "Couldn't delete old dir: {e:#}"),
            }
        }

        let provision_ffs =
            DiskFs::create_clean_dir_all(user_db_config.provision_db_dir())
                .context("Could not create provision ffs")?;
        let provision_history = Arc::new(Mutex::new(ProvisionHistory::new()));

        Ok(Self {
            user_db_config,
            payments_db: PaymentsDb::empty(payments_ffs),
            payment_sync_lock: tokio::sync::Mutex::new(()),
            provision_ffs,
            provision_history,
        })
    }

    /// Load an existing [`WalletDb`] (or create a new one if none exists).
    pub fn load(user_db_config: WalletUserDbConfig) -> anyhow::Result<Self> {
        let payments_ffs =
            DiskFs::create_dir_all(user_db_config.payments_db_dir())
                .context("Could not create payments ffs")?;

        let payments_db = PaymentsDb::read(payments_ffs)
            .context("Failed to load payments db")?;

        // If the payments_db contains 0 payments, the user may have just
        // upgraded to the latest format. Delete the old dirs just in case.
        let num_payments = payments_db.num_payments();
        if num_payments == 0 {
            for old_dir in user_db_config.old_payment_db_dirs() {
                match fsext::remove_dir_all_idempotent(&old_dir) {
                    Ok(()) => info!("Deleted old payments_db dir: {old_dir:?}"),
                    Err(e) => warn!(?old_dir, "Couldn't delete old dir: {e:#}"),
                }
            }
        }

        let num_pending = payments_db.num_pending();
        let latest_updated_index = payments_db.latest_updated_index();
        info!(
            %num_payments, %num_pending, ?latest_updated_index,
            "Loaded WalletDb."
        );

        let provision_ffs =
            DiskFs::create_dir_all(user_db_config.provision_db_dir())
                .context("Could not create provision ffs")?;
        let provision_history = ProvisionHistory::read_from_ffs(&provision_ffs)
            .context("Could not read provision history")?;

        // Log the latest provisioned enclave
        match provision_history.provisioned.last() {
            Some(latest) => info!(
                version = %latest.version,
                measurement = %latest.measurement,
                machine_id = %latest.machine_id,
                "Latest provisioned: "
            ),
            None => info!("Empty provision history"),
        }

        Ok(Self {
            user_db_config,
            payments_db,
            payment_sync_lock: tokio::sync::Mutex::new(()),
            provision_ffs,
            provision_history: Arc::new(Mutex::new(provision_history)),
        })
    }

    /// Get the user database configuration.
    pub fn user_db_config(&self) -> &WalletUserDbConfig {
        &self.user_db_config
    }

    /// Get a reference to the payments database.
    pub fn payments_db(&self) -> &PaymentsDb<DiskFs> {
        &self.payments_db
    }

    /// Get a reference to the provision file system.
    pub fn provision_ffs(&self) -> &DiskFs {
        &self.provision_ffs
    }

    /// Get a reference to the provision history.
    pub fn provision_history(&self) -> &Arc<Mutex<ProvisionHistory>> {
        &self.provision_history
    }

    /// Sync payments from the user node.
    ///
    /// Only one sync can run at a time.
    /// Errors if another sync is already in progress.
    pub async fn sync_payments(
        &self,
        node_client: &NodeClient,
        batch_size: u16,
    ) -> anyhow::Result<PaymentSyncSummary> {
        // TODO(max): Should we switch to lock().await?
        let _lock = self.payment_sync_lock.try_lock().map_err(|_| {
            anyhow!(
                "Another task is syncing payments. \
                 Only one task should sync payments at a time."
            )
        })?;

        payments_db::sync_payments(&self.payments_db, node_client, batch_size)
            .await
    }
}
