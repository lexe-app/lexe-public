//! Sidecar API definition in the style of [`lexe_api::def`].
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]
#![deny(missing_docs)]

use lexe::types::command::{
    AnalyzeRequest, CreateInvoiceRequest, CreateInvoiceResponse,
    CreateOfferRequest, CreateOfferResponse, GetPaymentRequest,
    GetPaymentResponse, GetUpdatedPaymentsRequest, GetUpdatedPaymentsResponse,
    NodeInfo, PayInvoiceRequest, PayInvoiceResponse, PayOfferRequest,
    PayOfferResponse, PayResponse,
};
use lexe_api::error::SdkApiError;
#[cfg(doc)]
use lexe_api::types::Empty;

use crate::api::{AnalyzeResponse, HealthCheckResponse, PayRequest};

/// The API that `lexe-sidecar` exposes to the SDK user.
pub trait UserSidecarApi {
    /// GET /v2/health [`Empty`] -> [`HealthCheckResponse`]
    ///
    /// Check the health of the sidecar itself.
    async fn health_check(&self) -> Result<HealthCheckResponse, SdkApiError>;

    /// Get basic information about the Lexe node.
    async fn node_info(&self) -> Result<NodeInfo, SdkApiError>;

    /// Get information about a Bitcoin or Lightning payment string and its
    /// constituent payment methods (if any). Returned information includes the
    /// type of payment method used (invoice, offer, onchain, lnurl) and the
    /// amount constraints requested by the receiver.
    ///
    /// Also, for each payment method, get a `callback` URL pointing to the
    /// `pay` endpoint that can be used to pay the associated payment method.
    ///
    /// If `amount` is `null`, an amount must be supplied before calling the
    /// callback - either by appending `&amount=<amount>` as a query parameter
    /// or by providing it in the JSON body of the `pay` request.
    async fn analyze(
        &self,
        req: &AnalyzeRequest,
    ) -> Result<AnalyzeResponse, SdkApiError>;

    /// Pay any string which encodes a Bitcoin or Lightning payment method.
    ///
    /// If there exist multiple encoded payment methods, one best recommended
    /// payment method will be chosen.
    ///
    /// Arguments can be given as either query parameters or as fields
    /// within the JSON body, but requests with duplicate fields will be
    /// rejected.
    ///
    /// For finer control over how to pay, consider first using the `analyze`
    /// endpoint to resolve the contents of the payable string. From there,
    /// one can either use the callback or invoke the specific pay endpoint
    /// for the payment method of choice: `pay_offer`, `pay_invoice`, etc.
    async fn pay(&self, req: &PayRequest) -> Result<PayResponse, SdkApiError>;

    /// Create a BOLT11 invoice.
    async fn create_invoice(
        &self,
        req: &CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, SdkApiError>;

    /// Pay a BOLT11 invoice.
    async fn pay_invoice(
        &self,
        req: &PayInvoiceRequest,
    ) -> Result<PayInvoiceResponse, SdkApiError>;

    /// Create a BOLT 12 offer to receive Lightning payments.
    async fn create_offer(
        &self,
        req: &CreateOfferRequest,
    ) -> Result<CreateOfferResponse, SdkApiError>;

    /// Pay a BOLT 12 offer over Lightning.
    async fn pay_offer(
        &self,
        req: &PayOfferRequest,
    ) -> Result<PayOfferResponse, SdkApiError>;

    /// Get information about a payment by its index.
    async fn get_payment(
        &self,
        req: &GetPaymentRequest,
    ) -> Result<GetPaymentResponse, SdkApiError>;

    /// Get a batch of payments in ascending `updated_at` order, starting
    /// from a given `updated_at` index.
    ///
    /// `start_index` is the cursor at which the results should start,
    /// exclusive. If `None`, the least recently updated payments will be
    /// returned first. `limit` caps the number of payments returned
    /// (max 100, default 50).
    async fn get_updated_payments(
        &self,
        req: &GetUpdatedPaymentsRequest,
    ) -> Result<GetUpdatedPaymentsResponse, SdkApiError>;
}
