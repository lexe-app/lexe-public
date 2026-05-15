//! Sidecar-specific SDK models.
//!
//! API types which we may reasonably expect our other SDKs to use should go in
//! [`lexe::types`].

use std::borrow::Cow;

use anyhow::ensure;
use lexe::types::bitcoin::Amount;
use lexe_common::time::TimestampMs;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct HealthCheckResponse {
    pub status: Cow<'static, str>,
}

#[derive(Serialize, Deserialize)]
pub struct AnalyzeResponse {
    pub payables: Vec<PayableDetails>,
}

/// Mirrors [`lexe::types::command::PayableDetails`],
/// but instead includes a sidecar-specific callback,
/// a `kind` field to indicate the method, and specific
/// fields for each payable type
#[derive(Serialize, Deserialize)]
pub struct PayableDetails {
    pub callback: String,

    /// Used in lieu of `method: PaymentMethod`
    pub kind: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lnurl: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onchain: Option<String>,

    pub description: Option<String>,
    pub amount: Option<Amount>,
    pub min_amount: Option<Amount>,
    pub max_amount: Option<Amount>,
    pub expires_at: Option<TimestampMs>,
}

/// Mirrors [`lexe::types::command::PayRequest`],
/// but makes `payable` field optional so that the requester
/// can pass arguments as query parameters
#[derive(Serialize, Deserialize)]
pub struct PayRequest {
    pub payable: Option<String>,
    pub amount: Option<Amount>,
    pub message: Option<String>,
    pub personal_note: Option<String>,
}

impl PayRequest {
    /// Merge two [`PayRequest`]s. Disallows duplicates, even if the values
    /// are equal.
    pub fn merge_no_dups(self, other: Self) -> anyhow::Result<Self> {
        let Self {
            payable,
            amount,
            message,
            personal_note,
        } = self;

        let err_msg_with = |field| format!("Found duplicate '{field}' field.");
        ensure!(
            !(payable.is_some() && other.payable.is_some()),
            err_msg_with("payable")
        );
        ensure!(
            !(amount.is_some() && other.amount.is_some()),
            err_msg_with("amount")
        );
        ensure!(
            !(message.is_some() && other.message.is_some()),
            err_msg_with("message")
        );
        ensure!(
            !(personal_note.is_some() && other.personal_note.is_some()),
            err_msg_with("personal_note")
        );

        let merged = Self {
            payable: payable.or(other.payable),
            amount: amount.or(other.amount),
            message: message.or(other.message),
            personal_note: personal_note.or(other.personal_note),
        };

        Ok(merged)
    }
}
