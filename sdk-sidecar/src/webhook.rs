//! Webhook types and functionality for payment notifications.

// TODO(a-mpch): Remove once types are used
#![allow(dead_code)]

use std::{
    borrow::Cow, collections::HashMap, path::PathBuf, sync::Arc, time::Duration,
};

use common::{api::user::UserPk, env::DeployEnv};
use lexe_api::types::payments::{
    LxPaymentId, PaymentCreatedIndex, PaymentUpdatedIndex,
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use node_client::{
    client::NodeClient,
    credentials::{ClientCredentials, Credentials},
};
use reqwest::Url;
use sdk_core::types::SdkPayment;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, info_span};

/// Polling interval for checking payment updates.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Channel buffer size for track requests.
const TRACK_REQUEST_BUFFER: usize = 64;

/// Configuration for spawning a [`WebhookSender`].
pub(crate) struct WebhookSenderConfig {
    pub url: Url,
    pub sidecar_dir: Option<PathBuf>,
    pub deploy_env: DeployEnv,
    pub gateway_url: Cow<'static, str>,
    pub shutdown: NotifyOnce,
}

/// Sends webhook notifications when payments finalize.
///
/// Runs as a dedicated task that receives track requests via channel and
/// polls for payment updates using `get_updated_payments` batched tailing.
pub(crate) struct WebhookSender {
    /// Per-user in-memory tracking state.
    users: HashMap<UserPk, UserTrackingState>,
    /// Receives track request from HTTP handlers.
    webhook_sender_rx: mpsc::Receiver<TrackRequest>,
    /// Global webhook URL.
    webhook_url: Url,
    /// HTTP client for webhook delivery.
    http_client: reqwest::Client,
    /// Data directory for persisting tracking state.
    sidecar_dir: Option<PathBuf>,
    /// Gateway URL used to construct [`NodeClient`].
    gateway_url: Cow<'static, str>,
    /// Deploy environment used to construct [`NodeClient`].
    deploy_env: DeployEnv,
    /// Shutdown signal.
    shutdown: NotifyOnce,
}

impl WebhookSender {
    /// Create a new webhook sender and return the channel sender to track
    /// requests.
    pub fn new(
        config: WebhookSenderConfig,
    ) -> (Self, mpsc::Sender<TrackRequest>) {
        let (tx, rx) = mpsc::channel(TRACK_REQUEST_BUFFER);

        let sender = Self {
            users: HashMap::new(),
            webhook_sender_rx: rx,
            webhook_url: config.url,
            http_client: reqwest::Client::new(),
            sidecar_dir: config.sidecar_dir,
            gateway_url: config.gateway_url,
            deploy_env: config.deploy_env,
            shutdown: config.shutdown,
        };
        sender.load_state();

        (sender, tx)
    }

    /// Spawn a new task that runs the webhook sender.
    pub fn spawn(self) -> LxTask<()> {
        LxTask::spawn_with_span(
            "(webhook-sender)",
            info_span!("(webhook-sender)"),
            async move { self.run().await },
        )
    }

    /// Main run loop that handles track requests and polls for payment updates.
    async fn run(mut self) {
        info!("Webhook sender task started");

        loop {
            tokio::select! {
                biased;

                () = self.shutdown.recv() => {
                    info!("Shutdown signal received");
                    break;
                }

                Some(req) = self.webhook_sender_rx.recv() => {
                    self.handle_track_request(req);
                    self.persist_state();
                }

                () = tokio::time::sleep(POLL_INTERVAL) => {
                    self.poll_all_users().await;
                    self.persist_state();
                }
            }
        }

        // Final persist before shutdown
        self.persist_state();
        info!("Webhook sender task stopped");
    }

    /// Handle a track request: add payment to tracking, min-set cursor.
    fn handle_track_request(&mut self, _req: TrackRequest) {
        // TODO(a-mpch): Implement handle_track_request
    }

    /// Poll all users for payment updates and send webhooks.
    async fn poll_all_users(&mut self) {
        // TODO(a-mpch): Implement poll_all_users
    }

    /// Persist tracking state to disk.
    fn persist_state(&self) {
        // TODO(a-mpch): Implement persist_state
    }

    /// Load persisted tracking state from disk.
    fn load_state(&self) {
        // TODO(a-mpch): Implement load_state
    }
}

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

/// Per-user tracking state (in-memory).
pub(crate) struct UserTrackingState {
    /// Persistable state (credentials, cursor, pending payments).
    pub inner: PersistedUserTrackingState,
    /// Cached [`NodeClient`] created from credentials.
    pub node_client: NodeClient,
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
