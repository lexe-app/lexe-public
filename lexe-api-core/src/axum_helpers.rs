use http::{HeaderValue, StatusCode, Version, header::CONTENT_TYPE};
use serde::Serialize;
use tracing::error;

use crate::error::{CommonErrorKind, ErrorResponse, ToHttpStatus};

/// The HTTP version returned in our server responses.
const HTTP_VERSION: Version = Version::HTTP_2;

/// Constructs a JSON [`http::Response<axum::body::Body>`] from the data and
/// status code. If serialization fails for some reason (very unlikely), log and
/// return a [`ErrorResponse`] containing a [`CommonErrorKind::Server`] along
/// with the associated [`StatusCode`].
// This is pub only bc it is used in the api_error! macro
pub fn build_json_response(
    status: StatusCode,
    data: &impl Serialize,
) -> http::Response<axum::body::Body> {
    /// Most of the logic goes in this monomorphic fn to prevent binary bloat.
    fn build_json_response_inner(
        status: StatusCode,
        try_json_bytes: Result<Vec<u8>, serde_json::Error>,
    ) -> http::Response<axum::body::Body> {
        let (status, json_bytes) = match try_json_bytes {
            Ok(jb) => (status, jb),
            Err(e) => {
                let msg = format!("Couldn't serialize response: {e:#}");
                error!(target: "http", "{msg}");
                let error_kind = CommonErrorKind::Server;
                let code = error_kind.to_code();
                let status = error_kind.to_http_status();
                let err_resp = ErrorResponse {
                    code,
                    msg,
                    ..Default::default()
                };
                let json_bytes = serde_json::to_vec(&err_resp)
                    .expect("Serializing ErrorResponse really shouldn't fail");
                (status, json_bytes)
            }
        };

        let bytes = bytes::Bytes::from(json_bytes);
        let http_body = http_body_util::Full::new(bytes);
        let axum_body = axum::body::Body::new(http_body);

        default_response_builder()
            .header(
                CONTENT_TYPE,
                // Can do `HeaderValue::from_static(mime::APPLICATION_JSON)`
                // if we ever have a non-trivial need for the `mime` crate
                HeaderValue::from_static("application/json"),
            )
            .status(status)
            .body(axum_body)
            .expect("All operations here should be infallible")
    }

    build_json_response_inner(status, serde_json::to_vec(data))
}

/// A builder for a [`http::Response`] with Lexe's defaults set.
pub fn default_response_builder() -> http::response::Builder {
    http::Response::builder().version(HTTP_VERSION)
}
