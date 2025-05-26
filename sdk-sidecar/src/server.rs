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
        .route("/v1/node/payment", get(node::get_payment))
        .with_state(state)
}

mod sidecar {
    use std::borrow::Cow;

    use lexe_api::server::LxJson;
    use sdk_core::SdkApiError;
    use tracing::instrument;

    use crate::api::HealthCheckResponse;

    #[instrument(skip_all, name = "(health)")]
    pub(crate) async fn health(
    ) -> Result<LxJson<HealthCheckResponse>, SdkApiError> {
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
        models::command::{GetNewPayments, PaymentIndexes},
        server::{extract::LxQuery, LxJson},
        types::payments::{LxPaymentId, PaymentIndex},
    };
    use sdk_core::{
        models::{
            SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
            SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfoResponse,
            SdkPayInvoiceRequest, SdkPayInvoiceResponse,
        },
        types::SdkPayment,
        SdkApiError,
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
        let resp = state.node_client.create_invoice(req.into()).await?;

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
                SdkApiError::command(
                    "Failed to lookup payment index for invoice",
                )
            })?;

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
            index: PaymentIndex { id, created_at },
            created_at,
        };

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(get-payment)")]
    pub(crate) async fn get_payment(
        state: State<Arc<RouterState>>,
        LxQuery(req): LxQuery<SdkGetPaymentRequest>,
    ) -> Result<LxJson<SdkGetPaymentResponse>, SdkApiError> {
        // TODO(max): Replace this with a call to a payment-specific API which
        // doesn't need to hit the DB
        let indexes = vec![req.index];
        let req = PaymentIndexes { indexes };

        let basic_payment = {
            let mut payments = state
                .node_client
                .get_payments_by_indexes(req)
                .await?
                .payments;

            payments.truncate(1);

            match payments.pop() {
                Some(p) => p,
                None =>
                    return Ok(LxJson(SdkGetPaymentResponse { payment: None })),
            }
        };

        let payment = Some(SdkPayment {
            index: basic_payment.index,
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
        });

        Ok(LxJson(SdkGetPaymentResponse { payment }))
    }
}
