//! The credential response and its JSON form.

use std::{collections::BTreeSet, convert::Infallible, fmt, str::FromStr};

use lexe_common::time::TimestampMs;
use serde::{Deserialize, Serialize, Serializer};
use serde_with::DeserializeFromStr;

use crate::request::OneTimeSecret;

#[derive(Debug, thiserror::Error)]
pub enum ResponseError {
    #[error("Response is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Response must set exactly one of credential or error")]
    Malformed,
}

/// The WALLET's response, delivered as plaintext JSON or sealed in a
/// [`Blob`](crate::seal::Blob).
//
// Spec: Credential response.
///
/// This is the parsed, validated form; [`CredentialResponseWire`] is the JSON.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialResponse {
    pub result: CredentialResult,
    /// The request's `one_time_secret`, echoed back.
    pub one_time_secret: OneTimeSecret,
    /// The request's `account`, echoed back if set.
    pub account: Option<String>,
    /// The request's `metadata`, echoed back if set.
    pub metadata: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CredentialResult {
    Granted(Grant),
    Error(CredentialError),
}

/// A granted credential, echoing the request's exact grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grant {
    /// The client credential, in its base64 string form.
    pub credential: String,
    /// Exactly as requested.
    pub scopes: BTreeSet<String>,
    /// Exactly as requested.
    pub permissions: BTreeSet<String>,
    /// When the credential expires; `None` if never.
    pub expires_at: Option<TimestampMs>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialError {
    pub code: ErrorCode,
    /// A human-readable description.
    pub message: Option<String>,
}

/// Unrecognized codes parse as [`ErrorCode::Other`].
//
// Spec: Protocol fields, `error`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[derive(DeserializeFromStr)]
pub enum ErrorCode {
    UserRejected,
    Other,
}

/// The response JSON: one flat object whose success fields are set on a
/// grant and whose `error` fields are set on an error. Unrecognized fields
/// are ignored.
//
// Spec: Protocol fields and Credential fields.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialResponseWire {
    pub credential: Option<String>,
    pub error: Option<ErrorCode>,
    pub error_message: Option<String>,
    pub one_time_secret: OneTimeSecret,
    pub account: Option<String>,
    pub metadata: Option<String>,
    pub scopes: Option<BTreeSet<String>>,
    pub permissions: Option<BTreeSet<String>>,
    pub expires_at: Option<TimestampMs>,
}

impl CredentialResponse {
    /// Parse the response JSON, plaintext or freshly opened.
    pub fn from_json(json: &[u8]) -> Result<Self, ResponseError> {
        let wire = serde_json::from_slice::<CredentialResponseWire>(json)?;
        Self::try_from_wire(wire)
    }

    pub fn try_from_wire(
        wire: CredentialResponseWire,
    ) -> Result<Self, ResponseError> {
        let result = match (wire.credential, wire.error) {
            (Some(credential), None) => CredentialResult::Granted(Grant {
                credential,
                scopes: wire.scopes.unwrap_or_default(),
                permissions: wire.permissions.unwrap_or_default(),
                expires_at: wire.expires_at,
            }),
            (None, Some(code)) => CredentialResult::Error(CredentialError {
                code,
                message: wire.error_message,
            }),
            _ => return Err(ResponseError::Malformed),
        };

        Ok(Self {
            result,
            one_time_secret: wire.one_time_secret,
            account: wire.account,
            metadata: wire.metadata,
        })
    }

    /// The response JSON bytes.
    pub fn to_json(self) -> Vec<u8> {
        serde_json::to_vec(&CredentialResponseWire::from(self))
            .expect("JSON serialization is infallible")
    }
}

impl From<CredentialResponse> for CredentialResponseWire {
    fn from(response: CredentialResponse) -> Self {
        let mut wire = Self {
            credential: None,
            error: None,
            error_message: None,
            one_time_secret: response.one_time_secret,
            account: response.account,
            metadata: response.metadata,
            scopes: None,
            permissions: None,
            expires_at: None,
        };

        match response.result {
            CredentialResult::Granted(grant) => {
                wire.credential = Some(grant.credential);
                wire.scopes = Some(grant.scopes);
                wire.permissions = Some(grant.permissions);
                wire.expires_at = grant.expires_at;
            }
            CredentialResult::Error(error) => {
                wire.error = Some(error.code);
                wire.error_message = error.message;
            }
        }

        wire
    }
}

impl ErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserRejected => "user_rejected",
            Self::Other => "other",
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ErrorCode {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "user_rejected" => Self::UserRejected,
            _ => Self::Other,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// The spec's success example, `budget_*` fields included.
    const SPEC_SUCCESS: &str = r#"{
        "credential": "<client credential>",
        "one_time_secret": "000102030405060708090a0b0c0d0e0f",
        "account": "@janedoe",
        "metadata": "<metadata echoed from the request>",
        "scopes": ["read_info", "read_payments",  "receive", "spend"],
        "permissions": ["cancel_payment"],
        "expires_at": 1821484800000,
        "budget_limit": "20",
        "budget_currency": "usd",
        "budget_period": "month",
        "budget_period_multiple": 1,
        "budget_first_reset": 1790838000000,
        "budget_utc_offset_secs": -25200
    }"#;

    const SPEC_ERROR: &str = r#"{
        "error": "user_rejected",
        "error_message": "<human-readable message>",
        "one_time_secret": "000102030405060708090a0b0c0d0e0f",
        "account": "@janedoe",
        "metadata": "<metadata echoed from the request>"
    }"#;

    #[test]
    fn spec_examples() {
        let from_json =
            |json: &str| CredentialResponse::from_json(json.as_bytes());
        let success = from_json(SPEC_SUCCESS).unwrap();
        let CredentialResult::Granted(grant) = &success.result else {
            panic!("Expected a grant");
        };
        assert_eq!(grant.credential, "<client credential>");
        assert_eq!(
            grant.scopes,
            ["read_info", "read_payments", "receive", "spend"]
                .map(String::from)
                .into()
        );
        assert_eq!(grant.permissions, ["cancel_payment".to_owned()].into());
        assert_eq!(
            grant.expires_at,
            Some(TimestampMs::from_millis(1821484800000).unwrap())
        );
        assert_eq!(success.account.as_deref(), Some("@janedoe"));

        let error = from_json(SPEC_ERROR).unwrap();
        let CredentialResult::Error(err) = &error.result else {
            panic!("Expected an error");
        };
        assert_eq!(err.code, ErrorCode::UserRejected);
        assert_eq!(err.message.as_deref(), Some("<human-readable message>"));

        // Round trip through our own serialization, and unknown codes.
        for resp in [success, error] {
            let json = String::from_utf8(resp.clone().to_json()).unwrap();
            assert_eq!(from_json(&json).unwrap(), resp);
        }
        let json = SPEC_ERROR.replace("user_rejected", "new_code");
        let CredentialResult::Error(err) = from_json(&json).unwrap().result
        else {
            panic!("Expected an error");
        };
        assert_eq!(err.code, ErrorCode::Other);

        // Exactly one of `credential` and `error` must be set.
        let secret_only =
            r#"{"one_time_secret": "000102030405060708090a0b0c0d0e0f"}"#;
        assert!(matches!(
            from_json(secret_only),
            Err(ResponseError::Malformed)
        ));
        let both = SPEC_ERROR
            .replace("\"error\":", "\"credential\": \"c\", \"error\":");
        assert!(matches!(from_json(&both), Err(ResponseError::Malformed)));
    }

    #[test]
    fn serialized_shape_is_flat() {
        let resp = CredentialResponse {
            result: CredentialResult::Error(CredentialError {
                code: ErrorCode::Other,
                message: None,
            }),
            one_time_secret: "000102030405060708090a0b0c0d0e0f"
                .parse()
                .unwrap(),
            account: None,
            metadata: None,
        };
        assert_eq!(
            String::from_utf8(resp.to_json()).unwrap(),
            r#"{"error":"other","one_time_secret":"000102030405060708090a0b0c0d0e0f"}"#
        );
    }
}
