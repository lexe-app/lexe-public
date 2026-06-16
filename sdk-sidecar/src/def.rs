//! Sidecar API definition in the style of [`lexe_api::def`].
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]
#![deny(missing_docs)]

use lexe::types::{
    command::{
        AnalyzeRequest, CreateInvoiceRequest, CreateInvoiceResponse,
        CreateOfferRequest, CreateOfferResponse, GetPaymentRequest,
        GetPaymentResponse, GetUpdatedPaymentsRequest,
        GetUpdatedPaymentsResponse, ListPaymentsResponse, NodeInfo,
        PayInvoiceRequest, PayOfferRequest, PaymentSyncSummary,
    },
    payment::Payment,
};
use lexe_api::{error::SdkApiError, types::Empty};

use crate::api::{
    AnalyzeResponse, HealthCheckResponse, ListPaymentsRequest, PayLnurlRequest,
    PayRequest, SignupRequest, WithdrawLnurlRequest,
};

/// The API that `lexe-sidecar` exposes to the SDK user.
pub trait UserSidecarApi {
    /// GET /v2/health [`Empty`] -> [`HealthCheckResponse`]
    ///
    /// Check the health of the sidecar itself.
    async fn health_check(&self) -> Result<HealthCheckResponse, SdkApiError>;

    /// PUT /v2/node/sync_payments [`Empty`] -> [`PaymentSyncSummary`]
    ///
    /// Sync the local payment cache with the latest payment data from the
    /// node, fetching all payments which are new or have been updated since
    /// the last sync. Returns a summary of how many were added or updated.
    async fn sync_payments(&self) -> Result<PaymentSyncSummary, SdkApiError>;

    /// GET /v2/node/list_payments [`ListPaymentsRequest`]
    ///                         -> [`ListPaymentsResponse`]
    ///
    /// List payments from the local cache, filtered by status and returned in
    /// the requested order with cursor-based pagination. Call [`sync_payments`]
    /// first to ensure the cache reflects the latest data from the node.
    ///
    /// [`sync_payments`]: Self::sync_payments
    async fn list_payments(
        &self,
        req: &ListPaymentsRequest,
    ) -> Result<ListPaymentsResponse, SdkApiError>;

    /// POST /v2/node/clear_payments [`Empty`] -> [`Empty`]
    ///
    /// Clear all locally cached payment data. Remote data on the node is not
    /// affected; call [`sync_payments`] to re-populate the cache.
    ///
    /// [`sync_payments`]: Self::sync_payments
    async fn clear_payments(&self) -> Result<Empty, SdkApiError>;

    /// PUT /v2/node/signup [`SignupRequest`] -> [`Empty`]
    ///
    /// Register with Lexe and perform initial provisioning using a root seed.
    async fn signup(&self, req: &SignupRequest) -> Result<Empty, SdkApiError>;

    /// PUT /v2/node/provision [`Empty`] -> [`Empty`]
    ///
    /// Ensure the wallet is provisioned to all recent trusted releases.
    async fn provision(&self) -> Result<Empty, SdkApiError>;

    /// GET /v2/node/node_info [`Empty`] -> [`NodeInfo`]
    ///
    /// Get basic information about the Lexe node.
    async fn node_info(&self) -> Result<NodeInfo, SdkApiError>;

    /// GET /v2/node/analyze [`AnalyzeRequest`] -> [`AnalyzeResponse`]
    ///
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

    /// POST /v2/node/pay [`PayRequest`] -> [`Payment`]
    ///
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
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed). Exception: onchain sends return immediately.
    async fn pay(&self, req: &PayRequest) -> Result<Payment, SdkApiError>;

    /// POST /v2/node/create_invoice [`CreateInvoiceRequest`]
    ///                           -> [`CreateInvoiceResponse`]
    ///
    /// Create a BOLT11 invoice.
    async fn create_invoice(
        &self,
        req: &CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, SdkApiError>;

    /// POST /v2/node/pay_invoice [`PayInvoiceRequest`] -> [`Payment`]
    ///
    /// Pay a BOLT11 invoice.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed).
    async fn pay_invoice(
        &self,
        req: &PayInvoiceRequest,
    ) -> Result<Payment, SdkApiError>;

    /// POST /v2/node/create_offer [`CreateOfferRequest`]
    ///                         -> [`CreateOfferResponse`]
    ///
    /// Create a BOLT 12 offer to receive Lightning payments.
    async fn create_offer(
        &self,
        req: &CreateOfferRequest,
    ) -> Result<CreateOfferResponse, SdkApiError>;

    /// POST /v2/node/pay_offer [`PayOfferRequest`] -> [`Payment`]
    ///
    /// Pay a BOLT 12 offer over Lightning.
    ///
    /// Returns the resulting [`Payment`] once it reaches a terminal state
    /// (completed or failed).
    async fn pay_offer(
        &self,
        req: &PayOfferRequest,
    ) -> Result<Payment, SdkApiError>;

    /// POST /v2/node/pay_lnurl [`PayLnurlRequest`] -> [`Payment`]
    ///
    /// Pay to a Lightning address or LNURL-pay endpoint.
    async fn pay_lnurl(
        &self,
        req: &PayLnurlRequest,
    ) -> Result<Payment, SdkApiError>;

    /// POST /v2/node/withdraw_lnurl [`WithdrawLnurlRequest`] -> [`Payment`]
    ///
    /// Withdraw from an LNURL-withdraw endpoint.
    async fn withdraw_lnurl(
        &self,
        req: &WithdrawLnurlRequest,
    ) -> Result<Payment, SdkApiError>;

    /// GET /v2/node/payment [`GetPaymentRequest`] -> [`GetPaymentResponse`]
    ///
    /// Get information about a payment by its index.
    async fn get_payment(
        &self,
        req: &GetPaymentRequest,
    ) -> Result<GetPaymentResponse, SdkApiError>;

    /// GET /v2/node/updated_payments [`GetUpdatedPaymentsRequest`]
    ///                            -> [`GetUpdatedPaymentsResponse`]
    ///
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
