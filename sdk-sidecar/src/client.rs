use lexe_api::{rest::RestClient, types::Empty};
use sdk_core::{
    SdkApiError, SdkErrorKind,
    def::SdkApi,
    models::{
        SdkCreateInvoiceRequest, SdkCreateInvoiceResponse,
        SdkGetPaymentRequest, SdkGetPaymentResponse, SdkNodeInfoResponse,
        SdkPayInvoiceRequest, SdkPayInvoiceResponse,
    },
    types::SdkPayment,
};

use crate::{api::HealthCheckResponse, def::UserSidecarApi};

// TODO(max): Test all of these methods in smoketests.

/// A Rust client to a `lexe-sidecar` server.
///
/// This mostly exists so the Sidecar SDK can be integration tested, but SDK
/// users working with Rust are welcome to use this client with the caveat that
/// Lexe does NOT provide Rust stability guarantees for this client - only API
/// stability for the JSON REST API itself.
pub struct SidecarClient {
    sidecar_url: String,
    rest: RestClient,
}

impl SidecarClient {
    /// Example `sidecar_url`: "http://127.0.0.1:5393"
    pub fn new(sidecar_url: String) -> Self {
        let (from, to) = ("sidecar-client", "sidecar");
        let rest = RestClient::new_insecure(from, to);
        Self { sidecar_url, rest }
    }

    pub fn sidecar_url(&self) -> &str {
        &self.sidecar_url
    }
}

impl UserSidecarApi for SidecarClient {
    async fn health_check(&self) -> Result<HealthCheckResponse, SdkApiError> {
        let url = format!("{base}/v2/health", base = self.sidecar_url);
        let http_req = self.rest.get(url, &Empty {});
        self.rest.send(http_req).await
    }
}

impl SdkApi for SidecarClient {
    async fn node_info(&self) -> Result<SdkNodeInfoResponse, SdkApiError> {
        let url = format!("{base}/v2/node/node_info", base = self.sidecar_url);
        let http_req = self.rest.get(url, &Empty {});
        self.rest.send(http_req).await
    }

    async fn create_invoice(
        &self,
        req: &SdkCreateInvoiceRequest,
    ) -> Result<SdkCreateInvoiceResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/create_invoice");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn pay_invoice(
        &self,
        req: &SdkPayInvoiceRequest,
    ) -> Result<SdkPayInvoiceResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/pay_invoice");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    /// NOTE: See server handler for why we deserialize as [`SdkPayment`]
    /// rather than [`SdkGetPaymentResponse`], and why we check for HTTP 404.
    async fn get_payment(
        &self,
        req: &SdkGetPaymentRequest,
    ) -> Result<SdkGetPaymentResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/payment");
        let http_req = self.rest.get(url, req);

        self.rest
            .send::<SdkPayment, SdkApiError>(http_req)
            .await
            .map(|payment| SdkGetPaymentResponse {
                payment: Some(payment),
            })
            .or_else(|error| match error.kind {
                SdkErrorKind::NotFound =>
                    Ok(SdkGetPaymentResponse { payment: None }),
                _ => Err(error),
            })
    }
}
