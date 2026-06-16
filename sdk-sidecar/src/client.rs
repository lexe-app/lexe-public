use lexe::types::{
    command::{
        AnalyzeRequest, CreateInvoiceRequest, CreateInvoiceResponse,
        CreateOfferRequest, CreateOfferResponse, GetPaymentRequest,
        GetPaymentResponse, GetUpdatedPaymentsRequest,
        GetUpdatedPaymentsResponse, ListPaymentsResponse, NodeInfo,
        PayInvoiceRequest, PayOfferRequest, PaymentSyncSummary,
        UpdatePersonalNoteRequest,
    },
    payment::Payment,
};
use lexe_api::{
    error::{SdkApiError, SdkErrorKind},
    rest::RestClient,
    types::Empty,
};

use crate::{
    api::{
        AnalyzeResponse, HealthCheckResponse, ListPaymentsRequest,
        PayLnurlRequest, PayRequest, SignupRequest, WithdrawLnurlRequest,
    },
    def::UserSidecarApi,
};

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

    async fn sync_payments(&self) -> Result<PaymentSyncSummary, SdkApiError> {
        let url =
            format!("{base}/v2/node/sync_payments", base = self.sidecar_url);
        let http_req = self.rest.put(url, &Empty {});
        self.rest.send(http_req).await
    }

    async fn list_payments(
        &self,
        req: &ListPaymentsRequest,
    ) -> Result<ListPaymentsResponse, SdkApiError> {
        let url =
            format!("{base}/v2/node/list_payments", base = self.sidecar_url);
        let http_req = self.rest.get(url, req);
        self.rest.send(http_req).await
    }

    async fn clear_payments(&self) -> Result<Empty, SdkApiError> {
        let url =
            format!("{base}/v2/node/clear_payments", base = self.sidecar_url);
        let http_req = self.rest.post(url, &Empty {});
        self.rest.send(http_req).await
    }

    async fn signup(&self, req: &SignupRequest) -> Result<Empty, SdkApiError> {
        let url = format!("{base}/v2/node/signup", base = self.sidecar_url);
        let http_req = self.rest.put(url, req);
        self.rest.send(http_req).await
    }

    async fn provision(&self) -> Result<Empty, SdkApiError> {
        let url = format!("{base}/v2/node/provision", base = self.sidecar_url);
        let http_req = self.rest.put(url, &Empty {});
        self.rest.send(http_req).await
    }

    async fn node_info(&self) -> Result<NodeInfo, SdkApiError> {
        let url = format!("{base}/v2/node/node_info", base = self.sidecar_url);
        let http_req = self.rest.get(url, &Empty {});
        self.rest.send(http_req).await
    }

    async fn analyze(
        &self,
        req: &AnalyzeRequest,
    ) -> Result<AnalyzeResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/analyze");
        let http_req = self.rest.get(url, req);
        self.rest.send(http_req).await
    }

    async fn pay(&self, req: &PayRequest) -> Result<Payment, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/pay");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn create_invoice(
        &self,
        req: &CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/create_invoice");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn pay_invoice(
        &self,
        req: &PayInvoiceRequest,
    ) -> Result<Payment, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/pay_invoice");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn create_offer(
        &self,
        req: &CreateOfferRequest,
    ) -> Result<CreateOfferResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/create_offer");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn pay_offer(
        &self,
        req: &PayOfferRequest,
    ) -> Result<Payment, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/pay_offer");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn pay_lnurl(
        &self,
        req: &PayLnurlRequest,
    ) -> Result<Payment, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/pay_lnurl");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    async fn withdraw_lnurl(
        &self,
        req: &WithdrawLnurlRequest,
    ) -> Result<Payment, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/withdraw_lnurl");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }

    /// NOTE: The v2 server returns [`Payment`] directly (see server handler
    /// for rationale), using HTTP 404 for not-found. The Rust client wraps this
    /// back into [`GetPaymentResponse`] so that the `Option` is enforced by
    /// the type system, guaranteeing callers handle the not-found case.
    async fn get_payment(
        &self,
        req: &GetPaymentRequest,
    ) -> Result<GetPaymentResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/payment");
        let http_req = self.rest.get(url, req);

        self.rest
            .send::<Payment, SdkApiError>(http_req)
            .await
            .map(|payment| GetPaymentResponse {
                payment: Some(payment),
            })
            .or_else(|error| match error.kind {
                SdkErrorKind::NotFound =>
                    Ok(GetPaymentResponse { payment: None }),
                _ => Err(error),
            })
    }

    async fn get_updated_payments(
        &self,
        req: &GetUpdatedPaymentsRequest,
    ) -> Result<GetUpdatedPaymentsResponse, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/updated_payments");
        let http_req = self.rest.get(url, req);
        self.rest.send(http_req).await
    }

    async fn update_personal_note(
        &self,
        req: &UpdatePersonalNoteRequest,
    ) -> Result<Empty, SdkApiError> {
        let sidecar = &self.sidecar_url;
        let url = format!("{sidecar}/v2/node/update_personal_note");
        let http_req = self.rest.post(url, req);
        self.rest.send(http_req).await
    }
}
