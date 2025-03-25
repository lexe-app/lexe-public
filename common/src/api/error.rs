//! Serializable api error types and error kinds returned by various lexe
//! services.

// Deny suspicious match names that are probably non-existent variants.
#![deny(non_snake_case)]

use std::{error::Error, fmt};

use anyhow::anyhow;
use axum::response::IntoResponse;
use http::status::StatusCode;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    auth,
    user::{NodePk, UserPk},
};
#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{api::server, enclave::Measurement};

// Associated constants can't be imported.
pub const CLIENT_400_BAD_REQUEST: StatusCode = StatusCode::BAD_REQUEST;
pub const CLIENT_401_UNAUTHORIZED: StatusCode = StatusCode::UNAUTHORIZED;
pub const CLIENT_404_NOT_FOUND: StatusCode = StatusCode::NOT_FOUND;
pub const CLIENT_409_CONFLICT: StatusCode = StatusCode::CONFLICT;
pub const SERVER_500_INTERNAL_SERVER_ERROR: StatusCode =
    StatusCode::INTERNAL_SERVER_ERROR;
pub const SERVER_502_BAD_GATEWAY: StatusCode = StatusCode::BAD_GATEWAY;
pub const SERVER_503_SERVICE_UNAVAILABLE: StatusCode =
    StatusCode::SERVICE_UNAVAILABLE;
pub const SERVER_504_GATEWAY_TIMEOUT: StatusCode = StatusCode::GATEWAY_TIMEOUT;

/// `ErrorCode` is the common serialized representation for all `ErrorKind`s.
pub type ErrorCode = u16;

/// `ErrorResponse` is the common JSON-serialized representation for all
/// `ApiError`s. It is the only error struct actually sent across the wire.
/// Everything else is converted to / from it.
///
/// For displaying the full human-readable message to the user, convert
/// `ErrorResponse` to the corresponding API error type first.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct ErrorResponse {
    pub code: ErrorCode,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub msg: String,
}

/// A 'trait alias' defining all the supertraits an API error type must impl
/// to be accepted for use in the `RestClient` and across all Lexe APIs.
pub trait ApiError:
    ToHttpStatus
    + From<CommonApiError>
    + From<ErrorResponse>
    + Into<ErrorResponse>
    + Error
    + Clone
{
}

impl<E> ApiError for E where
    E: ToHttpStatus
        + From<CommonApiError>
        + From<ErrorResponse>
        + Into<ErrorResponse>
        + Error
        + Clone
{
}

/// `ApiErrorKind` defines the methods required of all API error kinds.
/// Implementations of this trait are derived by `api_error_kind!`.
///
/// Try to keep this light, since debugging macros is a pain : )
pub trait ApiErrorKind:
    Copy
    + Clone
    + Default
    + Eq
    + PartialEq
    + fmt::Debug
    + fmt::Display
    + ToHttpStatus
    + From<CommonErrorKind>
    + From<ErrorCode>
    + Sized
    + 'static
{
    /// An array of all known error kind variants, excluding `Unknown(_)`.
    const KINDS: &'static [Self];

    /// Returns `true` if the error kind is unrecognized (at least by this
    /// version of the software).
    fn is_unknown(&self) -> bool;

    /// Returns the variant name of this error kind.
    ///
    /// Ex: `MyErrorKind::Foo.to_name() == "Foo"`
    fn to_name(self) -> &'static str;

    /// Returns the human-readable message for this error kind. For a generated
    /// error kind, this is the same as the variant's doc string.
    fn to_msg(self) -> &'static str;

    /// Returns the serializable [`ErrorCode`] for this error kind.
    fn to_code(self) -> ErrorCode;

    /// Returns the error kind for this raw [`ErrorCode`].
    ///
    /// This method is infallible as every error kind must always have an
    /// `Unknown(_)` variant for backwards compatibility.
    fn from_code(code: ErrorCode) -> Self;
}

/// A trait to get the HTTP status code for a given Error.
pub trait ToHttpStatus {
    fn to_http_status(&self) -> StatusCode;
}

// --- api_error! and api_error_kind! macros --- //

// Easily debug/view the macro expansions with `cargo expand`:
//
// ```bash
// $ cargo install cargo-expand
// $ cd public/common/
// $ cargo expand api::error
// ```

/// This macro takes the name of an [`ApiError`] and its error kind type to
/// generate the various impls required by the [`ApiError`] trait alias.
///
/// This macro should be used in combination with `api_error_kind!` below.
///
/// ```ignore
/// api_error!(FooApiError, FooErrorKind);
/// ```
#[macro_export]
macro_rules! api_error {
    ($api_error:ident, $api_error_kind:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Hash, Error)]
        #[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
        pub struct $api_error {
            pub kind: $api_error_kind,
            #[cfg_attr(
                any(test, feature = "test-utils"),
                proptest(strategy = "arbitrary::any_string()")
            )]
            pub msg: String,
        }

        impl fmt::Display for $api_error {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let kind_msg = self.kind.to_msg();
                let msg = &self.msg;
                write!(f, "{kind_msg}: {msg}")
            }
        }

        impl From<ErrorResponse> for $api_error {
            fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
                let kind = $api_error_kind::from_code(code);
                Self { kind, msg }
            }
        }

        impl From<$api_error> for ErrorResponse {
            fn from($api_error { kind, msg }: $api_error) -> Self {
                let code = kind.to_code();
                Self { code, msg }
            }
        }

        impl From<CommonApiError> for $api_error {
            fn from(CommonApiError { kind, msg }: CommonApiError) -> Self {
                let kind = $api_error_kind::from(kind);
                Self { kind, msg }
            }
        }

        impl ToHttpStatus for $api_error {
            fn to_http_status(&self) -> StatusCode {
                self.kind.to_http_status()
            }
        }

        impl IntoResponse for $api_error {
            fn into_response(self) -> http::Response<axum::body::Body> {
                let status = self.to_http_status();
                let error_response = ErrorResponse::from(self);
                server::build_json_response(status, &error_response)
            }
        }
    };
}

/// This macro takes an error kind enum declaration and generates impls for the
/// trait [`ApiErrorKind`] (and its dependent traits).
///
/// Each invocation should be paired with a `ToHttpStatus` impl.
///
/// ### Example
///
/// ```ignore
/// api_error_kind! {
///     #[derive(Copy, Clone, Debug, Eq, PartialEq)]
///     pub enum FooErrorKind {
///         /// Unknown error
///         Unknown(ErrorCode),
///
///         /// A Foo error occured
///         Foo = 1,
///         /// Bar failed to complete
///         Bar = 2,
///     }
/// }
///
/// impl ToHttpStatus for FooErrorKind {
///     fn to_http_status(&self) -> StatusCode {
///         use FooErrorKind::*;
///         match self {
///             Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,
///
///             Foo => CLIENT_400_BAD_REQUEST,
///             Bar => SERVER_500_INTERNAL_SERVER_ERROR,
///         }
///     }
/// }
/// ```
///
/// * All error kind types _must_ have an `Unknown(ErrorCode)` variant and it
///   _must_ be first. This handles any unrecognized errors seen from remote
///   services and preserves the error code for debugging / propagating.
///
/// * Doc strings on the error variants are used for [`ApiErrorKind::to_msg`]
///   and the [`fmt::Display`] impl.
#[macro_export]
macro_rules! api_error_kind {
    {
        $(#[$enum_meta:meta])*
        pub enum $error_kind_name:ident {
            $( #[doc = $unknown_msg:literal] )*
            Unknown(ErrorCode),

            $(
                // use the doc string for the error message
                $( #[doc = $item_msg:literal] )*
                $item_name:ident = $item_code:literal
            ),*

            $(,)?
        }
    } => { // generate the error kind enum + impls

        $(#[$enum_meta])*
        pub enum $error_kind_name {
            $( #[doc = $unknown_msg] )*
            Unknown(ErrorCode),

            $(
                $( #[doc = $item_msg] )*
                $item_name
            ),*
        }

        // --- macro-generated impls --- //

        impl ApiErrorKind for $error_kind_name {
            const KINDS: &'static [Self] = &[
                $( Self::$item_name, )*
            ];

            #[inline]
            fn is_unknown(&self) -> bool {
                matches!(self, Self::Unknown(_))
            }

            fn to_name(self) -> &'static str {
                match self {
                    $( Self::$item_name => stringify!($item_name), )*
                    Self::Unknown(_) => "Unknown",
                }
            }

            fn to_msg(self) -> &'static str {
                let kind_msg = match self {
                    $( Self::$item_name => concat!($( $item_msg, )*), )*
                    Self::Unknown(_) => concat!($( $unknown_msg, )*),
                };
                kind_msg.trim_start()
            }

            fn to_code(self) -> ErrorCode {
                match self {
                    $( Self::$item_name => $item_code, )*
                    Self::Unknown(code) => code,
                }
            }

            fn from_code(code: ErrorCode) -> Self {
                // this deny attr makes duplicate codes a compile error : )
                #[deny(unreachable_patterns)]
                match code {
                    // make 0 the first entry so any variants with 0 code will
                    // raise a compile error.
                    0 => Self::Unknown(0),
                    $( $item_code => Self::$item_name, )*
                    _ => Self::Unknown(code),
                }
            }
        }

        // --- standard trait impls --- //

        impl Default for $error_kind_name {
            fn default() -> Self {
                Self::Unknown(0)
            }
        }

        impl fmt::Display for $error_kind_name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let msg = (*self).to_msg();

                // No ':' because the ApiError's Display impl adds it.
                //
                // NOTE: We used to prefix with `[<code>=<kind_name>]` like
                // "[106=Command]", but this was not helpful, so we removed it.
                write!(f, "{msg}")
            }
        }

        // --- impl Into/From ErrorCode --- //

        impl From<ErrorCode> for $error_kind_name {
            #[inline]
            fn from(code: ErrorCode) -> Self {
                Self::from_code(code)
            }
        }

        impl From<$error_kind_name> for ErrorCode {
            #[inline]
            fn from(val: $error_kind_name) -> ErrorCode {
                val.to_code()
            }
        }

        // --- impl From CommonErrorKind --- //

        impl From<CommonErrorKind> for $error_kind_name {
            #[inline]
            fn from(common: CommonErrorKind) -> Self {
                // We can use `Self::from_code` here bc `error_kind_invariants`
                // checks that the recovered `ApiError` kind != Unknown
                Self::from_code(common.to_code())
            }
        }

        // --- impl Arbitrary --- //

        // Unfortunately, we can't just derive Arbitrary since proptest will
        // generate `Unknown(code)` with code that actually is a valid variant.
        #[cfg(any(test, feature = "test-utils"))]
        impl proptest::arbitrary::Arbitrary for $error_kind_name {
            type Parameters = ();
            type Strategy = proptest::strategy::BoxedStrategy<Self>;

            fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
                use proptest::{prop_oneof, sample};
                use proptest::arbitrary::any;
                use proptest::strategy::Strategy;

                // 9/10 sample a valid error code, o/w sample a random error
                // code (likely unknown).
                prop_oneof![
                    9 => sample::select(Self::KINDS),
                    1 => any::<ErrorCode>().prop_map(Self::from_code),
                ].boxed()
            }
        }
    }
}

// --- Error structs --- //

/// Errors common to all [`ApiError`]s.
///
/// - This is an intermediate error type which should only be used in API
///   library code (e.g. `RestClient`, `lexe_api::server`) which cannot assume a
///   specific API error type.
/// - [`ApiError`]s and [`ApiErrorKind`]s must impl `From<CommonApiError>` and
///   `From<CommonErrorKind>` respectively to ensure all cases are covered.
pub struct CommonApiError {
    pub kind: CommonErrorKind,
    pub msg: String,
}

api_error!(BackendApiError, BackendErrorKind);
api_error!(GatewayApiError, GatewayErrorKind);
api_error!(LspApiError, LspErrorKind);
api_error!(NodeApiError, NodeErrorKind);
api_error!(RunnerApiError, RunnerErrorKind);

// --- Error variants --- //

/// Error variants common to all `ApiError`s.
#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum CommonErrorKind {
    /// Unknown Reqwest client error
    UnknownReqwest = 1,
    /// Error building the HTTP request
    Building = 2,
    /// Error connecting to a remote HTTP service
    Connect = 3,
    /// Request timed out
    Timeout = 4,
    /// Error decoding/deserializing the HTTP response body
    Decode = 5,
    /// General server error
    Server = 6,
    /// Client provided a bad request that the server rejected
    Rejection = 7,
    /// Server is currently at capacity; retry later
    AtCapacity = 8,
    // NOTE: If adding a variant, be sure to also update Self::KINDS!
}

impl ToHttpStatus for CommonErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use CommonErrorKind::*;
        match self {
            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,
        }
    }
}

api_error_kind! {
    /// All variants of errors that the backend can return.
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub enum BackendErrorKind {
        /// Unknown error
        Unknown(ErrorCode),

        // --- Common --- //

        /// Unknown Reqwest client error
        UnknownReqwest = 1,
        /// Error building the HTTP request
        Building = 2,
        /// Error connecting to a remote HTTP service
        Connect = 3,
        /// Request timed out
        Timeout = 4,
        /// Error decoding/deserializing the HTTP response body
        Decode = 5,
        /// General server error
        Server = 6,
        /// Client provided a bad request that the server rejected
        Rejection = 7,
        /// Server is at capacity
        AtCapacity = 8,

        // --- Backend --- //

        /// Database error
        Database = 100,
        /// Resource not found
        NotFound = 101,
        /// Resource was duplicate
        Duplicate = 102,
        /// Could not convert field or model to type
        Conversion = 103,
        /// User failed authentication
        Unauthenticated = 104,
        /// User not authorized
        Unauthorized = 105,
        /// Auth token or auth request is expired
        AuthExpired = 106,
        /// Parsed request is invalid
        InvalidParsedRequest = 107,
        /// Request batch size is over the limit
        BatchSizeOverLimit = 108,
    }
}

impl ToHttpStatus for BackendErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use BackendErrorKind::*;
        match self {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,

            Database => SERVER_500_INTERNAL_SERVER_ERROR,
            NotFound => CLIENT_404_NOT_FOUND,
            Duplicate => CLIENT_409_CONFLICT,
            Conversion => SERVER_500_INTERNAL_SERVER_ERROR,
            Unauthenticated => CLIENT_401_UNAUTHORIZED,
            Unauthorized => CLIENT_401_UNAUTHORIZED,
            AuthExpired => CLIENT_401_UNAUTHORIZED,
            InvalidParsedRequest => CLIENT_400_BAD_REQUEST,
            BatchSizeOverLimit => CLIENT_400_BAD_REQUEST,
        }
    }
}

api_error_kind! {
    /// All variants of errors that the gateway can return.
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub enum GatewayErrorKind {
        /// Unknown error
        Unknown(ErrorCode),

        // --- Common --- //

        /// Unknown Reqwest client error
        UnknownReqwest = 1,
        /// Error building the HTTP request
        Building = 2,
        /// Error connecting to a remote HTTP service
        Connect = 3,
        /// Request timed out
        Timeout = 4,
        /// Error decoding/deserializing the HTTP response body
        Decode = 5,
        /// General server error
        Server = 6,
        /// Client provided a bad request that the server rejected
        Rejection = 7,
        /// Server is at capacity
        AtCapacity = 8,

        // --- Gateway --- //

        /// Missing fiat exchange rates; issue with upstream data source
        FiatRatesMissing = 100,
    }
}

impl ToHttpStatus for GatewayErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use GatewayErrorKind::*;
        match self {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,

            FiatRatesMissing => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

api_error_kind! {
    /// All variants of errors that the LSP can return.
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub enum LspErrorKind {
        /// Unknown error
        Unknown(ErrorCode),

        // --- Common --- //

        /// Unknown Reqwest client error
        UnknownReqwest = 1,
        /// Error building the HTTP request
        Building = 2,
        /// Error connecting to a remote HTTP service
        Connect = 3,
        /// Request timed out
        Timeout = 4,
        /// Error decoding/deserializing the HTTP response body
        Decode = 5,
        /// General server error
        Server = 6,
        /// Client provided a bad request that the server rejected
        Rejection = 7,
        /// Server is at capacity
        AtCapacity = 8,

        // --- LSP --- //

        /// Error occurred during provisioning
        Provision = 100,
        /// Error occurred while fetching new scid
        Scid = 101,
        /// Error
        // NOTE: Intentionally NOT descriptive.
        // These get displayed on the app UI frequently and should be concise.
        Command = 102,
    }
}

impl ToHttpStatus for LspErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use LspErrorKind::*;
        match self {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,

            Provision => SERVER_500_INTERNAL_SERVER_ERROR,
            Scid => SERVER_500_INTERNAL_SERVER_ERROR,
            Command => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

api_error_kind! {
    /// All variants of errors that the node can return.
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub enum NodeErrorKind {
        /// Unknown error
        Unknown(ErrorCode),

        // --- Common --- //

        /// Unknown Reqwest client error
        UnknownReqwest = 1,
        /// Error building the HTTP request
        Building = 2,
        /// Error connecting to a remote HTTP service
        Connect = 3,
        /// Request timed out
        Timeout = 4,
        /// Error decoding/deserializing the HTTP response body
        Decode = 5,
        /// General server error
        Server = 6,
        /// Client provided a bad request that the server rejected
        Rejection = 7,
        /// Server is at capacity
        AtCapacity = 8,

        // --- Node --- //

        /// Wrong user pk
        WrongUserPk = 100,
        /// Given node pk doesn't match node pk derived from seed
        WrongNodePk = 101,
        /// Request measurement doesn't match current enclave measurement
        WrongMeasurement = 102,
        /// Error occurred during provisioning
        Provision = 103,
        /// Node unable to authenticate
        BadAuth = 104,
        /// Could not proxy request to node
        Proxy = 105,
        /// Error
        // NOTE: Intentionally NOT descriptive.
        // These get displayed on the app UI frequently and should be concise.
        Command = 106,
    }
}

impl ToHttpStatus for NodeErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use NodeErrorKind::*;
        match self {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,

            WrongUserPk => CLIENT_400_BAD_REQUEST,
            WrongNodePk => CLIENT_400_BAD_REQUEST,
            WrongMeasurement => CLIENT_400_BAD_REQUEST,
            Provision => SERVER_500_INTERNAL_SERVER_ERROR,
            BadAuth => CLIENT_401_UNAUTHORIZED,
            Proxy => SERVER_502_BAD_GATEWAY,
            Command => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

api_error_kind! {
    /// All variants of errors that the runner can return.
    #[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
    pub enum RunnerErrorKind {
        /// Unknown error
        Unknown(ErrorCode),

        // --- Common --- //

        /// Unknown Reqwest client error
        UnknownReqwest = 1,
        /// Error building the HTTP request
        Building = 2,
        /// Error connecting to a remote HTTP service
        Connect = 3,
        /// Request timed out
        Timeout = 4,
        /// Error decoding/deserializing the HTTP response body
        Decode = 5,
        /// General server error
        Server = 6,
        /// Client provided a bad request that the server rejected
        Rejection = 7,
        /// Server is at capacity
        AtCapacity = 8,

        // --- Runner --- //

        /// General Runner error
        Runner = 100,
        /// Caller provided an unknown or unserviceable measurement
        UnknownMeasurement = 101,
        /// Caller requested a version which is too old
        OldVersion = 102,
        /// Requested node temporarily unavailable, most likely due to a common
        /// race condition; retry the request (temporary error)
        TemporarilyUnavailable = 103,
        /// Runner service is unavailable (semi-permanent error)
        ServiceUnavailable = 104,
        /// Requested node failed to boot
        Boot = 106,
    }
}

impl ToHttpStatus for RunnerErrorKind {
    fn to_http_status(&self) -> StatusCode {
        use RunnerErrorKind::*;
        match self {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Server => SERVER_500_INTERNAL_SERVER_ERROR,
            Rejection => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_503_SERVICE_UNAVAILABLE,

            Runner => SERVER_500_INTERNAL_SERVER_ERROR,
            UnknownMeasurement => CLIENT_404_NOT_FOUND,
            OldVersion => CLIENT_400_BAD_REQUEST,
            TemporarilyUnavailable => CLIENT_409_CONFLICT,
            ServiceUnavailable => SERVER_503_SERVICE_UNAVAILABLE,
            Boot => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

// --- CommonApiError / CommonErrorKind impls --- //

impl CommonApiError {
    pub fn new(kind: CommonErrorKind, msg: String) -> Self {
        Self { kind, msg }
    }

    #[inline]
    pub fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
}

impl CommonErrorKind {
    #[cfg(any(test, feature = "test-utils"))]
    const KINDS: &'static [Self] = &[
        Self::UnknownReqwest,
        Self::Building,
        Self::Connect,
        Self::Timeout,
        Self::Decode,
        Self::Server,
        Self::Rejection,
        Self::AtCapacity,
    ];

    #[inline]
    pub fn to_code(self) -> ErrorCode {
        self as ErrorCode
    }
}

impl From<serde_json::Error> for CommonApiError {
    fn from(err: serde_json::Error) -> Self {
        let kind = CommonErrorKind::Decode;
        let msg = format!("Failed to deserialize response as json: {err:#}");
        Self { kind, msg }
    }
}

impl From<reqwest::Error> for CommonApiError {
    fn from(err: reqwest::Error) -> Self {
        // NOTE: The `reqwest::Error` `Display` impl is totally useless!!
        // We've had tons of problems with it swallowing TLS errors.
        // You have to use the `Debug` impl to get any info about the source.
        let msg = format!("{err:?}");
        // Be more granular than just returning a general reqwest::Error
        let kind = if err.is_builder() {
            CommonErrorKind::Building
        } else if err.is_connect() {
            CommonErrorKind::Connect
        } else if err.is_timeout() {
            CommonErrorKind::Timeout
        } else if err.is_decode() {
            CommonErrorKind::Decode
        } else {
            CommonErrorKind::UnknownReqwest
        };
        Self { kind, msg }
    }
}

impl From<CommonApiError> for ErrorResponse {
    fn from(CommonApiError { kind, msg }: CommonApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}

impl IntoResponse for CommonApiError {
    fn into_response(self) -> http::Response<axum::body::Body> {
        let status = self.kind.to_http_status();
        let error_response = ErrorResponse::from(self);
        server::build_json_response(status, &error_response)
    }
}

// --- ApiError impls --- //

impl BackendApiError {
    pub fn unauthorized(error: impl fmt::Display) -> Self {
        let kind = BackendErrorKind::Unauthorized;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }

    pub fn unauthenticated(error: impl fmt::Display) -> Self {
        let kind = BackendErrorKind::Unauthenticated;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }

    pub fn invalid_parsed_req(msg: impl Into<String>) -> Self {
        let kind = BackendErrorKind::InvalidParsedRequest;
        let msg = msg.into();
        Self { kind, msg }
    }

    pub fn bcs_serialize(err: bcs::Error) -> Self {
        let kind = BackendErrorKind::Building;
        let msg = format!("Failed to serialize bcs request: {err:#}");
        Self { kind, msg }
    }

    pub fn batch_size_too_large() -> Self {
        let kind = BackendErrorKind::BatchSizeOverLimit;
        let msg = kind.to_msg().to_owned();
        Self { kind, msg }
    }

    pub fn conversion(error: impl fmt::Display) -> Self {
        let kind = BackendErrorKind::Conversion;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }

    pub fn database(error: impl fmt::Display) -> Self {
        let kind = BackendErrorKind::Database;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }
}

impl From<auth::Error> for BackendApiError {
    fn from(error: auth::Error) -> Self {
        let kind = match error {
            auth::Error::ClockDrift => BackendErrorKind::AuthExpired,
            auth::Error::Expired => BackendErrorKind::AuthExpired,
            _ => BackendErrorKind::Unauthenticated,
        };
        let msg = format!("{error:#}");
        Self { kind, msg }
    }
}

impl GatewayApiError {
    pub fn fiat_rates_missing() -> Self {
        let kind = GatewayErrorKind::FiatRatesMissing;
        let msg = kind.to_string();
        Self { kind, msg }
    }
}

impl LspApiError {
    pub fn provision(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = LspErrorKind::Provision;
        Self { kind, msg }
    }

    pub fn scid(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = LspErrorKind::Scid;
        Self { kind, msg }
    }

    pub fn command(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = LspErrorKind::Command;
        Self { kind, msg }
    }

    pub fn rejection(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = LspErrorKind::Rejection;
        Self { kind, msg }
    }
}

impl NodeApiError {
    pub fn wrong_user_pk(current_pk: UserPk, given_pk: UserPk) -> Self {
        // We don't name these 'expected' and 'actual' because the meaning of
        // those terms is swapped depending on if you're the server or client.
        let msg =
            format!("Node has UserPk '{current_pk}' but received '{given_pk}'");
        let kind = NodeErrorKind::WrongUserPk;
        Self { kind, msg }
    }

    pub fn wrong_node_pk(derived_pk: NodePk, given_pk: NodePk) -> Self {
        // We don't name these 'expected' and 'actual' because the meaning of
        // those terms is swapped depending on if you're the server or client.
        let msg =
            format!("Derived NodePk '{derived_pk}' but received '{given_pk}'");
        let kind = NodeErrorKind::WrongNodePk;
        Self { kind, msg }
    }

    pub fn wrong_measurement(
        req_measurement: &Measurement,
        actual_measurement: &Measurement,
    ) -> Self {
        let kind = NodeErrorKind::WrongMeasurement;
        let msg =
            format!("Req: {req_measurement}, Actual: {actual_measurement}");
        Self { kind, msg }
    }

    pub fn proxy(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = NodeErrorKind::Proxy;
        Self { kind, msg }
    }

    pub fn provision(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = NodeErrorKind::Provision;
        Self { kind, msg }
    }

    pub fn command(error: impl fmt::Display) -> Self {
        let msg = format!("{error:#}");
        let kind = NodeErrorKind::Command;
        Self { kind, msg }
    }
}

impl RunnerApiError {
    pub fn at_capacity(error: impl fmt::Display) -> Self {
        let kind = RunnerErrorKind::AtCapacity;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }

    pub fn temporarily_unavailable(error: impl fmt::Display) -> Self {
        let kind = RunnerErrorKind::TemporarilyUnavailable;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }

    pub fn service_unavailable(error: impl fmt::Display) -> Self {
        let kind = RunnerErrorKind::ServiceUnavailable;
        let msg = format!("{error:#}");
        Self { kind, msg }
    }
}

// --- Misc error utilities --- //

/// Converts a [`Vec<anyhow::Result<()>>`] to an [`anyhow::Result<()>`],
/// with any error messages joined by a semicolon.
pub fn join_results(results: Vec<anyhow::Result<()>>) -> anyhow::Result<()> {
    let errors = results
        .into_iter()
        .filter_map(|res| match res {
            Ok(_) => None,
            Err(e) => Some(format!("{e:#}")),
        })
        .collect::<Vec<String>>();
    if errors.is_empty() {
        Ok(())
    } else {
        let joined_errs = errors.join("; ");
        Err(anyhow!("{joined_errs}"))
    }
}

// --- Test utils for asserting error invariants --- //

#[cfg(any(test, feature = "test-utils"))]
pub mod invariants {
    use proptest::{
        arbitrary::{any, Arbitrary},
        prop_assert, prop_assert_eq, proptest,
    };

    use super::*;

    pub fn assert_error_kind_invariants<K>()
    where
        K: ApiErrorKind + Arbitrary,
    {
        // error code 0 and default error code must be unknown
        assert!(K::from_code(0).is_unknown());
        assert!(K::default().is_unknown());

        // CommonErrorKind is a strict subset of ApiErrorKind
        //
        // CommonErrorKind [ _, 1, 2, 3, 4, 5, 6 ]
        //    ApiErrorKind [ _, 1, 2, 3, 4, 5,   , 100, 101 ]
        //                                     ^
        //                                    BAD
        for common_kind in CommonErrorKind::KINDS {
            let common_code = common_kind.to_code();
            let common_status = common_kind.to_http_status();
            let api_kind = K::from_code(common_kind.to_code());
            let api_code = api_kind.to_code();
            let api_status = api_kind.to_http_status();
            assert_eq!(common_code, api_code, "Error codes must match");
            assert_eq!(common_status, api_status, "HTTP statuses must match");

            if api_kind.is_unknown() {
                panic!(
                    "all CommonErrorKind's should be covered; \
                     missing common code: {common_code}, \
                     common kind: {common_kind:?}",
                );
            }
        }

        // error kind enum isomorphic to error code representation
        // kind -> code -> kind2 -> code2
        for kind in K::KINDS {
            let code = kind.to_code();
            let kind2 = K::from_code(code);
            let code2 = kind2.to_code();
            assert_eq!(code, code2);
            assert_eq!(kind, &kind2);
        }

        // try the first 200 error codes to ensure isomorphic
        // code -> kind -> code2 -> kind2
        for code in 0_u16..200 {
            let kind = K::from_code(code);
            let code2 = kind.to_code();
            let kind2 = K::from_code(code2);
            assert_eq!(code, code2);
            assert_eq!(kind, kind2);
        }

        // ensure proptest generator is also well-behaved
        proptest!(|(kind in any::<K>())| {
            let code = kind.to_code();
            let kind2 = K::from_code(code);
            let code2 = kind2.to_code();
            prop_assert_eq!(code, code2);
            prop_assert_eq!(kind, kind2);
        });

        // - Ensure the error kind message is non-empty, otherwise the error is
        //   displayed like ": Here's my extra info" (with leading ": ")
        // - Ensure the error kind message doesn't end with '.', otherwise the
        //   error is displayed like "Service is at capacity.: Extra info"
        proptest!(|(kind in any::<K>())| {
            prop_assert!(!kind.to_msg().is_empty());
            prop_assert!(!kind.to_msg().ends_with('.'));
        });
    }

    pub fn assert_api_error_invariants<E, K>()
    where
        E: ApiError + Arbitrary + PartialEq,
        K: ApiErrorKind + Arbitrary,
    {
        // Double roundtrip proptest
        // - ApiError -> ErrorResponse -> ApiError
        // - ErrorResponse -> ApiError -> ErrorResponse
        // i.e. The errors should be equal in serialized & unserialized form.
        proptest!(|(e1 in any::<E>())| {
            let err_resp1 = Into::<ErrorResponse>::into(e1.clone());
            let e2 = E::from(err_resp1.clone());
            let err_resp2 = Into::<ErrorResponse>::into(e2.clone());
            prop_assert_eq!(e1, e2);
            prop_assert_eq!(err_resp1, err_resp2);
        });

        // Check that the ApiError Display impl is of form
        // `<kind_msg>: <main_msg>`
        //
        // NOTE: We used to prefix with `[<code>=<kind_name>]` like
        // "[106=Command]", but this was not helpful, so we removed it.
        proptest!(|(
            kind in any::<K>(),
            main_msg in arbitrary::any_string()
        )| {
            let code = kind.to_code();
            let msg = main_msg.clone();
            let err_resp = ErrorResponse { code, msg };
            let api_error = E::from(err_resp);
            let kind_msg = kind.to_msg();

            let actual_display = format!("{api_error}");
            let expected_display =
                format!("{kind_msg}: {main_msg}");
            prop_assert_eq!(actual_display, expected_display);
        });
    }
}

// --- Tests --- //

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn error_response_serde_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<ErrorResponse>();
    }

    #[test]
    fn client_error_kinds_non_zero() {
        for kind in CommonErrorKind::KINDS {
            assert_ne!(kind.to_code(), 0);
        }
    }

    #[test]
    fn error_kind_invariants() {
        invariants::assert_error_kind_invariants::<BackendErrorKind>();
        invariants::assert_error_kind_invariants::<GatewayErrorKind>();
        invariants::assert_error_kind_invariants::<LspErrorKind>();
        invariants::assert_error_kind_invariants::<NodeErrorKind>();
        invariants::assert_error_kind_invariants::<RunnerErrorKind>();
    }

    #[test]
    fn api_error_invariants() {
        use invariants::assert_api_error_invariants;
        assert_api_error_invariants::<BackendApiError, BackendErrorKind>();
        assert_api_error_invariants::<GatewayApiError, GatewayErrorKind>();
        assert_api_error_invariants::<LspApiError, LspErrorKind>();
        assert_api_error_invariants::<NodeApiError, NodeErrorKind>();
        assert_api_error_invariants::<RunnerApiError, RunnerErrorKind>();
    }

    #[test]
    fn node_lsp_command_error_is_concise() {
        let err1 = format!("{:#}", NodeApiError::command("Oops!"));
        let err2 = format!("{:#}", LspApiError::command("Oops!"));

        assert_eq!(err1, "Error: Oops!");
        assert_eq!(err2, "Error: Oops!");
    }
}
