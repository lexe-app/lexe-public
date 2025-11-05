//! Types related to LUD-06 (LNURL-pay).

use std::fmt;

use anyhow::Context;
use common::{ByteArray, ln::amount::Amount};
use http::StatusCode;
use serde::{Deserialize, Serialize};

use super::invoice::LxInvoice;

/// The validated and parsed LNURL-pay request ("payRequest").
///
/// This is the internal representation used throughout the codebase.
/// For LUD-06 wire format serialization, use [`LnurlPayRequestWire`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnurlPayRequest {
    /// Callback URL to request invoice from.
    pub callback: String,
    /// Minimum sendable amount.
    pub min_sendable: Amount,
    /// Maximum sendable amount.
    pub max_sendable: Amount,
    /// Parsed metadata with description and description hash.
    pub metadata: LnurlPayRequestMetadata,
}

/// The QueryString parameters internally required in lnurl-pay callbacks.
#[derive(Serialize, Deserialize)]
pub struct LnurlCallbackRequest {
    pub username: String,
    /// The amount in millisats. We can't use [`Amount`] here as we don't
    /// control this API definition.
    #[serde(rename = "amount")]
    pub amount_msat: u64,
}

#[cfg(feature = "axum")]
#[axum::async_trait]
impl<S: Send + Sync> axum::extract::FromRequestParts<S>
    for LnurlCallbackRequest
{
    type Rejection = LnurlError;

    // LUD-06 defines an error message differently than Lexe-style
    // rejection errors. Then, we build a custom rejection `[LnurlError]`
    // which is then converted to a JSON response.
    //
    // We disable the clippy lint as we want to use `[axum::extract::Query]`
    #[allow(clippy::disallowed_types)]
    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        axum::extract::Query::from_request_parts(parts, state)
            .await
            .map(|axum::extract::Query(req)| req)
            .map_err(|e| LnurlError {
                reason: format!("{e}"),
                status_code: StatusCode::BAD_REQUEST,
            })
    }
}

/// The callback response from a LNURL-pay request (LUD-06).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LnurlCallbackResponse {
    /// The BOLT11 invoice to pay.
    pub pr: LxInvoice,
    // The LUD-06 spec mandates a `routes` field (always empty array).
    // Modern implementations (Breez SDK, Phoenix) ignore it entirely.
    // It was likely intended for source routing hints but became
    // redundant since BOLT11 invoices already contain route hints.
    #[serde(default)]
    pub routes: Vec<()>,
}

/// Error response for lnurl payment requests and callbacks.
pub struct LnurlError {
    /// The error message to be diplayed to the payer.
    pub reason: String,
    // TODO(maurice): We are assuming that clients will understand
    // the error codes and also parse the Json error response.
    // We should revisit this on production as other services are
    // returning code 200 with the error message.
    pub status_code: StatusCode,
}

impl LnurlError {
    /// Constructs a [`LnurlError`] with a [`StatusCode::BAD_REQUEST`].
    pub fn bad_request(reason: impl fmt::Display) -> Self {
        Self {
            reason: format!("{reason:#}"),
            status_code: StatusCode::BAD_REQUEST,
        }
    }

    /// Constructs a [`LnurlError`] with a
    /// [`StatusCode::INTERNAL_SERVER_ERROR`].
    /// NOTE: This is used as the default error code for all errors that occur
    /// in the `lnurl` module in order to avoid leaking internal information
    /// to external clients.
    pub fn server_error() -> Self {
        Self {
            reason: "Could not get user information".to_owned(),
            status_code: StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[cfg(feature = "axum")]
impl axum::response::IntoResponse for LnurlError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status_code;
        let error_response = LnurlErrorWire::from(self);
        crate::axum_helpers::build_json_response(status, &error_response)
    }
}

/// Serialized error response for lnurl payment requests and callbacks.
#[derive(Serialize, Deserialize)]
pub struct LnurlErrorWire {
    status: Status,
    pub reason: String,
}

/// An LNURL `status` field.
#[derive(Serialize, Deserialize)]
pub enum Status {
    #[serde(rename = "ERROR")]
    Error,
}

impl From<LnurlError> for LnurlErrorWire {
    fn from(e: LnurlError) -> Self {
        Self {
            status: Status::Error,
            reason: e.reason,
        }
    }
}
/// LUD-06 wire format for LNURL-pay request.
///
/// This matches the exact JSON format specified in LUD-06:
/// - camelCase field names
/// - amounts in millisatoshi (integers)
/// - metadata as raw JSON string
/// - includes the "tag" field
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LnurlPayRequestWire {
    /// The URL which will accept the pay request parameters.
    pub callback: String,
    /// Max millisatoshi amount willing to receive.
    #[serde(rename = "maxSendable")]
    pub max_sendable_msat: u64,
    /// Min millisatoshi amount willing to receive.
    #[serde(rename = "minSendable")]
    pub min_sendable_msat: u64,
    /// Metadata json as raw string (required for signature verification).
    pub metadata: String,
    /// Type of LNURL (always "payRequest").
    pub tag: String,
}

impl From<LnurlPayRequest> for LnurlPayRequestWire {
    fn from(value: LnurlPayRequest) -> Self {
        Self {
            callback: value.callback,
            max_sendable_msat: value.max_sendable.msat(),
            min_sendable_msat: value.min_sendable.msat(),
            metadata: value.metadata.raw,
            tag: "payRequest".to_owned(),
        }
    }
}

impl From<LnurlPayRequestWire> for LnurlPayRequest {
    fn from(value: LnurlPayRequestWire) -> Self {
        Self {
            callback: value.callback,
            max_sendable: Amount::from_msat(value.max_sendable_msat),
            min_sendable: Amount::from_msat(value.min_sendable_msat),
            metadata: LnurlPayRequestMetadata::from_raw_string(value.metadata)
                .expect("LnurlPayRequestWire should contain valid metadata"),
        }
    }
}

/// The metadata inside a [`LnurlPayRequest`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LnurlPayRequestMetadata {
    /// Short description from `text/plain` (required, LUD-06).
    pub description: String,
    /// Long description from `text/long-desc` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub long_description: Option<String>,
    /// PNG thumbnail from `image/png;base64` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub image_png_base64: Option<String>,
    /// JPEG thumbnail from `image/jpeg;base64` (optional, LUD-06).
    /// Can be displayed to the user when prompting the user for an amount.
    pub image_jpeg_base64: Option<String>,
    /// Internet identifier from `text/identifier` (LUD-16).
    /// LNURL-Pay via LUD-16 requires this or `text/email` to be set.
    pub identifier: Option<String>,
    /// Email address from `text/email` (LUD-16).
    /// LNURL-Pay via LUD-16 requires this or `text/identifier` to be set.
    pub email: Option<String>,
    /// SHA256 hash of raw metadata for invoice validation.
    pub description_hash: [u8; 32],
    /// The original unparsed metadata string.
    pub raw: String,
}

impl LnurlPayRequestMetadata {
    pub fn from_email(email: &str) -> Self {
        let description = format!("Pay to {email}");
        let mut this = Self {
            description,
            email: Some(email.to_owned()),
            long_description: None,
            image_png_base64: None,
            image_jpeg_base64: None,
            identifier: None,
            description_hash: [0; 32],
            raw: String::new(),
        };
        let raw = this.to_raw_string();
        this.description_hash = sha256::digest(raw.as_bytes()).to_array();
        this.raw = raw;
        this
    }

    /// Parses LNURL-pay metadata string into structured metadata.
    ///
    /// LUD-06 `metadata` field is a JSON array encoded as a string:
    /// `"[[\"text/plain\", \"lorem ipsum blah blah\"]]"`.
    pub fn from_raw_string(raw: String) -> anyhow::Result<Self> {
        let description_hash = sha256::digest(raw.as_bytes()).to_array();

        // LUD-06: "The `metadata` json array is only allowed to contain
        // arrays. The first item of an array inside the `metadata` array is
        // always a string representing the metadata type while any item
        // that follows can be of any JSON type. Implementors MUST NOT
        // assume it will always be a string."
        let metadata_array =
            serde_json::from_str::<Vec<(&str, serde_json::Value)>>(&raw)
                .context("LNURL-pay metadata is not in correct format")?;

        let mut description = None;
        let mut long_description = None;
        let mut image_png_base64 = None;
        let mut image_jpeg_base64 = None;
        let mut identifier = None;
        let mut email = None;

        for (ty, value) in metadata_array {
            let value = match value {
                serde_json::Value::String(value) => Some(value),
                _ => continue, // Ignore non-string values
            };

            match ty {
                "text/plain" => description = value,
                "text/long-desc" => long_description = value,
                "image/png;base64" => image_png_base64 = value,
                "image/jpeg;base64" => image_jpeg_base64 = value,
                "text/identifier" => identifier = value,
                "text/email" => email = value,
                _ => {} // Ignore unknown types
            }
        }

        let description = description.context(
            "LNURL-pay metadata is missing required 'text/plain' entry",
        )?;

        Ok(Self {
            description,
            description_hash,
            email,
            identifier,
            image_jpeg_base64,
            image_png_base64,
            long_description,
            raw,
        })
    }

    /// Serializes the metadata back into the raw LNURL-pay metadata string.
    ///
    /// Generates a JSON array encoded as a string, suitable for use in
    /// LNURL-pay requests. The order of entries is deterministic.
    pub fn to_raw_string(&self) -> String {
        let mut metadata_array: Vec<(&str, &str)> = Vec::new();

        metadata_array.push(("text/plain", &self.description));

        if let Some(ref long_desc) = self.long_description {
            metadata_array.push(("text/long-desc", long_desc));
        }
        if let Some(ref png) = self.image_png_base64 {
            metadata_array.push(("image/png;base64", png));
        }
        if let Some(ref jpeg) = self.image_jpeg_base64 {
            metadata_array.push(("image/jpeg;base64", jpeg));
        }
        if let Some(ref id) = self.identifier {
            metadata_array.push(("text/identifier", id));
        }
        if let Some(ref email) = self.email {
            metadata_array.push(("text/email", email));
        }

        serde_json::to_string(&metadata_array)
            .expect("metadata serialization should never fail")
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub mod arbitrary_impl {
    use common::test_utils::arbitrary::{self, any_string};
    use proptest::{
        arbitrary::{Arbitrary, any},
        option,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for LnurlPayRequestMetadata {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (
                arbitrary::any_simple_string(),
                option::of(arbitrary::any_simple_string()),
                option::of(arbitrary::any_simple_string()),
                option::of(arbitrary::any_simple_string()),
                option::of(arbitrary::any_simple_string()),
                option::of(arbitrary::any_simple_string()),
            )
                .prop_map(
                    |(
                        description,
                        long_description,
                        image_png_base64,
                        image_jpeg_base64,
                        identifier,
                        email,
                    )| {
                        let mut this = LnurlPayRequestMetadata {
                            description,
                            description_hash: [0; 32],
                            email,
                            identifier,
                            image_jpeg_base64,
                            image_png_base64,
                            long_description,
                            raw: "".to_owned(),
                        };
                        let raw = this.to_raw_string();
                        this.description_hash =
                            sha256::digest(raw.as_bytes()).to_array();
                        this.raw = raw;
                        this
                    },
                )
                .boxed()
        }
    }

    impl Arbitrary for LnurlPayRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (
                any_string(),
                any::<Amount>(),
                any::<Amount>(),
                any::<LnurlPayRequestMetadata>(),
            )
                .prop_map(
                    |(
                        callback,
                        mut min_sendable,
                        mut max_sendable,
                        metadata,
                    )| {
                        // Ensure min <= max
                        if min_sendable > max_sendable {
                            std::mem::swap(
                                &mut min_sendable,
                                &mut max_sendable,
                            );
                        }

                        LnurlPayRequest {
                            callback,
                            max_sendable,
                            metadata,
                            min_sendable,
                        }
                    },
                )
                .boxed()
        }
    }

    impl Arbitrary for LnurlPayRequestWire {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<LnurlPayRequest>().prop_map(|req| req.into()).boxed()
        }
    }

    impl proptest::arbitrary::Arbitrary for LnurlCallbackResponse {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<LxInvoice>()
                .prop_map(|pr| LnurlCallbackResponse { pr, routes: vec![] })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn lnurl_pay_request_wire_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LnurlPayRequestWire>();
    }

    #[test]
    fn lnurl_pay_request_callback_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LnurlCallbackResponse>();
    }
}
