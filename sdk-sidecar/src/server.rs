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

    #[derive(Serialize)]
    pub(crate) struct HealthCheck {
        status: &'static str,
    }

    pub(crate) async fn health() -> Result<LxJson<HealthCheck>, NodeApiError> {
        Ok(LxJson(HealthCheck { status: "ok" }))
    }
}

mod node {
    use std::sync::Arc;

    use axum::extract::State;
    use common::api::{
        command::{
            CreateInvoiceRequest, CreateInvoiceResponse, NodeInfo,
            PayInvoiceRequest, PayInvoiceResponse, PaymentIndexes,
        },
        def::AppNodeRunApi,
        error::NodeApiError,
    };
    use lexe_api::server::{extract::LxQuery, LxJson};

    use super::{
        model::{GetPaymentByIndexRequest, GetPaymentByIndexResponse},
        RouterState,
    };

    pub(crate) async fn node_info(
        state: State<Arc<RouterState>>,
    ) -> Result<LxJson<NodeInfo>, NodeApiError> {
        state.node_client.node_info().await.map(LxJson)
    }

    pub(crate) async fn create_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<CreateInvoiceRequest>,
    ) -> Result<LxJson<CreateInvoiceResponse>, NodeApiError> {
        state.node_client.create_invoice(req).await.map(LxJson)
    }

    pub(crate) async fn pay_invoice(
        state: State<Arc<RouterState>>,
        LxJson(req): LxJson<PayInvoiceRequest>,
    ) -> Result<LxJson<PayInvoiceResponse>, NodeApiError> {
        state.node_client.pay_invoice(req).await.map(LxJson)
    }

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
        ln::payments::{BasicPayment, PaymentIndex, VecBasicPayment},
    };
    use serde::{Deserialize, Serialize};

    // --- enriched request/response types for dumb clients --- //

    #[derive(Deserialize)]
    pub(crate) struct GetPaymentByIndexRequest {
        index: PaymentIndex,
    }

    #[derive(Serialize)]
    pub(crate) struct GetPaymentByIndexResponse {
        payment: Option<BasicPayment>,
    }

    // --- Conversions --- //

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
