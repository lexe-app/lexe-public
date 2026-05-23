//! The [`WebhookSender`] allows for sidecar users to receive notifications
//! via webhook `POST` whenever a payment is finalized. (See [`is_finalized`]).
//!
//! The notification format is described by [`WebhookPayload`].
//!
//! To receive webhook notifications, configure the sidecar with either
//! - CLI arg: `--webhook-url <url>`
//! - Environment variable: `LEXE_WEBHOOK_URL=<url>`
//!
//! Outbound webhooks can optionally be signed using the "Standard Webhooks"
//! HMAC-SHA256 scheme by configuring a shared secret with the receiver:
//! - CLI arg: `--webhook-secret <secret>`
//! - Environment variable: `LEXE_WEBHOOK_SECRET=<secret>`
//!
//! Full webhook documentation is at <https://docs.lexe.tech/sidecar/webhooks/>.
//!
//! [`is_finalized`]: lexe::types::payment::PaymentStatus::is_finalized
//! [`WebhookSender`]: crate::webhook::WebhookSender
//! [`WebhookPayload`]: crate::webhook::WebhookPayload

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, anyhow, ensure};
use bytes::Bytes;
use lexe::{
    config::WalletEnvConfig,
    types::{
        auth::{ClientCredentials, Credentials, CredentialsRef, UserPk},
        command::{GetPaymentRequest, GetUpdatedPaymentsRequest},
        payment::Payment,
    },
    wallet::LexeWallet,
};
use lexe_api::types::payments::{PaymentCreatedIndex, PaymentUpdatedIndex};
use lexe_common::time::TimestampMs;
use lexe_crypto::ed25519;
use lexe_std::{Apply, backoff};
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use quick_cache::unsync;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use standardwebhooks::Webhook as WebhookSigner;
use tokio::{
    sync::{Semaphore, mpsc},
    time::MissedTickBehavior,
};
use tracing::{error, info, info_span, warn};

/// Cache of [`LexeWallet`]s, keyed by [`ClientCredentials`] `client_pk`.
/// Shared between the sidecar server and webhook sender.
pub(crate) type WalletCache =
    unsync::Cache<ed25519::PublicKey, Arc<LexeWallet>>;

/// Filename for persisted tracking state. By default, state is persisted in
/// `$HOME/.lexe/sidecar/<wallet_env>/tracked_payments.json`.
const TRACKED_PAYMENTS_FILE: &str = "tracked_payments.json";

/// Polling interval for checking payment updates.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Channel buffer size for track requests.
const TRACK_REQUEST_BUFFER: usize = 64;

/// Maximum concurrent `get_updated_payments` API calls.
const MAX_CONCURRENT_API_CALLS: usize = 4;

/// Maximum number of webhook delivery attempts.
const MAX_WEBHOOK_ATTEMPTS: u32 = 3;

/// Request to track a payment for webhook notification.
pub(crate) struct TrackRequest {
    /// Indicates the [`LexeWallet`] associated with the payment.
    pub creds_or_default: CredentialsOrDefault,
    /// The payment's created index, used to initialize the cursor.
    pub payment_created_index: PaymentCreatedIndex,
}

/// Either full per-request credentials or a pointer to the default wallet.
///
/// Sent from the server to the webhook sender in a [`TrackRequest`] so that
/// the sender can locate (or construct) the [`LexeWallet`] to track payments
/// against.
///
/// - The `PerRequest` variant carries the [`ClientCredentials`] used to build
///   the wallet on demand (one wallet per distinct client pk, cached).
/// - The `Default` variant carries just the default wallet's `user_pk`, since
///   the default wallet itself is already loaded at sidecar startup.
#[derive(Serialize, Deserialize)]
pub(crate) enum CredentialsOrDefault {
    /// Per-request wallet built from [`ClientCredentials`].
    PerRequest(ClientCredentials),
    /// Default wallet, identified by its `user_pk`.
    Default(UserPk),
}

impl From<&Credentials> for CredentialsOrDefault {
    fn from(value: &Credentials) -> Self {
        match value {
            Credentials::ClientCredentials(cc) => Self::PerRequest(cc.clone()),
            Credentials::RootSeed(seed) => Self::Default(seed.derive_user_pk()),
        }
    }
}

/// JSON payload `POST`ed to the user's webhook URL when a payment finalizes.
///
/// The top-level `type` and `timestamp` fields follow the "Standard Webhooks"
/// recommended payload structure; the rest of the fields are flattened from
/// [`Payment`] alongside `user_pk`.
#[derive(Serialize)]
pub struct WebhookPayload {
    /// Event type discriminator. Always `"payment.finalized"` for now.
    pub r#type: &'static str,
    /// When the underlying event occurred (ISO 8601).
    /// For a finalized payment, this is `finalized_at`.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// The user's public key. Allows for identifying the user associated with
    /// the payment in the case of per-request authorization.
    pub user_pk: UserPk,
    /// The full payment information.
    #[serde(flatten)]
    pub payment: Payment,
}

/// Sends webhook notifications when payments finalize.
///
/// Runs as a dedicated task that receives track requests via channel and
/// polls for payment updates using `get_updated_payments` batched tailing.
///
/// Persists tracking state in `tracked_wallets` to `tracked_payments_path`
/// on disk.
pub(crate) struct WebhookSender {
    /// In-memory state for the wallets we're currently tracking payments for.
    /// Persisted to disk and rebuilt from `tracked_payments_path` on startup.
    tracked_wallets: HashMap<WalletKey, WalletState>,
    /// Full path to the persisted tracking-state file, e.g.
    /// `$HOME/.lexe/sidecar/<wallet_env>/tracked_payments.json`.
    tracked_payments_path: PathBuf,

    /// Wallet environment of the payments being tracked.
    wallet_env: WalletEnvConfig,
    /// The default wallet and credentials to be used.
    default_wallet: Option<Arc<LexeWallet>>,
    /// In-memory cache of wallets associated with tracked payments.
    /// Only wallets using [`ClientCredentials`] are cached; the cache is
    /// keyed by client pk.
    cache: Arc<Mutex<WalletCache>>,

    /// Webhook URL to send payment notifications to.
    /// Notification format is described by [`WebhookPayload`].
    webhook_url: Url,
    /// Optional signer for outbound webhooks ("Standard Webhooks"
    /// HMAC-SHA256). When `Some`, every webhook delivery carries
    /// `webhook-id`, `webhook-timestamp`, and `webhook-signature` headers.
    webhook_signer: Option<WebhookSigner>,
    /// The HTTP client used for webhook delivery.
    http_client: reqwest::Client,

    /// Receives track request from HTTP handlers.
    webhook_sender_rx: mpsc::Receiver<TrackRequest>,
    /// Shutdown signal.
    shutdown: NotifyOnce,
}

/// Per-wallet state for payments being tracked. Persisted to disk.
#[derive(Serialize, Deserialize)]
pub(crate) struct WalletState {
    /// Tracks the latest payment update we have received.
    pub cursor: PaymentUpdatedIndex,
    /// The indices of the payments being tracked.
    pub tracked_payments: HashSet<PaymentCreatedIndex>,
    /// Credentials (or `user_pk` for the default wallet) used to (re)build
    /// the associated [`LexeWallet`].
    pub creds_or_default: CredentialsOrDefault,
}

/// Hashmap key identifying a unique [`LexeWallet`] being tracked.
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
pub(crate) enum WalletKey {
    /// The default wallet, keyed by its `user_pk`.
    Default(UserPk),
    /// A per-request wallet, keyed by its `client_pk`.
    PerRequest(ed25519::PublicKey),
}

impl Display for WalletKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default(pk) => write!(f, "D{pk}"),
            Self::PerRequest(pk) => write!(f, "P{pk}"),
        }
    }
}

impl FromStr for WalletKey {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        let head = chars.next().context("empty string")?;
        let tail = chars.as_str();
        match head {
            'D' => Ok(Self::Default(UserPk::from_str(tail)?)),
            'P' => Ok(Self::PerRequest(ed25519::PublicKey::from_str(tail)?)),
            _ => Err(anyhow!("invalid prefix")),
        }
    }
}

impl From<&CredentialsOrDefault> for WalletKey {
    fn from(value: &CredentialsOrDefault) -> Self {
        match value {
            CredentialsOrDefault::Default(pk) => Self::Default(*pk),
            CredentialsOrDefault::PerRequest(cc) =>
                Self::PerRequest(cc.unstable().client_pk),
        }
    }
}

// --- impl WebhookSender --- //

impl WebhookSender {
    /// Create a new webhook sender and return the channel sender to track
    /// requests.
    ///
    /// `sidecar_dir` is the base directory for the webhook sender's data
    /// persistence; state is persisted in
    /// `<sidecar_dir>/<wallet_env>/tracked_payments.json`.
    pub fn new(
        default_wallet: Option<Arc<LexeWallet>>,
        shutdown: NotifyOnce,
        sidecar_dir: PathBuf,
        url: Url,
        webhook_signer: Option<WebhookSigner>,
        wallet_cache: Arc<Mutex<WalletCache>>,
        wallet_env: WalletEnvConfig,
    ) -> (Self, mpsc::Sender<TrackRequest>) {
        let (tx, rx) = mpsc::channel(TRACK_REQUEST_BUFFER);

        // Build HTTP client with proper TLS configuration
        #[allow(clippy::disallowed_methods)]
        let tls_config = lexe_tls_core::rustls::ClientConfig::builder()
            .with_root_certificates(lexe_tls_core::WEBPKI_ROOT_CERTS.clone())
            .with_no_client_auth();
        let http_client = reqwest::ClientBuilder::new()
            .use_preconfigured_tls(tls_config)
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest::ClientBuilder::build failed");

        let tracked_payments_path = sidecar_dir
            .join(wallet_env.wallet_env.to_string())
            .join(TRACKED_PAYMENTS_FILE);

        info!(
            "Webhook sender tracking state will be persisted at {}",
            tracked_payments_path.display()
        );

        let sender = Self {
            tracked_wallets: Self::load_wallets(&tracked_payments_path),
            tracked_payments_path,
            cache: wallet_cache,
            default_wallet,
            wallet_env,
            webhook_sender_rx: rx,
            webhook_url: url,
            webhook_signer,
            http_client,
            shutdown,
        };

        (sender, tx)
    }

    /// Spawn a new task that runs the webhook sender.
    pub(crate) fn spawn(self) -> LxTask<()> {
        LxTask::spawn_with_span(
            "(webhook-sender)",
            info_span!("(webhook-sender)"),
            self.run(),
        )
    }

    /// Main run loop that handles track requests and polls for payment updates.
    async fn run(mut self) {
        info!("Webhook sender task started");

        let mut poll_interval = tokio::time::interval(POLL_INTERVAL);
        poll_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                biased;

                () = self.shutdown.recv() => {
                    info!("Shutdown signal received");
                    break;
                }

                Some(req) = self.webhook_sender_rx.recv() => {
                    self.handle_track_request(req).await;
                    // TODO(nicole): Consider writing incrementally
                    self.persist_state();
                }

                _ = poll_interval.tick() => {
                    self.poll_wallets_and_send_webhooks().await;
                    self.persist_state();
                }
            }
        }

        // Final persist before shutdown
        self.persist_state();
        info!("Webhook sender task stopped");
    }

    /// Handle a track request idempotently by adding the payment to
    /// `self.tracked_wallets`. Payments which have already been finalized will
    /// not be added, but a webhook notification will still be sent.
    async fn handle_track_request(&mut self, req: TrackRequest) {
        let payment_created_index = req.payment_created_index;
        let handle_track_request_inner = async || -> anyhow::Result<()> {
            // By the time we receive the TrackRequest, the payment might have
            // been finalized already; call `get_payment` and check
            let wallet = self
                .get_wallet(&req.creds_or_default)
                .context("Couldn't get wallet")?;
            let get_payment_req = GetPaymentRequest {
                index: req.payment_created_index,
            };
            let resp = wallet
                .get_payment(get_payment_req)
                .await
                .context("Failed to make get_payment request")?;
            if let Some(p) = resp.payment
                && p.status.is_finalized()
            {
                // Send webhook
                let user_pk = wallet.user_config().user_pk;
                self.send_webhook(user_pk, p).await;
                return Ok(());
            }

            // Get the tracking state or create a new one;
            // From finalization check above, time-wise:
            //   old_cursor <= now < finalization of new payment
            // meaning we don't need to update the cursor.
            let wallet_key = WalletKey::from(&req.creds_or_default);
            self.tracked_wallets.entry(wallet_key).or_insert_with(|| {
                WalletState {
                    creds_or_default: req.creds_or_default,
                    cursor: PaymentUpdatedIndex {
                        id: req.payment_created_index.id,
                        // Set updated_at to current payment created_at
                        updated_at: req.payment_created_index.created_at,
                    },
                    tracked_payments: HashSet::from(
                        [req.payment_created_index],
                    ),
                }
            });

            Ok(())
        };

        let result = handle_track_request_inner().await;
        if let Err(e) = result {
            warn!(
                "Couldn't track payment with index \
                 {payment_created_index}: {e:#}"
            );
        }
    }

    /// Poll all tracked wallets for payment updates and send webhooks.
    async fn poll_wallets_and_send_webhooks(&mut self) {
        // Limit the number of concurrent API calls
        let semaphore = Semaphore::new(MAX_CONCURRENT_API_CALLS);

        // We make API calls to `get_updated_payments`, storing the results
        // instead of consuming them so we can ignore failures
        let results = self
            .tracked_wallets
            .iter()
            .map(async |(wallet_key, state)| -> Result<_, (_, _)> {
                // Get the associated wallet
                let wallet = self
                    .get_wallet(&state.creds_or_default)
                    .with_context(|| {
                        anyhow!("Couldn't fetch nor create a wallet")
                    })
                    .map_err(|e| (*wallet_key, e))?;

                // Call `get_updated_payments`, with concurrency limit
                let _permit = semaphore
                    .acquire()
                    .await
                    .expect("Semaphore should not have been closed");
                let req = GetUpdatedPaymentsRequest {
                    start_index: Some(state.cursor),
                    limit: None,
                };
                let resp = wallet
                    .get_updated_payments(req)
                    .await
                    .with_context(|| {
                        anyhow!("Couldn't make request to get updated payments")
                    })
                    .map_err(|e| (*wallet_key, e))?;

                let user_pk = wallet.user_config().user_pk;

                Ok((user_pk, *wallet_key, resp))
            })
            .apply(futures::future::join_all)
            .await;

        // Update tracking state according to updated payments and
        // gather finalized payments
        let mut finalized_payments = vec![];
        for result in results {
            match result {
                Ok((user_pk, wallet_key, resp)) => {
                    let state = self
                        .tracked_wallets
                        .get_mut(&wallet_key)
                        .expect("Wallet key came from prior iteration");

                    // Update cursor
                    if let Some(index) = resp.updated_index
                        && index > state.cursor
                    {
                        state.cursor = index;
                    }

                    // Check the updated payments
                    for payment in resp.payments {
                        // If the payment is one we are tracking, and is
                        // finalized, then queue a webhook.
                        //
                        // Check `is_finalized` *before* removing so that
                        // intermediate (non-finalizing) updates don't
                        // prematurely remove the payment from tracking.
                        //
                        // It's safe to remove the payment from the state before
                        // sending the webhook because the state is persisted
                        // only after `poll_wallets_and_send_webhooks` returns.
                        if payment.status.is_finalized()
                            && state.tracked_payments.remove(&payment.index)
                        {
                            // Queue webhook
                            finalized_payments.push((user_pk, payment));
                        }
                    }

                    // Clean up tracking state if we're done with this wallet
                    if state.tracked_payments.is_empty() {
                        self.tracked_wallets.remove(&wallet_key);
                    }
                }
                Err((wallet_key, e)) => {
                    let wallet_hint = match wallet_key {
                        WalletKey::Default(_) => "default wallet".to_string(),
                        WalletKey::PerRequest(pk) =>
                            format!("client credentials with public key {pk}"),
                    };
                    warn!(
                        "Failed to check for payment updates \
                         using {wallet_hint}: {e:#}"
                    );
                }
            }
        }

        // Send all webhooks in parallel
        finalized_payments
            .into_iter()
            .map(|(user_pk, payment)| self.send_webhook(user_pk, payment))
            .apply(futures::future::join_all)
            .await;
    }

    /// Send a webhook for a finalized payment with exponential backoff retry.
    async fn send_webhook(&self, user_pk: UserPk, payment: Payment) {
        let payment_index = payment.index;
        // Use `payment.index` as the event id and `finalized_at` as the event
        // timestamp so that retries across sidecar restarts replay the same
        // logical event (idempotency). The `pf-` prefix tags the event kind
        // ("payment finalized") so future event kinds are disambiguable.
        let event_id = format!("pf-{payment_index}");
        let finalized_at = payment
            .finalized_at
            .expect("Finalized payments should always have finalized_at");
        let timestamp =
            chrono::DateTime::from_timestamp_millis(finalized_at.to_i64())
                .expect("TimestampMs is in chrono's representable range");

        let payload = WebhookPayload {
            r#type: "payment.finalized",
            timestamp,
            user_pk,
            payment,
        };

        let mut backoff = backoff::get_backoff_iter();

        // Serialize only once to ensure the bytes on the wire are the exact
        // same as the bytes that are signed.
        let body = serde_json::to_vec(&payload)
            .map(Bytes::from)
            .expect("Always succeeds");

        for attempt in 1..=MAX_WEBHOOK_ATTEMPTS {
            let req = self
                .http_client
                .post(self.webhook_url.clone())
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());

            // Per the "Standard Webhooks" spec, re-sign each attempt with a
            // fresh timestamp so the receiver can reject replayed requests.
            //
            // This per-request timestamp is distinct from the per-event
            // timestamp which uses the payment's `finalized_at` value.
            let req = match &self.webhook_signer {
                Some(signer) => {
                    let request_ts = TimestampMs::now().to_secs() as i64;
                    let signature =
                        match signer.sign(&event_id, request_ts, &body) {
                            Ok(sig) => sig,
                            Err(e) =>
                                return warn!(
                                    %user_pk, %payment_index,
                                    "Failed to sign webhook payload: {e:#}"
                                ),
                        };
                    req.header(standardwebhooks::HEADER_WEBHOOK_ID, &event_id)
                        .header(
                            standardwebhooks::HEADER_WEBHOOK_TIMESTAMP,
                            request_ts.to_string(),
                        )
                        .header(
                            standardwebhooks::HEADER_WEBHOOK_SIGNATURE,
                            signature,
                        )
                }
                None => req,
            };

            let result = req.send().await;

            match result {
                Ok(resp) if resp.status().is_success() => {
                    info!(
                        %user_pk, %payment_index, "Webhook delivered successfully"
                    );
                    return;
                }
                Ok(resp) => warn!(
                    %user_pk, %payment_index, attempt,
                    status = %resp.status(),
                    "Webhook delivery failed with non-success status"
                ),

                Err(e) => warn!(
                    %user_pk, %payment_index, attempt,
                    "Webhook delivery failed: {e:#}"
                ),
            }

            if attempt < MAX_WEBHOOK_ATTEMPTS {
                let delay =
                    backoff.next().expect("Backoff iterator is infinite");
                tokio::time::sleep(delay).await;
            }
        }

        error!(
            %user_pk, %payment_index,
            "Webhook delivery failed after {MAX_WEBHOOK_ATTEMPTS} attempts"
        );
    }

    /// Persist tracking state to `tracked_payments_path`.
    fn persist_state(&self) {
        let persist_state_inner = || -> anyhow::Result<()> {
            if let Some(parent) = self.tracked_payments_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let json_string = serde_json::to_string(&self.tracked_wallets)?;

            let mut opts = OpenOptions::new();
            opts.write(true).create(true).truncate(true);

            #[cfg(unix)]
            opts.mode(0o600);

            let mut file = opts.open(&self.tracked_payments_path)?;
            file.write_all(json_string.as_bytes())?;
            file.sync_all()?;

            Ok(())
        };

        if let Err(e) = persist_state_inner() {
            warn!("Failed to persist tracking state: {e:#}");
        }
    }

    /// Load persisted tracking state from `tracked_payments_path`.
    fn load_wallets(
        tracked_payments_path: &Path,
    ) -> HashMap<WalletKey, WalletState> {
        if !tracked_payments_path.exists() {
            info!("No persisted tracking states found.");
            return HashMap::new();
        }

        let json_string = match std::fs::read_to_string(tracked_payments_path) {
            Ok(contents) => contents,
            Err(e) => {
                warn!(?tracked_payments_path, "Failed to read: {e:#}");
                return HashMap::new();
            }
        };

        let wallets: HashMap<WalletKey, WalletState> =
            match serde_json::from_str(&json_string) {
                Ok(state) => state,
                Err(e) => {
                    warn!(
                        ?tracked_payments_path,
                        "Failed to deserialize json '{json_string}': {e:#}"
                    );
                    return HashMap::new();
                }
            };

        info!(
            num_wallets = wallets.len(),
            "Loaded persisted tracking state"
        );

        wallets
    }

    /// Get a [`LexeWallet`] based on a [`CredentialsOrDefault`].
    /// Find the wallet in cache; if none, create a new wallet and add to cache.
    fn get_wallet(
        &self,
        creds_or_default: &CredentialsOrDefault,
    ) -> anyhow::Result<Arc<LexeWallet>> {
        let wallet = match creds_or_default {
            CredentialsOrDefault::Default(user_pk) => {
                let default = self
                    .default_wallet
                    .clone()
                    .ok_or_else(|| anyhow!("Couldn't get default wallet"))?;
                let default_user_pk = &default.user_config().user_pk;
                ensure!(
                    default_user_pk == user_pk,
                    "Tried to get default wallet with user pk of \
                     {default_user_pk}, but the current default wallet has a \
                     user pk of {user_pk}"
                );
                default
            }
            CredentialsOrDefault::PerRequest(cc) => {
                let client_pk = &cc.unstable().client_pk;
                let mut locked_cache = self.cache.lock().unwrap();
                match locked_cache.get(client_pk) {
                    Some(cached_wallet) => cached_wallet.clone(),
                    None => {
                        // Create new
                        let wallet = LexeWallet::without_db(
                            self.wallet_env.clone(),
                            CredentialsRef::from(cc),
                        )
                        .map(Arc::new)
                        .context("Couldn't create new wallet")?;
                        // Add to cache
                        locked_cache.insert(*client_pk, wallet.clone());
                        wallet
                    }
                }
            }
        };
        Ok(wallet)
    }
}
