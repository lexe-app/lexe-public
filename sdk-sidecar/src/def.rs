//! Sidecar API definition in the style of [`common::api::def`].
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

use common::api::error::NodeApiError;
#[cfg(doc)]
use common::api::Empty;
use lexe_api::types::sdk::{
    SdkCreateInvoiceRequest, SdkCreateInvoiceResponse, SdkGetPaymentRequest,
    SdkGetPaymentResponse, SdkNodeInfo, SdkPayInvoiceRequest,
    SdkPayInvoiceResponse,
};

use crate::models::HealthCheck;

/// The API that `lexe-sidecar` exposes to the SDK user.
pub trait UserSidecarApi {
    /// GET /v1/health [`Empty`] -> [`HealthCheck`]
    ///
    /// Check the health of the sidecar itself.
    async fn health_check(&self) -> Result<HealthCheck, NodeApiError>;

    /// GET /v1/node/node_info [`Empty`] -> [`SdkNodeInfo`]
    ///
    /// Get basic information about the Lexe node.
    async fn node_info(&self) -> Result<SdkNodeInfo, NodeApiError>;

    /// POST /v1/node/create_invoice
    ///     [`SdkCreateInvoiceRequest`] -> [`SdkCreateInvoiceResponse`]
    async fn create_invoice(
        &self,
        req: &SdkCreateInvoiceRequest,
    ) -> Result<SdkCreateInvoiceResponse, NodeApiError>;

    /// POST /v1/node/pay_invoice
    ///     [`SdkPayInvoiceRequest`] -> [`SdkPayInvoiceResponse`]
    async fn pay_invoice(
        &self,
        req: &SdkPayInvoiceRequest,
    ) -> Result<SdkPayInvoiceResponse, NodeApiError>;

    /// GET /v1/node/payment
    ///     [`SdkGetPaymentRequest`] -> [`SdkGetPaymentResponse`]
    async fn get_payment(
        &self,
        req: &SdkGetPaymentRequest,
    ) -> Result<SdkGetPaymentResponse, NodeApiError>;
}
