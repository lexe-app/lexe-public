//! Serializable api error types and error kinds returned by various lexe
//! services.

// Deny suspicious match names that are probably non-existent variants.
#![deny(non_snake_case)]

use std::{error::Error, fmt};

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
// So the consts fit in 80 chars
use warp::http::status::StatusCode as Status;

#[cfg(any(test, feature = "test-utils"))]
use crate::test_utils::arbitrary;
use crate::{
    api::{auth, NodePk, UserPk},
    hex,
};

// Associated constants can't be imported.
pub const CLIENT_400_BAD_REQUEST: Status = Status::BAD_REQUEST;
pub const CLIENT_401_UNAUTHORIZED: Status = Status::UNAUTHORIZED;
pub const CLIENT_404_NOT_FOUND: Status = Status::NOT_FOUND;
pub const CLIENT_409_CONFLICT: Status = Status::CONFLICT;
pub const SERVER_500_INTERNAL_SERVER_ERROR: Status =
    Status::INTERNAL_SERVER_ERROR;
pub const SERVER_502_BAD_GATEWAY: Status = Status::BAD_GATEWAY;
pub const SERVER_503_SERVICE_UNAVAILABLE: Status = Status::SERVICE_UNAVAILABLE;
pub const SERVER_504_GATEWAY_TIMEOUT: Status = Status::GATEWAY_TIMEOUT;

/// `ErrorCode` is the common serialized representation for all `ErrorKind`s.
pub type ErrorCode = u16;

/// `ErrorResponse` is the common JSON-serialized representation for all
/// `ApiError`s. It is the only error struct actually sent across the wire.
/// Everything else is converted to / from it.
///
/// For displaying the full human-readable message to the user, convert
/// `ErrorResponse` to the corresponding service error type first.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub msg: String,
}

/// Get the HTTP status code returned for a particular Error.
pub trait ToHttpStatus {
    fn to_http_status(&self) -> Status;
}

/// A 'trait alias' defining all the supertraits a service error type must impl
/// to be accepted for use in the `RestClient` and across all Lexe services.
pub trait ServiceApiError:
    From<RestClientError>
    + From<ErrorResponse>
    + Into<ErrorResponse>
    + Error
    + Clone
{
}

impl<E> ServiceApiError for E where
    E: From<RestClientError>
        + From<ErrorResponse>
        + Into<ErrorResponse>
        + Error
        + Clone
{
}

/// `ErrorKindGenerated` is the set of methods and traits derived by the
/// `error_kind!` macro.
///
/// Try to keep this light, since debugging macros is a pain : )
pub trait ErrorKindGenerated:
    Copy
    + Clone
    + Default
    + Eq
    + PartialEq
    + fmt::Debug
    + fmt::Display
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

// --- error_kind! macro --- //

// Easily debug/view the `error_kind!` macro expansion with `cargo expand`:
//
// ```bash
// $ cargo install cargo-expand
// $ cd common/
// $ cargo expand api::error
// ```

/// This macro takes an error kind enum declaration and generates impls for the
/// trait [`ErrorKindGenerated`] (and its dependent traits).
///
/// ### Example
///
/// ```ignore
/// error_kind! {
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
/// ```
///
/// * All error kind types _must_ have an `Unknown(ErrorCode)` variant and it
///   _must_ be first. This handles any unrecognized errors seen from remote
///   services and preserves the error code for debugging / propagating.
///
/// * Doc strings on the error variants are used for
///   [`ErrorKindGenerated::to_msg`] and the [`fmt::Display`] impl.
#[macro_export]
macro_rules! error_kind {
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

        impl ErrorKindGenerated for $error_kind_name {
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

            // FIXME(max): The returned kind msg has a " " at the beginning
            fn to_msg(self) -> &'static str {
                match self {
                    $( Self::$item_name => concat!($( $item_msg, )*), )*
                    Self::Unknown(_) => concat!($( $unknown_msg, )*),
                }
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
                let name = (*self).to_name();
                let msg = (*self).to_msg();
                let code = (*self).to_code();
                // ex: "[102=EntityConversion] Could not convert entity to type"
                // No ':' because the ServiceApiError's Display impl adds it.
                write!(f, "[{code}={name}]{msg}")
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

        // --- impl From RestClientErrorKind --- //

        impl From<RestClientErrorKind> for $error_kind_name {
            #[inline]
            fn from(common: RestClientErrorKind) -> Self {
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

/// Defines the common classes of errors that the `RestClient` can generate.
/// This error should not be used directly. Rather, it serves as an intermediate
/// representation; service api errors must define a `From<RestClientError>`
/// impl to ensure they have covered these cases.
pub struct RestClientError {
    pub kind: RestClientErrorKind,
    pub msg: String,
}

/// The primary error type that the backend returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BackendApiError {
    pub kind: BackendErrorKind,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub msg: String,
}

/// The primary error type that the runner returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct RunnerApiError {
    pub kind: RunnerErrorKind,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub msg: String,
}

/// The primary error type that the gateway returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct GatewayApiError {
    pub kind: GatewayErrorKind,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub msg: String,
}

/// The primary error type that the node returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct NodeApiError {
    pub kind: NodeErrorKind,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub msg: String,
}

/// The primary error type that the LSP returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct LspApiError {
    pub kind: LspErrorKind,
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    pub msg: String,
}

// --- Error variants --- //

/// All variants of errors that the [`RestClient`] can generate.
///
/// [`RestClient`]: crate::api::rest::RestClient
#[derive(Copy, Clone, Debug)]
#[repr(u16)]
pub enum RestClientErrorKind {
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
}

error_kind! {
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

        // --- Backend --- //

        /// Database error
        Database = 100,
        /// Resource not found
        NotFound = 101,
        /// Resource was duplicate
        Duplicate = 102,
        /// Could not convert entity to type
        EntityConversion = 103,
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

error_kind! {
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

        // --- Runner --- //

        /// Runner cannot take any more commands
        AtCapacity = 100,
        /// Runner gave up servicing the request, likely at capacity
        Cancelled = 101,
        /// Runner error
        Runner = 102,
    }
}

error_kind! {
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

        // --- Gateway --- //

        /// Missing fiat exchange rates; issue with upstream data source.
        FiatRatesMissing = 100,
    }
}

error_kind! {
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

        // --- Node --- //

        /// Wrong user pk
        WrongUserPk = 100,
        /// Given node pk doesn't match node pk derived from seed
        WrongNodePk = 101,
        /// Error occurred during provisioning
        Provision = 102,
        /// Node unable to authenticate
        BadAuth = 103,
        /// Could not proxy request to node
        Proxy = 104,
        /// Error while executing command
        Command = 105,
    }
}

error_kind! {
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

        // --- LSP --- //

        /// Error occurred during provisioning
        Provision = 100,
        /// Error occurred while fetching new scid
        Scid = 101,
        /// Error while executing command
        Command = 102,
    }
}

// --- RestClientError impl --- //

impl RestClientError {
    pub(crate) fn new(kind: RestClientErrorKind, msg: String) -> Self {
        Self { kind, msg }
    }

    #[inline]
    pub(crate) fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
}

// --- RestClientErrorKind impl --- //

impl RestClientErrorKind {
    #[cfg(any(test, feature = "test-utils"))]
    const KINDS: &'static [Self] = &[
        Self::UnknownReqwest,
        Self::Building,
        Self::Connect,
        Self::Timeout,
        Self::Decode,
    ];

    #[inline]
    pub fn to_code(self) -> ErrorCode {
        self as ErrorCode
    }
}

// --- Misc constructors / helpers --- //

impl BackendApiError {
    pub fn unauthorized_user() -> Self {
        let kind = BackendErrorKind::Unauthorized;
        let msg = "current user is not authorized".to_owned();
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
}

impl GatewayApiError {
    pub fn fiat_rates_missing() -> Self {
        let kind = GatewayErrorKind::FiatRatesMissing;
        let msg = kind.to_string();
        Self { kind, msg }
    }
}

// --- warp::reject::Reject impls --- ///

// Allow our error types to be returned as Rejections from warp Filters using
// `warp::reject::custom`.

impl warp::reject::Reject for BackendApiError {}
impl warp::reject::Reject for RunnerApiError {}
impl warp::reject::Reject for GatewayApiError {}
impl warp::reject::Reject for NodeApiError {}
impl warp::reject::Reject for LspApiError {}

// --- ErrorResponse -> ServiceApiError impls --- //

impl From<ErrorResponse> for BackendApiError {
    fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
        let kind = BackendErrorKind::from_code(code);
        Self { kind, msg }
    }
}
impl From<ErrorResponse> for RunnerApiError {
    fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
        let kind = RunnerErrorKind::from_code(code);
        Self { kind, msg }
    }
}
impl From<ErrorResponse> for GatewayApiError {
    fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
        let kind = GatewayErrorKind::from_code(code);
        Self { kind, msg }
    }
}
impl From<ErrorResponse> for NodeApiError {
    fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
        let kind = NodeErrorKind::from_code(code);
        Self { kind, msg }
    }
}
impl From<ErrorResponse> for LspApiError {
    fn from(ErrorResponse { code, msg }: ErrorResponse) -> Self {
        let kind = LspErrorKind::from_code(code);
        Self { kind, msg }
    }
}

// --- ServiceApiError -> ErrorResponse impls --- //

impl From<BackendApiError> for ErrorResponse {
    fn from(BackendApiError { kind, msg }: BackendApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}
impl From<RunnerApiError> for ErrorResponse {
    fn from(RunnerApiError { kind, msg }: RunnerApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}
impl From<GatewayApiError> for ErrorResponse {
    fn from(GatewayApiError { kind, msg }: GatewayApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}
impl From<NodeApiError> for ErrorResponse {
    fn from(NodeApiError { kind, msg }: NodeApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}
impl From<LspApiError> for ErrorResponse {
    fn from(LspApiError { kind, msg }: LspApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}

// --- ToHttpStatus impls --- //

impl ToHttpStatus for BackendApiError {
    fn to_http_status(&self) -> Status {
        use BackendErrorKind::*;
        match self.kind {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,

            Database => SERVER_500_INTERNAL_SERVER_ERROR,
            NotFound => CLIENT_404_NOT_FOUND,
            Duplicate => CLIENT_409_CONFLICT,
            EntityConversion => SERVER_500_INTERNAL_SERVER_ERROR,
            Unauthenticated => CLIENT_401_UNAUTHORIZED,
            Unauthorized => CLIENT_401_UNAUTHORIZED,
            AuthExpired => CLIENT_401_UNAUTHORIZED,
            InvalidParsedRequest => CLIENT_400_BAD_REQUEST,
            BatchSizeOverLimit => CLIENT_400_BAD_REQUEST,
        }
    }
}

impl ToHttpStatus for RunnerApiError {
    fn to_http_status(&self) -> Status {
        use RunnerErrorKind::*;
        match self.kind {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,

            AtCapacity => SERVER_500_INTERNAL_SERVER_ERROR,
            Cancelled => SERVER_500_INTERNAL_SERVER_ERROR,
            Runner => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

impl ToHttpStatus for GatewayApiError {
    fn to_http_status(&self) -> Status {
        use GatewayErrorKind::*;
        match self.kind {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,

            FiatRatesMissing => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

impl ToHttpStatus for NodeApiError {
    fn to_http_status(&self) -> Status {
        use NodeErrorKind::*;
        match self.kind {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,

            WrongUserPk => CLIENT_400_BAD_REQUEST,
            WrongNodePk => CLIENT_400_BAD_REQUEST,
            Provision => SERVER_500_INTERNAL_SERVER_ERROR,
            BadAuth => CLIENT_401_UNAUTHORIZED,
            Proxy => SERVER_502_BAD_GATEWAY,
            Command => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

impl ToHttpStatus for LspApiError {
    fn to_http_status(&self) -> Status {
        use LspErrorKind::*;
        match self.kind {
            Unknown(_) => SERVER_500_INTERNAL_SERVER_ERROR,

            UnknownReqwest => CLIENT_400_BAD_REQUEST,
            Building => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,

            Provision => SERVER_500_INTERNAL_SERVER_ERROR,
            Scid => SERVER_500_INTERNAL_SERVER_ERROR,
            Command => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

// --- Library crate -> RestClientError impls --- //

impl From<serde_json::Error> for RestClientError {
    fn from(err: serde_json::Error) -> Self {
        let kind = RestClientErrorKind::Decode;
        let msg = format!("Failed to deserialize response as json: {err:#}");
        Self { kind, msg }
    }
}

// Be more granular than just returning a general reqwest::Error
impl From<reqwest::Error> for RestClientError {
    fn from(err: reqwest::Error) -> Self {
        let msg = format!("{err:#}");
        let kind = if err.is_builder() {
            RestClientErrorKind::Building
        } else if err.is_connect() {
            RestClientErrorKind::Connect
        } else if err.is_timeout() {
            RestClientErrorKind::Timeout
        } else if err.is_decode() {
            RestClientErrorKind::Decode
        } else {
            RestClientErrorKind::UnknownReqwest
        };
        Self { kind, msg }
    }
}

// --- RestClientError -> ServiceApiError impls --- //

impl From<RestClientError> for BackendApiError {
    fn from(RestClientError { kind, msg }: RestClientError) -> Self {
        let kind = BackendErrorKind::from(kind);
        Self { kind, msg }
    }
}
impl From<RestClientError> for RunnerApiError {
    fn from(RestClientError { kind, msg }: RestClientError) -> Self {
        let kind = RunnerErrorKind::from(kind);
        Self { kind, msg }
    }
}
impl From<RestClientError> for GatewayApiError {
    fn from(RestClientError { kind, msg }: RestClientError) -> Self {
        let kind = GatewayErrorKind::from(kind);
        Self { kind, msg }
    }
}
impl From<RestClientError> for NodeApiError {
    fn from(RestClientError { kind, msg }: RestClientError) -> Self {
        let kind = NodeErrorKind::from(kind);
        Self { kind, msg }
    }
}
impl From<RestClientError> for LspApiError {
    fn from(RestClientError { kind, msg }: RestClientError) -> Self {
        let kind = LspErrorKind::from(kind);
        Self { kind, msg }
    }
}

// --- Misc -> BackendApiError impls --- //

impl From<bitcoin::secp256k1::Error> for BackendApiError {
    fn from(err: bitcoin::secp256k1::Error) -> Self {
        let kind = BackendErrorKind::EntityConversion;
        let msg = format!("Pubkey decode error: {err:#}");
        Self { kind, msg }
    }
}
impl From<hex::DecodeError> for BackendApiError {
    fn from(err: hex::DecodeError) -> Self {
        let kind = BackendErrorKind::EntityConversion;
        let msg = format!("Hex decode error: {err:#}");
        Self { kind, msg }
    }
}
impl From<std::num::ParseIntError> for BackendApiError {
    fn from(err: std::num::ParseIntError) -> Self {
        let kind = BackendErrorKind::EntityConversion;
        let msg = format!("Integer parsing error: {err:#}");
        Self { kind, msg }
    }
}
impl From<auth::Error> for BackendApiError {
    fn from(err: auth::Error) -> Self {
        let kind = match err {
            auth::Error::ClockDrift => BackendErrorKind::AuthExpired,
            auth::Error::Expired => BackendErrorKind::AuthExpired,
            _ => BackendErrorKind::Unauthenticated,
        };
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<super::InvalidNodePkProofSignature> for BackendApiError {
    fn from(err: super::InvalidNodePkProofSignature) -> Self {
        let kind = BackendErrorKind::Unauthenticated;
        let msg = err.to_string();
        Self { kind, msg }
    }
}

// --- Misc -> RunnerApiError impls --- //

impl<T> From<mpsc::error::TrySendError<T>> for RunnerApiError {
    fn from(err: mpsc::error::TrySendError<T>) -> Self {
        let kind = RunnerErrorKind::AtCapacity;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<oneshot::error::RecvError> for RunnerApiError {
    fn from(err: oneshot::error::RecvError) -> Self {
        let kind = RunnerErrorKind::Cancelled;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}

// --- Misc -> GatewayErrorKind impls --- //

// (Placeholder only, for consistency)

// --- Misc -> NodeApiError impls --- //

// (Placeholder only, for consistency)

// --- Misc -> LspApiError impls --- //

// (Placeholder only, for consistency)

// --- Test utils for asserting error invariants --- //

#[cfg(any(test, feature = "test-utils"))]
pub mod invariants {
    use proptest::{
        arbitrary::{any, Arbitrary},
        prop_assert_eq, proptest,
    };

    use super::*;
    use crate::test_utils::arbitrary;

    pub fn assert_error_kind_invariants<T>()
    where
        T: ErrorKindGenerated + Arbitrary,
    {
        // error code 0 and default error code must be unknown
        assert!(T::from_code(0).is_unknown());
        assert!(T::default().is_unknown());

        // RestClientErrorKind is a strict subset of T
        //
        // Client [ _, 1, 2, 3, 4, 5, 6 ]
        //      T [ _, 1, 2, 3, 4, 5,   , 100, 101 ]
        //                            ^
        //                           BAD
        for client_kind in RestClientErrorKind::KINDS {
            let client_code = client_kind.to_code();
            let other_kind = T::from_code(client_kind.to_code());
            let other_code = other_kind.to_code();
            assert_eq!(client_code, other_code, "client codes must roundtrip");

            if other_kind.is_unknown() {
                panic!(
                    "all RestClientErrorKind's should be covered; \
                     missing client code: {client_code}, \
                     client kind: {client_kind:?}",
                );
            }
        }

        // error kind enum isomorphic to error code representation
        // kind -> code -> kind2 -> code2
        for kind in T::KINDS {
            let code = kind.to_code();
            let kind2 = T::from_code(code);
            let code2 = kind2.to_code();
            assert_eq!(code, code2);
            assert_eq!(kind, &kind2);
        }

        // try the first 200 error codes to ensure isomorphic
        // code -> kind -> code2 -> kind2
        for code in 0_u16..200 {
            let kind = T::from_code(code);
            let code2 = kind.to_code();
            let kind2 = T::from_code(code2);
            assert_eq!(code, code2);
            assert_eq!(kind, kind2);
        }

        // ensure proptest generator is also well-behaved
        proptest!(|(kind in any::<T>())| {
            let code = kind.to_code();
            let kind2 = T::from_code(code);
            let code2 = kind2.to_code();
            prop_assert_eq!(code, code2);
            prop_assert_eq!(kind, kind2);
        });
    }

    pub fn assert_service_error_invariants<S, K>()
    where
        S: ServiceApiError + Arbitrary + PartialEq,
        K: ErrorKindGenerated + Arbitrary,
    {
        // Double roundtrip proptest
        // - ServiceApiError -> ErrorResponse -> ServiceApiError
        // - ErrorResponse -> ServiceApiError -> ErrorResponse
        // i.e. The errors should be equal in serialized & unserialized form.
        proptest!(|(e1 in any::<S>())| {
            let err_resp1 = Into::<ErrorResponse>::into(e1.clone());
            let e2 = S::from(err_resp1.clone());
            let err_resp2 = Into::<ErrorResponse>::into(e2.clone());
            prop_assert_eq!(e1, e2);
            prop_assert_eq!(err_resp1, err_resp2);
        });

        // Check that the ServiceApiError Display impl is of form
        // `[<code>=<kind_name>] <kind_msg>: <main_msg>`
        proptest!(|(
            kind in any::<K>(),
            main_msg in arbitrary::any_string()
        )| {
            let code = kind.to_code();
            let msg = main_msg.clone();
            let err_resp = ErrorResponse { code, msg };
            let service_error = S::from(err_resp);
            let kind_name = kind.to_name();
            let kind_msg = kind.to_msg();

            let actual_display = format!("{service_error}");
            // e.g. "[0=Unknown] Unknown error: Additional context"
            let expected_display =
                format!("[{code}={kind_name}]{kind_msg}: {main_msg}");
            prop_assert_eq!(actual_display, expected_display);
        });
    }
}

// --- Tests --- //

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn client_error_kinds_non_zero() {
        for kind in RestClientErrorKind::KINDS {
            assert_ne!(kind.to_code(), 0);
        }
    }

    #[test]
    fn error_kind_invariants() {
        invariants::assert_error_kind_invariants::<BackendErrorKind>();
        invariants::assert_error_kind_invariants::<RunnerErrorKind>();
        invariants::assert_error_kind_invariants::<GatewayErrorKind>();
        invariants::assert_error_kind_invariants::<NodeErrorKind>();
        invariants::assert_error_kind_invariants::<LspErrorKind>();
    }

    #[test]
    fn service_api_error_invariants() {
        use invariants::assert_service_error_invariants;
        assert_service_error_invariants::<BackendApiError, BackendErrorKind>();
        assert_service_error_invariants::<RunnerApiError, RunnerErrorKind>();
        assert_service_error_invariants::<GatewayApiError, GatewayErrorKind>();
        assert_service_error_invariants::<NodeApiError, NodeErrorKind>();
        assert_service_error_invariants::<LspApiError, LspErrorKind>();
    }
}

// --- Tests, but only outside of SGX --- //

#[cfg(all(test, not(target_env = "sgx")))] // no regex in SGX
mod test_notsgx {
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn error_response_serde_roundtrip() {
        proptest!(|(code in any::<ErrorCode>(), msg in "[A-Za-z0-9]*")| {
            let e1 = ErrorResponse { code, msg };
            let e1_str = serde_json::to_string(&e1).unwrap();

            // Sanity test the serialized form is what we expect
            let msg = &e1.msg;
            prop_assert_eq!(
                &e1_str,
                &format!("{{\"code\":{code},\"msg\":\"{msg}\"}}")
            );

            // Test the round trip
            let e2: ErrorResponse = serde_json::from_str(&e1_str).unwrap();
            prop_assert_eq!(e1, e2);
        })
    }
}
