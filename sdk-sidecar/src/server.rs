use std::sync::Arc;

use app_rs::client::NodeClient;
use axum::{
    routing::{get, post},
    Router,
};

pub(crate) struct RouterState {
    pub node_client: NodeClient,
}

pub(crate) fn router(state: Arc<RouterState>) -> Router<()> {
    Router::new()
        .route("/v1/health", get(sidecar::health))
        .route("/v1/node/node_info", get(node::node_info))
        .route("/v1/node/create_invoice", post(node::create_invoice))
        .route("/v1/node/pay_invoice", post(node::pay_invoice))
        .route("/v1/node/payment", get(node::payment))
        .with_state(state)
}

mod sidecar {
    use common::api::error::NodeApiError;
    use lexe_api::server::LxJson;
    use serde::Serialize;
    use tracing::instrument;

    #[derive(Serialize)]
    pub(crate) struct HealthCheck {
        status: &'static str,
    }

    #[instrument(skip_all, name = "(health)")]
    pub(crate) async fn health() -> Result<LxJson<HealthCheck>, NodeApiError> {
        Ok(LxJson(HealthCheck { status: "ok" }))
    }
}

mod node {
    use std::sync::Arc;

    use axum::extract::State;
    use common::{
        api::{
            command::{
                CreateInvoiceRequest, GetNewPayments, NodeInfo,
                PayInvoiceRequest, PaymentIndexes,
            },
            def::AppNodeRunApi,
            error::NodeApiError,
        },
        ln::payments::{LxPaymentId, PaymentIndex},
    };
    use lexe_api::server::{extract::LxQuery, LxJson};
    use tracing::instrument;

    use super::{
        model::{
            CreateInvoiceResponse, GetPaymentByIndexRequest,
            GetPaymentByIndexResponse, PayInvoiceResponse,
        },
        RouterState,
    };

    #[instrument(skip_all, name = "(node-info)")]
    pub(crate) async fn node_info(
        state: State<Arc<RouterState>>,
    ) -> Result<LxJson<NodeInfo>, NodeApiError> {
        state.node_client.node_info().await.map(LxJson)
    }

    #[instrument(skip_all, name = "(create-invoice)")]
    pub(crate) async fn create_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<CreateInvoiceRequest>,
    ) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
        let resp = state.node_client.create_invoice(req).await?;

        // HACK: temporary hack to lookup `PaymentIndex` for new invoice.
        // TODO(phlip9): original response should include the PaymentIndex.
        let invoice = resp.invoice;
        let resp = state
            .node_client
            .get_new_payments(GetNewPayments {
                // `start_index` is exclusive. use the invoice `created_at`
                // (which is different from the payment `created_at` and
                // currently guaranteed to be before the payment `created_at`)
                // to get us close to the newly registered payment.
                start_index: Some(PaymentIndex {
                    created_at: invoice.saturating_created_at(),
                    id: LxPaymentId::MIN,
                }),
                // Lookup a few payments just in case we raced with other new
                // payments.
                limit: Some(3),
            })
            .await?;

        // Look for the newly registered invoice payment in the response by
        // it's payment id.
        let id = invoice.payment_id();
        let index = resp
            .payments
            .into_iter()
            .find_map(|p| {
                if p.index.id == id {
                    Some(p.index)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                NodeApiError::command(
                    "Failed to lookup payment index for invoice",
                )
            })?;

        Ok(LxJson(CreateInvoiceResponse::new(index, invoice)))
    }

    #[instrument(skip_all, name = "(pay-invoice)")]
    pub(crate) async fn pay_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<PayInvoiceRequest>,
    ) -> Result<LxJson<PayInvoiceResponse>, NodeApiError> {
        let id = req.invoice.payment_id();
        state
            .node_client
            .pay_invoice(req)
            .await
            .map(|resp| PayInvoiceResponse::new(id, resp.created_at))
            .map(LxJson)
    }

    #[instrument(skip_all, name = "(payment)")]
    pub(crate) async fn payment(
        state: State<Arc<RouterState>>,
        LxQuery(req): LxQuery<GetPaymentByIndexRequest>,
    ) -> Result<LxJson<GetPaymentByIndexResponse>, NodeApiError> {
        let req = PaymentIndexes::from(req);
        let resp = state.node_client.get_payments_by_indexes(req).await?;
        Ok(LxJson(GetPaymentByIndexResponse::from(resp)))
    }
}

mod model {
    use common::{
        api::command::PaymentIndexes,
        ln::{
            amount::Amount,
            invoice::LxInvoice,
            payments::{
                BasicPayment, LxPaymentHash, LxPaymentId, LxPaymentSecret,
                PaymentIndex, VecBasicPayment,
            },
        },
        time::TimestampMs,
    };
    use serde::{Deserialize, Serialize};

    // --- enriched request/response types for dumb clients --- //

    /// The response to a `create_invoice` request. Contains the encoded
    /// invoice, the payment index, and various decoded fields from the
    /// invoice for convenience.
    #[derive(Serialize)]
    pub(crate) struct CreateInvoiceResponse {
        pub index: PaymentIndex,
        pub invoice: LxInvoice,
        pub description: Option<String>,
        pub amount: Option<Amount>,
        pub created_at: TimestampMs,
        pub expires_at: TimestampMs,
        pub payment_hash: LxPaymentHash,
        pub payment_secret: LxPaymentSecret,
    }

    /// The response to a `pay_invoice` request. Contains the payment index
    /// and `created_at` timestamp.
    #[derive(Serialize)]
    pub(crate) struct PayInvoiceResponse {
        pub index: PaymentIndex,
        pub created_at: TimestampMs,
    }

    #[derive(Deserialize)]
    pub(crate) struct GetPaymentByIndexRequest {
        index: PaymentIndex,
    }

    #[derive(Serialize)]
    pub(crate) struct GetPaymentByIndexResponse {
        payment: Option<BasicPayment>,
    }

    // --- Conversions --- //

    impl CreateInvoiceResponse {
        pub fn new(index: PaymentIndex, invoice: LxInvoice) -> Self {
            let description = invoice.description_str().map(|s| s.to_owned());
            let amount_sats = invoice.amount();
            let created_at = invoice.saturating_created_at();
            let expires_at = invoice.saturating_expires_at();
            let payment_hash = invoice.payment_hash();
            let payment_secret = invoice.payment_secret();

            Self {
                index,
                invoice,
                description,
                amount: amount_sats,
                created_at,
                expires_at,
                payment_hash,
                payment_secret,
            }
        }
    }

    impl PayInvoiceResponse {
        pub fn new(id: LxPaymentId, created_at: TimestampMs) -> Self {
            Self {
                index: PaymentIndex { id, created_at },
                created_at,
            }
        }
    }

    impl From<GetPaymentByIndexRequest> for PaymentIndexes {
        fn from(req: GetPaymentByIndexRequest) -> Self {
            Self {
                indexes: vec![req.index],
            }
        }
    }

    impl From<VecBasicPayment> for GetPaymentByIndexResponse {
        fn from(mut resp: VecBasicPayment) -> Self {
            resp.payments.truncate(1);
            let payment = resp.payments.pop();
            Self { payment }
        }
    }
}
