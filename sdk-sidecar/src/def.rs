//! Sidecar API definition in the style of [`lexe_api::def`].
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]
#![deny(missing_docs)]

use lexe::types::command::{
    CreateInvoiceRequest, CreateInvoiceResponse, GetPaymentRequest,
    GetPaymentResponse, NodeInfo, PayInvoiceRequest, PayInvoiceResponse,
};
use lexe_api::error::SdkApiError;
#[cfg(doc)]
use lexe_api::types::Empty;

use crate::api::HealthCheckResponse;

/// The API that `lexe-sidecar` exposes to the SDK user.
pub trait UserSidecarApi {
    /// GET /v2/health [`Empty`] -> [`HealthCheckResponse`]
    ///
    /// Check the health of the sidecar itself.
    async fn health_check(&self) -> Result<HealthCheckResponse, SdkApiError>;

    /// Get basic information about the Lexe node.
    async fn node_info(&self) -> Result<NodeInfo, SdkApiError>;

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

    /// Get information about a payment by its index.
    async fn get_payment(
        &self,
        req: &GetPaymentRequest,
    ) -> Result<GetPaymentResponse, SdkApiError>;
}
