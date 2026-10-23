//! The [`WebhookSender`] `POST`s a [`WebhookPayload`] to the sidecar user's
//! webhook URL whenever a payment [is finalized].
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
//! [is finalized]: lexe::types::payment::PaymentStatus::is_finalized
//! [`WebhookSender`]: crate::webhook::WebhookSender
//! [`WebhookPayload`]: crate::webhook::WebhookPayload

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::{HashMap, HashSet, hash_map::Entry},
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
use futures::{StreamExt, stream::FuturesUnordered};
use lexe::{
    config::WalletEnvConfig,
    types::{
        auth::{ClientCredentials, Credentials, CredentialsRef, UserPk},
        payment::Payment,
    },
    wallet::LexeWallet,
};
use lexe_api::types::payments::PaymentCreatedIndex;
use lexe_common::time::TimestampMs;
use lexe_crypto::ed25519;
use lexe_std::backoff;
use lexe_tokio::{notify_once::NotifyOnce, task::LxTask};
use quick_cache::unsync;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use standardwebhooks::Webhook as WebhookSigner;
use tokio::sync::mpsc;
use tracing::{error, info, info_span, warn};

/// Cache of [`LexeWallet`]s, keyed by [`ClientCredentials`] `client_pk`.
/// Shared between the sidecar server and webhook sender.
pub(crate) type WalletCache =
    unsync::Cache<ed25519::PublicKey, Arc<LexeWallet>>;

/// Filename for persisted tracking state. By default, state is persisted in
/// `$HOME/.lexe/sidecar/<wallet_env>/tracked_payments.json`.
const TRACKED_PAYMENTS_FILE: &str = "tracked_payments.json";

/// Channel buffer size for track requests.
const TRACK_REQUEST_BUFFER: usize = 64;

/// Maximum number of webhook delivery attempts.
const MAX_WEBHOOK_ATTEMPTS: u32 = 3;

/// Request to track a payment for webhook notification.
pub(crate) struct TrackRequest {
    pub creds_or_default: CredentialsOrDefault,
    pub payment_created_index: PaymentCreatedIndex,
}

/// Identifies the [`LexeWallet`] to load for a tracked payment.
#[derive(Serialize, Deserialize)]
pub(crate) enum CredentialsOrDefault {
    /// A per-request wallet, loaded on demand with its [`ClientCredentials`].
    PerRequest(ClientCredentials),
    /// The default wallet. It is already loaded at sidecar startup, so its
    /// `user_pk` is enough to identify it.
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
/// recommended payload structure.
#[derive(Serialize)]
pub struct WebhookPayload {
    /// Event type discriminator. Always `"payment.finalized"` for now.
    pub r#type: &'static str,
    /// When the underlying event occurred (ISO 8601).
    /// For a finalized payment, this is `finalized_at`.
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Identifies the payment's user when using per-request authorization.
    pub user_pk: UserPk,
    /// The full payment information.
    #[serde(flatten)]
    pub payment: Payment,
}

/// Sends webhook notifications when payments finalize.
///
/// Runs as a task which receives [`TrackRequest`]s and spawns a waiter task
/// per tracked payment. Each waiter delivers the webhook once its payment
/// finalizes.
///
/// Tracked payments are persisted to disk, so tracking resumes after a restart.
pub(crate) struct WebhookSender {
    /// The payments being tracked, grouped by wallet.
    tracked_wallets: HashMap<WalletKey, WalletState>,
    /// Where `tracked_wallets` is persisted, e.g.
    /// `$HOME/.lexe/sidecar/<wallet_env>/tracked_payments.json`.
    tracked_payments_path: PathBuf,

    wallet_env: WalletEnvConfig,
    /// The wallet for the sidecar's default credentials, if configured.
    default_wallet: Option<Arc<LexeWallet>>,
    wallet_cache: Arc<Mutex<WalletCache>>,
    /// Data dir for the wallets loaded into `wallet_cache`. The webhook sender
    /// doesn't need wallet persistence, but the server, which shares the
    /// cache, does.
    data_dir: PathBuf,

    client: Arc<WebhookClient>,
    /// One task per tracked payment, which returns the payment once delivery
    /// of its webhook has finished.
    waiters: FuturesUnordered<LxTask<(WalletKey, PaymentCreatedIndex)>>,

    /// Receives track requests from the HTTP handlers.
    webhook_sender_rx: mpsc::Receiver<TrackRequest>,
    shutdown: NotifyOnce,
}

/// Posts [`WebhookPayload`]s to the user's webhook URL.
struct WebhookClient {
    http_client: reqwest::Client,
    url: Url,
    /// Signs webhooks using the "Standard Webhooks" HMAC-SHA256 scheme, if a
    /// secret is configured.
    signer: Option<WebhookSigner>,
}

/// Per-wallet state for payments being tracked. Persisted to disk.
#[derive(Serialize, Deserialize)]
pub(crate) struct WalletState {
    pub tracked_payments: HashSet<PaymentCreatedIndex>,
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
    /// Tracking state is persisted at
    /// `<data_dir>/sidecar/<wallet_env>/tracked_payments.json`.
    pub fn new(
        default_wallet: Option<Arc<LexeWallet>>,
        shutdown: NotifyOnce,
        data_dir: PathBuf,
        url: Url,
        webhook_signer: Option<WebhookSigner>,
        wallet_cache: Arc<Mutex<WalletCache>>,
        wallet_env: WalletEnvConfig,
    ) -> (Self, mpsc::Sender<TrackRequest>) {
        let (tx, rx) = mpsc::channel(TRACK_REQUEST_BUFFER);

        #[allow(clippy::disallowed_methods)]
        let tls_config = lexe_tls_core::rustls::ClientConfig::builder()
            .with_root_certificates(lexe_tls_core::WEBPKI_ROOT_CERTS.clone())
            .with_no_client_auth();
        let http_client = reqwest::ClientBuilder::new()
            .use_preconfigured_tls(tls_config)
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest::ClientBuilder::build failed");

        let tracked_payments_path = data_dir
            .join("sidecar")
            .join(wallet_env.wallet_env.to_string())
            .join(TRACKED_PAYMENTS_FILE);

        info!(
            "Webhook sender tracking state will be persisted at {}",
            tracked_payments_path.display()
        );

        let client = Arc::new(WebhookClient {
            http_client,
            url,
            signer: webhook_signer,
        });

        let sender = Self {
            tracked_wallets: Self::load_wallets(&tracked_payments_path),
            tracked_payments_path,
            wallet_cache,
            data_dir,
            default_wallet,
            wallet_env,
            client,
            waiters: FuturesUnordered::new(),
            webhook_sender_rx: rx,
            shutdown,
        };

        (sender, tx)
    }

    pub(crate) fn spawn(self) -> LxTask<()> {
        LxTask::spawn_with_span(
            "(webhook-sender)",
            info_span!("(webhook-sender)"),
            self.run(),
        )
    }

    async fn run(mut self) {
        info!("Webhook sender task started");

        // Resume waiting on the payments tracked before the last shutdown.
        let tracked = self
            .tracked_wallets
            .iter()
            .flat_map(|(wallet_key, state)| {
                let indexes = state.tracked_payments.iter();
                indexes.map(move |index| (*wallet_key, *index))
            })
            .collect::<Vec<_>>();
        for (wallet_key, index) in tracked {
            self.spawn_waiter(wallet_key, index);
        }

        loop {
            tokio::select! {
                biased;

                () = self.shutdown.recv() => {
                    info!("Shutdown signal received");
                    break;
                }

                Some(req) = self.webhook_sender_rx.recv() => {
                    self.track_payment(
                        req.creds_or_default,
                        req.payment_created_index,
                    );
                    // TODO(nicole): Consider writing incrementally
                    self.persist_state();
                }

                Some(result) = self.waiters.next() => match result {
                    Ok((wallet_key, index)) => {
                        self.untrack_payment(wallet_key, index);
                        self.persist_state();
                    }
                    Err(e) => error!("Webhook waiter failed: {e:#}"),
                },
            }
        }

        self.persist_state();
        info!("Webhook sender task stopped");
    }

    /// Track the payment and spawn its waiter, unless already tracked.
    fn track_payment(
        &mut self,
        creds_or_default: CredentialsOrDefault,
        index: PaymentCreatedIndex,
    ) {
        let wallet_key = WalletKey::from(&creds_or_default);
        let newly_tracked = Self::insert_tracked(
            &mut self.tracked_wallets,
            creds_or_default,
            index,
        );
        if newly_tracked {
            self.spawn_waiter(wallet_key, index);
        }
    }

    /// Returns whether the payment was newly tracked.
    fn insert_tracked(
        tracked_wallets: &mut HashMap<WalletKey, WalletState>,
        creds_or_default: CredentialsOrDefault,
        index: PaymentCreatedIndex,
    ) -> bool {
        tracked_wallets
            .entry(WalletKey::from(&creds_or_default))
            .or_insert_with(|| WalletState {
                creds_or_default,
                tracked_payments: HashSet::new(),
            })
            .tracked_payments
            .insert(index)
    }

    fn untrack_payment(
        &mut self,
        wallet_key: WalletKey,
        index: PaymentCreatedIndex,
    ) {
        if let Entry::Occupied(mut entry) =
            self.tracked_wallets.entry(wallet_key)
        {
            entry.get_mut().tracked_payments.remove(&index);
            if entry.get().tracked_payments.is_empty() {
                entry.remove();
            }
        }
    }

    /// Untracks the payment if its wallet can't be loaded.
    fn spawn_waiter(
        &mut self,
        wallet_key: WalletKey,
        index: PaymentCreatedIndex,
    ) {
        let creds_or_default =
            &self.tracked_wallets[&wallet_key].creds_or_default;
        let wallet = match self.get_wallet(creds_or_default) {
            Ok(wallet) => wallet,
            Err(e) => {
                warn!("Couldn't track payment with index {index}: {e:#}");
                self.untrack_payment(wallet_key, index);
                return;
            }
        };

        let client = self.client.clone();
        let name = format!("(webhook-waiter-{index})");
        let waiter = LxTask::spawn_unlogged(name, async move {
            Self::wait_and_deliver(&wallet, &client, index).await;
            (wallet_key, index)
        });
        self.waiters.push(waiter);
    }

    async fn wait_and_deliver(
        wallet: &LexeWallet,
        client: &WebhookClient,
        index: PaymentCreatedIndex,
    ) {
        let mut backoff = backoff::get_backoff_iter();
        let timeout = None;
        let payment = loop {
            match wallet.wait_for_payment(index, timeout).await {
                Ok(payment) => break payment,
                Err(e) => {
                    warn!(%index, "Failed to wait for payment: {e:#}");
                    tokio::time::sleep(backoff.next_delay()).await;
                }
            }
        };

        let user_pk = wallet.user_config().user_pk;
        client.send(user_pk, payment).await;
    }

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
                let mut locked_cache = self.wallet_cache.lock().unwrap();
                match locked_cache.get(client_pk) {
                    Some(cached_wallet) => cached_wallet.clone(),
                    None => {
                        let wallet = LexeWallet::load_or_fresh(
                            self.wallet_env.clone(),
                            CredentialsRef::from(cc),
                            Some(self.data_dir.clone()),
                        )
                        .map(Arc::new)
                        .context("Couldn't create new wallet")?;
                        locked_cache.insert(*client_pk, wallet.clone());
                        wallet
                    }
                }
            }
        };
        Ok(wallet)
    }
}

// --- impl WebhookClient --- //

impl WebhookClient {
    /// Send a webhook for a finalized payment with exponential backoff retry.
    async fn send(&self, user_pk: UserPk, payment: Payment) {
        let payment_index = payment.index;

        // Use `payment.index` as the event id and `finalized_at` as the event
        // timestamp, so that retries across sidecar restarts replay the same
        // event. The `pf-` prefix stands for "payment finalized", which
        // distinguishes this event from future event kinds.
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

        // Serialize once, so that the bytes sent are exactly the bytes signed.
        let body = serde_json::to_vec(&payload)
            .map(Bytes::from)
            .expect("Always succeeds");

        for attempt in 1..=MAX_WEBHOOK_ATTEMPTS {
            let req = self
                .http_client
                .post(self.url.clone())
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());

            // Per the "Standard Webhooks" spec, re-sign each attempt with a
            // fresh timestamp so the receiver can reject replayed requests.
            //
            // This per-request timestamp is distinct from the per-event
            // timestamp which uses the payment's `finalized_at` value.
            let req = match &self.signer {
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
}

#[cfg(test)]
mod test {
    use lexe::types::payment::PaymentId;
    use lexe_common::ByteArray;

    use super::*;

    /// Tracking a second open payment for an already-tracked wallet must add
    /// it to the existing state rather than dropping it.
    #[test]
    fn insert_tracked_accumulates_per_wallet() {
        let user_pk = UserPk::from_array([1; 32]);
        let creds = || CredentialsOrDefault::Default(user_pk);
        let index = |secs: u32| PaymentCreatedIndex {
            created_at: TimestampMs::from_secs_u32(secs),
            id: PaymentId::MIN,
        };
        let mut tracked_wallets = HashMap::new();
        let mut insert = |index| {
            WebhookSender::insert_tracked(&mut tracked_wallets, creds(), index)
        };

        assert!(insert(index(1)));
        assert!(insert(index(2)));
        assert!(!insert(index(2)));

        assert_eq!(tracked_wallets.len(), 1);
        let state = &tracked_wallets[&WalletKey::Default(user_pk)];
        assert_eq!(state.tracked_payments, HashSet::from([index(1), index(2)]));
    }
}
