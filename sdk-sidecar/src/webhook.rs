//! Webhook types and functionality for payment notifications.

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    borrow::Cow,
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use common::{api::user::UserPk, env::DeployEnv, rng::SysRng};
use lexe_api::{
    def::AppNodeRunApi,
    models::command::GetUpdatedPayments,
    types::payments::{
        LxPaymentId, PaymentCreatedIndex, PaymentStatus, PaymentUpdatedIndex,
        VecBasicPaymentV2,
    },
};
use lexe_std::{Apply, backoff};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use node_client::{
    client::{GatewayClient, NodeClient},
    credentials::{ClientCredentials, Credentials},
};
use reqwest::Url;
use sdk_core::types::SdkPayment;
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, mpsc};
use tracing::{info, info_span, warn};

/// Polling interval for checking payment updates.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Channel buffer size for track requests.
const TRACK_REQUEST_BUFFER: usize = 64;

/// Maximum concurrent `get_updated_payments` API calls.
const MAX_CONCURRENT_API_CALLS: usize = 4;

/// Maximum number of webhook delivery attempts.
const MAX_WEBHOOK_ATTEMPTS: u32 = 3;

/// Filename for persisted tracking state.
const TRACKED_PAYMENTS_FILE: &str = "tracked_payments.json";

/// Configuration for spawning a [`WebhookSender`].
pub(crate) struct WebhookSenderConfig {
    pub url: Url,
    pub sidecar_dir: PathBuf,
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
    sidecar_dir: PathBuf,
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

        let mut sender = Self {
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
                    self.poll_all_users_and_send_webhooks().await;
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
    async fn poll_all_users_and_send_webhooks(&mut self) {
        let semaphore = Semaphore::new(MAX_CONCURRENT_API_CALLS);

        let results = self
            .users
            .iter()
            .map(|(user_pk, state)| async {
                let user_pk = *user_pk;
                let semaphore = &semaphore;
                let _permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(_) => return None,
                };
                let req = GetUpdatedPayments {
                    start_index: Some(state.inner.cursor),
                    limit: None,
                };
                let result = state.node_client.get_updated_payments(req).await;
                Some((user_pk, result))
            })
            .apply(futures::future::join_all)
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<(UserPk, Result<VecBasicPaymentV2, _>)>>();

        let mut finalized: Vec<(UserPk, SdkPayment)> = Vec::new();

        for (user_pk, result) in results {
            let payments = match result {
                Ok(resp) => resp.payments,
                Err(e) => {
                    warn!(%user_pk, "Failed to get updated payments: {e:#}");
                    continue;
                }
            };

            let state = self
                .users
                .get_mut(&user_pk)
                .expect("Should always find user_pk");

            for payment in payments {
                let payment_index = PaymentUpdatedIndex {
                    updated_at: payment.updated_at,
                    id: payment.id,
                };
                if payment_index > state.inner.cursor {
                    state.inner.cursor = payment_index;
                }

                // Check if this payment is one we're tracking
                if !state.inner.pending.contains(&payment.id) {
                    continue;
                }

                // Check if payment is finalized (completed or failed)
                if payment.status == PaymentStatus::Pending {
                    continue;
                }

                // Remove from pending and queue webhook
                state.inner.pending.retain(|id| *id != payment.id);
                finalized.push((user_pk, SdkPayment::from(payment)));
            }
        }

        // Send all webhooks in parallel
        finalized
            .into_iter()
            .map(|(user_pk, payment)| self.send_webhook(user_pk, payment))
            .apply(futures::future::join_all)
            .await;

        // Cleanup users with no pending payments
        self.users
            .retain(|_, state| !state.inner.pending.is_empty());
    }

    /// Send a webhook for a finalized payment with exponential backoff retry.
    async fn send_webhook(&self, user_pk: UserPk, payment: SdkPayment) {
        let payment_id = payment.id;
        let payload = WebhookPayload { user_pk, payment };
        let mut backoff = backoff::get_backoff_iter();

        for attempt in 1..=MAX_WEBHOOK_ATTEMPTS {
            let result = self
                .http_client
                .post(self.webhook_url.clone())
                .json(&payload)
                .send()
                .await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    info!(
                        %user_pk, %payment_id, "Webhook delivered successfully"
                    );
                    return;
                }
                Ok(resp) => warn!(
                    %user_pk, %payment_id, attempt,
                    status = %resp.status(),
                    "Webhook delivery failed with non-success status"
                ),

                Err(e) => warn!(
                    %user_pk, %payment_id, attempt,
                    "Webhook delivery failed: {e:#}"
                ),
            }

            if attempt < MAX_WEBHOOK_ATTEMPTS {
                let delay =
                    backoff.next().expect("Backoff iterator is infinite");
                tokio::time::sleep(delay).await;
            }
        }

        warn!(
            %user_pk, %payment_id,
            "Webhook delivery failed after {MAX_WEBHOOK_ATTEMPTS} attempts"
        );
    }

    /// Persist tracking state to disk.
    fn persist_state(&self) {
        if let Err(e) = self.persist_state_inner() {
            warn!("Failed to persist tracking state: {e:#}");
        }
    }

    fn persist_state_inner(&self) -> anyhow::Result<()> {
        let state = PersistedTrackingState {
            users: self
                .users
                .iter()
                .map(|(user_pk, state)| (*user_pk, state.inner.clone()))
                .collect(),
        };

        fs::create_dir_all(&self.sidecar_dir)?;

        let path = self.sidecar_dir.join(TRACKED_PAYMENTS_FILE);
        let json = serde_json::to_string(&state)?;

        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);

        #[cfg(unix)]
        opts.mode(0o600);

        let mut file = opts.open(&path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        Ok(())
    }

    /// Load persisted tracking state from disk.
    fn load_state(&mut self) {
        let path = self.sidecar_dir.join(TRACKED_PAYMENTS_FILE);

        if !path.exists() {
            warn!("No persisted tracking state found");
            return;
        }

        let json = match std::fs::read_to_string(&path) {
            Ok(json) => json,
            Err(e) => {
                warn!(?path, "Failed to read tracking state: {e:#}");
                return;
            }
        };

        let state: PersistedTrackingState = match serde_json::from_str(&json) {
            Ok(state) => state,
            Err(e) => {
                warn!(?path, "Failed to deserialize tracking state: {e:#}");
                return;
            }
        };

        // Rebuild NodeClients from persisted credentials
        for (user_pk, persisted) in state.users {
            let credentials =
                Credentials::ClientCredentials(persisted.credentials.clone());
            let (client_creds, node_client) =
                match self.build_node_client(&credentials) {
                    Ok(result) => result,
                    Err(e) => {
                        warn!(%user_pk, "Failed to rebuild NodeClient: {e:#}");
                        continue;
                    }
                };

            self.users.insert(
                user_pk,
                UserTrackingState {
                    inner: PersistedUserTrackingState {
                        credentials: client_creds,
                        cursor: persisted.cursor,
                        pending: persisted.pending,
                    },
                    node_client,
                },
            );
        }

        info!(
            num_users = self.users.len(),
            "Loaded persisted tracking state"
        );
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
#[derive(Clone, Serialize, Deserialize)]
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
