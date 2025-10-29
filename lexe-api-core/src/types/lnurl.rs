//! Types related to LUD-06 (LNURL-pay).
use anyhow::Context;
use common::{ByteArray, ln::amount::Amount};
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
    pub max_sendable: u64,
    /// Min millisatoshi amount willing to receive.
    pub min_sendable: u64,
    /// Metadata json as raw string (required for signature verification).
    pub metadata: String,
    /// Type of LNURL (always "payRequest").
    pub tag: String,
}

impl From<LnurlPayRequest> for LnurlPayRequestWire {
    fn from(value: LnurlPayRequest) -> Self {
        Self {
            callback: value.callback,
            max_sendable: value.max_sendable.msat(),
            min_sendable: value.min_sendable.msat(),
            metadata: value.metadata.raw,
            tag: "payRequest".to_owned(),
        }
    }
}

impl From<LnurlPayRequestWire> for LnurlPayRequest {
    fn from(value: LnurlPayRequestWire) -> Self {
        Self {
            callback: value.callback,
            max_sendable: Amount::from_msat(value.max_sendable),
            min_sendable: Amount::from_msat(value.min_sendable),
            metadata: LnurlPayRequestMetadata::from_raw_str(value.metadata)
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
    /// Parses LNURL-pay metadata string into structured metadata.
    ///
    /// LUD-06 `metadata` field is a JSON array encoded as a string:
    /// `"[[\"text/plain\", \"lorem ipsum blah blah\"]]"`.
    pub fn from_raw_str(raw: String) -> anyhow::Result<Self> {
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
            match ty {
                "text/plain" =>
                    description = value.as_str().map(|s| s.to_owned()),
                "text/long-desc" =>
                    long_description = value.as_str().map(|s| s.to_owned()),
                "image/png;base64" =>
                    image_png_base64 = value.as_str().map(|s| s.to_owned()),
                "image/jpeg;base64" =>
                    image_jpeg_base64 = value.as_str().map(|s| s.to_owned()),
                "text/identifier" =>
                    identifier = value.as_str().map(|s| s.to_owned()),
                "text/email" => email = value.as_str().map(|s| s.to_owned()),
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
    pub fn to_raw_str(&self) -> String {
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

/// The callback response from a LNURL-pay request (LUD-06).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LnurlPayRequestCallback {
    /// The BOLT11 invoice to pay.
    pub pr: LxInvoice,
    /// Deprecated field, always empty.
    #[serde(default)]
    pub routes: Vec<()>,
}

#[cfg(any(test, feature = "test-utils"))]
pub mod arbitrary_impl {
    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::{Arbitrary, any},
        option, prop_oneof,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;

    /// Generate a simple and valid HTTPS URLs for testing.
    fn any_https_url() -> impl Strategy<Value = String> {
        (
            arbitrary::any_string(),
            prop_oneof![Just("com"), Just("org"), Just("app"), Just("io")],
            proptest::collection::vec(arbitrary::any_string(), 0..=3),
            proptest::collection::vec(
                (arbitrary::any_string(), arbitrary::any_string()),
                0..=2,
            ),
        )
            .prop_map(|(domain, tld, paths, params)| {
                let path = paths.join("/");
                let query = if params.is_empty() {
                    String::new()
                } else {
                    format!(
                        "?{}",
                        params
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join("&")
                    )
                };

                if path.is_empty() {
                    format!("https://{}.{}", domain, tld)
                } else {
                    format!("https://{}.{}/{}{}", domain, tld, path, query)
                }
            })
    }

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
                        let raw = {
                            let mut metadata_array: Vec<(&str, &str)> =
                                Vec::new();
                            metadata_array.push(("text/plain", &description));

                            if let Some(ref long_desc) = long_description {
                                metadata_array
                                    .push(("text/long-desc", long_desc));
                            }
                            if let Some(ref png) = image_png_base64 {
                                metadata_array.push(("image/png;base64", png));
                            }
                            if let Some(ref jpeg) = image_jpeg_base64 {
                                metadata_array
                                    .push(("image/jpeg;base64", jpeg));
                            }
                            if let Some(ref id) = identifier {
                                metadata_array.push(("text/identifier", id));
                            }
                            if let Some(ref email_val) = email {
                                metadata_array.push(("text/email", email_val));
                            }

                            serde_json::to_string(&metadata_array).expect(
                                "metadata serialization should never fail",
                            )
                        };

                        let description_hash =
                            sha256::digest(raw.as_bytes()).to_array();

                        LnurlPayRequestMetadata {
                            description,
                            description_hash,
                            email,
                            identifier,
                            image_jpeg_base64,
                            image_png_base64,
                            long_description,
                            raw,
                        }
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
                any_https_url(),
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

    impl proptest::arbitrary::Arbitrary for LnurlPayRequestCallback {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<LxInvoice>()
                .prop_map(|pr| LnurlPayRequestCallback { pr, routes: vec![] })
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
        roundtrip::json_string_roundtrip_proptest::<LnurlPayRequestCallback>();
    }
}
