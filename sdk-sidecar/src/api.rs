//! Sidecar-specific SDK models.
//!
//! API types which we may reasonably expect our other SDKs to use should go in
//! [`sdk_core::types`].

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub status: Cow<'static, str>,
}
