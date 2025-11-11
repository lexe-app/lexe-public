use std::sync::Arc;

use app_rs::client::NodeClient;
use axum::{
    Router,
    routing::{get, post},
};

pub(crate) struct RouterState {
    pub node_client: NodeClient,
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
    use std::borrow::Cow;

    use lexe_api::server::LxJson;
    use sdk_core::SdkApiError;
    use tracing::instrument;

    use crate::api::HealthCheckResponse;

    #[instrument(skip_all, name = "(health)")]
    pub(crate) async fn health()
    -> Result<LxJson<HealthCheckResponse>, SdkApiError> {
        Ok(LxJson(HealthCheckResponse {
            status: Cow::from("ok"),
        }))
    }
}

mod node {
    use std::sync::Arc;

    use axum::extract::State;
    use lexe_api::{
        def::AppNodeRunApi,
        error::NodeErrorKind,
        models::command::{CreateInvoiceResponse, LxPaymentIdStruct},
        server::{LxJson, extract::LxQuery},
        types::payments::PaymentCreatedIndex,
    };
    use sdk_core::{
        SdkApiError,
        models::{
            SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
            SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfoResponse,
            SdkPayInvoiceRequest, SdkPayInvoiceResponse,
        },
        types::SdkPayment,
    };
    use tracing::instrument;

    use super::RouterState;

    #[instrument(skip_all, name = "(node-info)")]
    pub(crate) async fn node_info(
        state: State<Arc<RouterState>>,
    ) -> Result<LxJson<SdkNodeInfoResponse>, SdkApiError> {
        state
            .node_client
            .node_info()
            .await
            .map(SdkNodeInfoResponse::from)
            .map(LxJson)
    }

    #[instrument(skip_all, name = "(create-invoice)")]
    pub(crate) async fn create_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<SdkCreateInvoiceRequest>,
    ) -> Result<LxJson<SdkCreateInvoiceResponse>, SdkApiError> {
        let CreateInvoiceResponse {
            invoice,
            created_index: maybe_index,
        } = state.node_client.create_invoice(req.into()).await?;

        let index = maybe_index
            .ok_or("Node out-of-date. Upgrade to node-v0.8.10 or later.")
            .map_err(SdkApiError::command)?;

        Ok(LxJson(SdkCreateInvoiceResponse::new(index, invoice)))
    }

    #[instrument(skip_all, name = "(pay-invoice)")]
    pub(crate) async fn pay_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<SdkPayInvoiceRequest>,
    ) -> Result<LxJson<SdkPayInvoiceResponse>, SdkApiError> {
        let id = req.invoice.payment_id();
        let created_at =
            state.node_client.pay_invoice(req.into()).await?.created_at;
        let resp = SdkPayInvoiceResponse {
            index: PaymentCreatedIndex { id, created_at },
            created_at,
        };

        Ok(LxJson(resp))
    }

    /// Legacy: Returns `{ "payment": null }` if not found.
    #[instrument(skip_all, name = "(get-payment-v1)")]
    pub(crate) async fn get_payment_v1(
        state: State<Arc<RouterState>>,
        req: LxQuery<SdkGetPaymentRequest>,
    ) -> Result<LxJson<SdkGetPaymentResponse>, SdkApiError> {
        // Wraps the v2 logic to return `{ "payment": null }` if not found.
        match get_payment(state, req).await {
            Ok(LxJson(payment)) => Ok(LxJson(SdkGetPaymentResponse {
                payment: Some(payment),
            })),
            Err(e) if e.kind == NodeErrorKind::NotFound =>
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
        state: State<Arc<RouterState>>,
        LxQuery(req): LxQuery<SdkGetPaymentRequest>,
    ) -> Result<LxJson<SdkPayment>, SdkApiError> {
        let id = req.index.id;

        let maybe_basic_payment = state
            .node_client
            .get_payment_by_id(LxPaymentIdStruct { id })
            .await?
            .maybe_payment;

        let basic_payment = match maybe_basic_payment {
            Some(p) => p,
            None => return Err(SdkApiError::not_found("Payment not found")),
        };

        let payment = SdkPayment {
            index: basic_payment.created_index(),
            kind: basic_payment.kind,
            direction: basic_payment.direction,
            txid: basic_payment.txid,
            replacement: basic_payment.replacement,
            amount: basic_payment.amount,
            fees: basic_payment.fees,
            status: basic_payment.status,
            status_msg: basic_payment.status_str,
            note: basic_payment.note,
            finalized_at: basic_payment.finalized_at,
        };

        Ok(LxJson(payment))
    }
}
