//! Sidecar API definition in the style of [`lexe_api::def`].
//!
//! The Sidecar API should be documented here until we figure out how our API
//! reference docs will be generated from request and response structs.

// We don't export our traits currently so auto trait stability is not relevant.
#![allow(async_fn_in_trait)]

#[cfg(doc)]
use lexe_api::types::Empty;
use sdk_core::{def::SdkApi, SdkApiError};

use crate::api::HealthCheckResponse;

/// The API that `lexe-sidecar` exposes to the SDK user.
pub trait UserSidecarApi: SdkApi {
    /// GET /v2/health [`Empty`] -> [`HealthCheckResponse`]
    ///
    /// Check the health of the sidecar itself.
    async fn health_check(&self) -> Result<HealthCheckResponse, SdkApiError>;
}
