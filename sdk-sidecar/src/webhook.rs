//! Webhook types and functionality for payment notifications.

// TODO(a-mpch): Remove once types are used
#![allow(dead_code)]

use std::{
    borrow::Cow, collections::HashMap, path::PathBuf, sync::Arc, time::Duration,
};

use common::{api::user::UserPk, env::DeployEnv, rng::SysRng};
use lexe_api::types::payments::{
    LxPaymentId, PaymentCreatedIndex, PaymentUpdatedIndex,
};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::{ClientCredentials, Credentials},
};
use reqwest::Url;
use sdk_core::types::SdkPayment;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, info_span, warn};

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
    pub(crate) fn spawn(self) -> LxTask<()> {
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

    /// Handle a track request idempotently by adding payment to tracking and
    /// min-setting cursor.
    fn handle_track_request(&mut self, req: TrackRequest) {
        let payment_id = req.payment_created_index.id;
        let new_cursor = PaymentUpdatedIndex {
            updated_at: req.payment_created_index.created_at,
            id: payment_id,
        };

        let (client_creds, node_client) =
            match self.build_node_client(&req.credentials) {
                Ok(result) => result,
                Err(e) => {
                    warn!(user_pk = %req.user_pk,
                        "Failed to build NodeClient: {e:#}"
                    );
                    return;
                }
            };

        match self.users.get_mut(&req.user_pk) {
            Some(state) => {
                // User exists: add payment, min-set cursor
                if !state.inner.pending.contains(&payment_id) {
                    state.inner.pending.push(payment_id);
                }

                // Min-set the cursor in case we receive the payment finalized
                // update before the payment has started tracking.
                if new_cursor < state.inner.cursor {
                    state.inner.cursor = new_cursor;
                }

                // Update the credentials to the latest given in case the
                // user has rotated to a different client credential.
                if state.inner.credentials.client_pk != client_creds.client_pk {
                    state.inner.credentials = client_creds;
                    state.node_client = node_client;
                }
            }
            None => {
                // New user: create tracking state
                let state = UserTrackingState {
                    inner: PersistedUserTrackingState {
                        credentials: client_creds,
                        cursor: new_cursor,
                        pending: vec![payment_id],
                    },
                    node_client,
                };
                self.users.insert(req.user_pk, state);
            }
        }
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

    /// Build a [`NodeClient`] from credentials.
    fn build_node_client(
        &self,
        credentials: &Credentials,
    ) -> anyhow::Result<(ClientCredentials, NodeClient)> {
        let Credentials::ClientCredentials(client_creds) = credentials else {
            anyhow::bail!("Webhooks don't support RootSeed credentials");
        };

        let gateway_client = GatewayClient::new(
            self.deploy_env,
            self.gateway_url.clone(),
            crate::USER_AGENT,
        )?;

        let node_client = NodeClient::new(
            &mut SysRng::new(),
            true, // use_sgx
            self.deploy_env,
            gateway_client,
            credentials.as_ref(),
        )?;

        Ok((client_creds.clone(), node_client))
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
