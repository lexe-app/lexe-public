//! Sidecar API definition in the style of `lexe_api::def`.
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

use lexe_api_core::error::SdkApiError;

use crate::models::{
    SdkCreateInvoiceRequest, SdkCreateInvoiceResponse, SdkGetPaymentRequest,
    SdkGetPaymentResponse, SdkNodeInfoResponse, SdkPayInvoiceRequest,
    SdkPayInvoiceResponse,
};

/// The API exposed to SDK users.
pub trait SdkApi {
    /// Get basic information about the Lexe node.
    async fn node_info(&self) -> Result<SdkNodeInfoResponse, SdkApiError>;

    /// Create a BOLT11 invoice.
    async fn create_invoice(
        &self,
        req: &SdkCreateInvoiceRequest,
    ) -> Result<SdkCreateInvoiceResponse, SdkApiError>;

    /// Pay a BOLT11 invoice.
    async fn pay_invoice(
        &self,
        req: &SdkPayInvoiceRequest,
    ) -> Result<SdkPayInvoiceResponse, SdkApiError>;

    /// Get information about a payment by its index.
    async fn get_payment(
        &self,
        req: &SdkGetPaymentRequest,
    ) -> Result<SdkGetPaymentResponse, SdkApiError>;
}
