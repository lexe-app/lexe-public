use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    extract::{FromRequestParts, State},
    routing::{get, post},
};
use common::{ed25519, env::DeployEnv};
use lexe_api::{
    def::AppNodeRunApi,
    error::{SdkApiError, SdkErrorKind},
    models::command::{CreateInvoiceResponse, LxPaymentIdStruct},
    server::{LxJson, extract::LxQuery},
    types::payments::PaymentCreatedIndex,
};
use node_client::{client::NodeClient, credentials::Credentials};
use quick_cache::unsync;
use sdk_core::{
    models::{
        SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
        SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfo,
        SdkPayInvoiceRequest, SdkPayInvoiceResponse,
    },
    types::SdkPayment,
};
use tokio::sync::mpsc;
use tracing::{instrument, warn};

use crate::{
    api::HealthCheckResponse,
    extract::{CredentialsExtractor, NodeClientExtractor},
    webhook::TrackRequest,
};

const CLIENT_CACHE_CAPACITY: usize = 64;

pub(crate) struct RouterState {
    /// The default [`NodeClient`] and [`Credentials`] from env/CLI.
    /// Used when no per-request credentials are provided.
    pub default: Option<(NodeClient, Arc<Credentials>)>,
    /// Caches `NodeClient`s by their `client_pk`.
    pub client_cache: Mutex<unsync::Cache<ed25519::PublicKey, NodeClient>>,
    pub deploy_env: DeployEnv,
    pub gateway_url: Cow<'static, str>,
    /// Channel to send track requests to the webhook sender.
    pub webhook_tx: Option<mpsc::Sender<TrackRequest>>,
}

impl RouterState {
    pub fn new(
        default: Option<(NodeClient, Arc<Credentials>)>,
        deploy_env: DeployEnv,
        gateway_url: Cow<'static, str>,
        webhook_tx: Option<mpsc::Sender<TrackRequest>>,
    ) -> Self {
        let client_cache =
            Mutex::new(unsync::Cache::new(CLIENT_CACHE_CAPACITY));
        Self {
            default,
            client_cache,
            deploy_env,
            gateway_url,
            webhook_tx,
        }
    }
}

pub(crate) fn router(state: Arc<RouterState>) -> Router<()> {
    // NOTE: If making a breaking change, bump the version of *all* endpoints.
    // This is because we don't want to trip up dumb AIs which fail to
    // distinguish between v1/v2. A consistent version is more reliable.
    Router::new()
        // v2
        .route("/v2/health", get(sidecar::health))
        .route("/v2/node/node_info", get(node::node_info))
        .route("/v2/node/create_invoice", post(node::create_invoice))
        .route("/v2/node/pay_invoice", post(node::pay_invoice))
        .route("/v2/node/payment", get(node::get_payment))
        // v1 (legacy)
        .route("/v1/health", get(sidecar::health))
        .route("/v1/node/node_info", get(node::node_info))
        .route("/v1/node/create_invoice", post(node::create_invoice))
        .route("/v1/node/pay_invoice", post(node::pay_invoice))
        .route("/v1/node/payment", get(node::get_payment_v1))
        .with_state(state)
}

mod sidecar {
    use super::*;

    #[instrument(skip_all, name = "(health)")]
    pub(crate) async fn health(
        state: State<Arc<RouterState>>,
        mut parts: http::request::Parts,
    ) -> Result<LxJson<HealthCheckResponse>, SdkApiError> {
        let maybe_credentials =
            CredentialsExtractor::from_request_parts(&mut parts, &state)
                .await?;

        let has_default = state.default.is_some();
        let has_request_credentials = maybe_credentials.0.is_some();

        let status = if has_default || has_request_credentials {
            Cow::from("ok")
        } else {
            // TODO(max): Mention root seed options later if desired.
            Cow::from(
                "warning: No client credentials configured. \
                 Set LEXE_CLIENT_CREDENTIALS in env or pass credentials \
                 per-request via the Authorization header.",
            )
        };

        Ok(LxJson(HealthCheckResponse { status }))
    }
}

mod node {
    use super::*;

    #[instrument(skip_all, name = "(node-info)")]
    pub(crate) async fn node_info(
        State(_): State<Arc<RouterState>>,
        NodeClientExtractor { node_client, .. }: NodeClientExtractor,
    ) -> Result<LxJson<SdkNodeInfo>, SdkApiError> {
        let info = node_client
            .node_info()
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(SdkNodeInfo::from(info)))
    }

    #[instrument(skip_all, name = "(create-invoice)")]
    pub(crate) async fn create_invoice(
        State(state): State<Arc<RouterState>>,
        NodeClientExtractor {
            node_client,
            credentials,
        }: NodeClientExtractor,
        LxJson(req): LxJson<SdkCreateInvoiceRequest>,
    ) -> Result<LxJson<SdkCreateInvoiceResponse>, SdkApiError> {
        let CreateInvoiceResponse {
            invoice,
            created_index: maybe_index,
        } = node_client
            .create_invoice(req.into())
            .await
            .map_err(SdkApiError::command)?;

        let index = maybe_index
            .ok_or("Node out-of-date. Upgrade to node-v0.8.10 or later.")
            .map_err(SdkApiError::command)?;

        try_track_payment(&state, &node_client, credentials, index);

        Ok(LxJson(SdkCreateInvoiceResponse::new(index, invoice)))
    }

    #[instrument(skip_all, name = "(pay-invoice)")]
    pub(crate) async fn pay_invoice(
        State(state): State<Arc<RouterState>>,
        NodeClientExtractor {
            node_client,
            credentials,
        }: NodeClientExtractor,
        LxJson(req): LxJson<SdkPayInvoiceRequest>,
    ) -> Result<LxJson<SdkPayInvoiceResponse>, SdkApiError> {
        let id = req.invoice.payment_id();
        let created_at = node_client
            .pay_invoice(req.into())
            .await
            .map_err(SdkApiError::command)?
            .created_at;

        let index = PaymentCreatedIndex { id, created_at };
        try_track_payment(&state, &node_client, credentials, index);

        Ok(LxJson(SdkPayInvoiceResponse { index, created_at }))
    }

    /// Legacy: Returns `{ "payment": null }` if not found.
    #[instrument(skip_all, name = "(get-payment-v1)")]
    pub(crate) async fn get_payment_v1(
        state: State<Arc<RouterState>>,
        node_client: NodeClientExtractor,
        req: LxQuery<SdkGetPaymentRequest>,
    ) -> Result<LxJson<SdkGetPaymentResponse>, SdkApiError> {
        // Wraps the v2 logic to return `{ "payment": null }` if not found.
        match get_payment(state, node_client, req).await {
            Ok(LxJson(payment)) => Ok(LxJson(SdkGetPaymentResponse {
                payment: Some(payment),
            })),
            Err(e) if e.kind == SdkErrorKind::NotFound =>
                Ok(LxJson(SdkGetPaymentResponse { payment: None })),
            Err(e) => Err(e),
        }
    }

    /// NOTE: For the v2 endpoint and above, we return the response as a
    /// [`SdkPayment`] rather than a [`SdkGetPaymentResponse`], because the
    /// `{ "payment": { ... } }` nesting trips up dumb AIs when vibe-coding on
    /// the Sidecar SDK, as discovered by Mat Balez et al. If the payment is
    /// missing, we use HTTP 404 to indicate this.
    ///
    /// If we need to add more fields to the response which don't fit in
    /// [`SdkPayment`], we can always reintroduce the response type but with
    /// `#[serde(flatten)]` on the [`SdkPayment`] field (since missing payments
    /// are now indicated by HTTP 404), or do another version bump.
    #[instrument(skip_all, name = "(get-payment)")]
    pub(crate) async fn get_payment(
        State(_): State<Arc<RouterState>>,
        NodeClientExtractor { node_client, .. }: NodeClientExtractor,
        LxQuery(req): LxQuery<SdkGetPaymentRequest>,
    ) -> Result<LxJson<SdkPayment>, SdkApiError> {
        let id = req.index.id;

        let maybe_basic_payment = node_client
            .get_payment_by_id(LxPaymentIdStruct { id })
            .await
            .map_err(SdkApiError::command)?
            .maybe_payment;

        let basic_payment = match maybe_basic_payment {
            Some(p) => p,
            None => return Err(SdkApiError::not_found("Payment not found")),
        };

        Ok(LxJson(SdkPayment::from(basic_payment)))
    }

    /// Try to track a payment for webhook notifications.
    ///
    /// No-op if webhooks are not configured.
    fn try_track_payment(
        state: &RouterState,
        node_client: &NodeClient,
        credentials: Arc<Credentials>,
        index: PaymentCreatedIndex,
    ) {
        let Some(tx) = &state.webhook_tx else { return };

        let Some(user_pk) = node_client.user_pk() else {
            warn!(
                "Webhook tracking unavailable: credentials created \
                 before node-v0.8.11 do not include user_pk"
            );
            return;
        };

        let req = TrackRequest {
            user_pk,
            credentials,
            payment_created_index: index,
        };

        if tx.try_send(req).is_err() {
            warn!("Webhook channel full, payment not tracked");
        }
    }
}
