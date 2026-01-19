//! Webhook types and functionality for payment notifications.

// TODO(a-mpch): Remove once types are used
#![allow(dead_code)]

use std::{collections::HashMap, sync::Arc};

use common::api::user::UserPk;
use lexe_api::types::payments::{
    LxPaymentId, PaymentCreatedIndex, PaymentUpdatedIndex,
};
use node_client::{
    client::NodeClient,
    credentials::{ClientCredentials, Credentials},
};
use sdk_core::types::SdkPayment;
use serde::{Deserialize, Serialize};

/// Request to track a payment for webhook notification.
pub(crate) struct TrackRequest {
    pub user_pk: UserPk,
    pub credentials: Arc<Credentials>,
    /// The payment's created index, used to initialize the cursor.
    pub payment_created_index: PaymentCreatedIndex,
}

/// JSON payload POSTed to the user's webhook URL when a payment finalizes.
#[derive(Serialize)]
pub struct WebhookPayload {
    /// The user's public key.
    pub user_pk: UserPk,
    /// The full payment information.
    #[serde(flatten)]
    pub payment: SdkPayment,
}

/// Per-user state for JSON persistence.
///
/// Note: Only users with [`ClientCredentials`] are be persisted. Users with
/// `RootSeed` credentials are skipped during persistence.
#[derive(Serialize, Deserialize)]
pub(crate) struct PersistedUserTrackingState {
    pub credentials: ClientCredentials,
    /// Cursor for `get_updated_payments`. Initialized from the payment's
    /// `created_at` timestamp.
    pub cursor: PaymentUpdatedIndex,
    /// Payment IDs we're tracking.
    pub pending: Vec<LxPaymentId>,
}

/// Wrapper for JSON persistence of all users' payment tracking state.
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct PersistedTrackingState {
    pub users: HashMap<UserPk, PersistedUserTrackingState>,
}

/// Per-user tracking state (in-memory).
pub(crate) struct UserTrackingState {
    /// Persistable state (credentials, cursor, pending payments).
    pub inner: PersistedUserTrackingState,
    /// Cached [`NodeClient`] created from credentials.
    pub node_client: NodeClient,
}
