use bitcoin::secp256k1::PublicKey;
use http::status::StatusCode as Status; // So the consts  fit in 80 chars
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::api::{auth, UserPk};
use crate::{ed25519, hex};

// Associated constants can't be imported.
const CLIENT_400_BAD_REQUEST: Status = Status::BAD_REQUEST;
const CLIENT_401_UNAUTHORIZED: Status = Status::UNAUTHORIZED;
const CLIENT_404_NOT_FOUND: Status = Status::NOT_FOUND;
const SERVER_500_INTERNAL_SERVER_ERROR: Status = Status::INTERNAL_SERVER_ERROR;
const SERVER_502_BAD_GATEWAY: Status = Status::BAD_GATEWAY;
const SERVER_503_SERVICE_UNAVAILABLE: Status = Status::SERVICE_UNAVAILABLE;
const SERVER_504_GATEWAY_TIMEOUT: Status = Status::GATEWAY_TIMEOUT;

pub type ErrorCode = u16;

/// The only error struct actually sent across the wire. Everything else is
/// converted to / from it. For displaying the full human-readable message to
/// the user, convert [`ErrorResponse`] to the service error type first.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    code: ErrorCode,
    msg: String,
}

/// A 'trait alias' defining all the supertraits a service error type must impl
/// to be accepted for use in the `RestClient` and across all Lexe services.
pub trait ServiceApiError:
    ErrorCodeConvertible + From<CommonError> + From<ErrorResponse>
{
}

impl<E: ErrorCodeConvertible + From<CommonError> + From<ErrorResponse>>
    ServiceApiError for E
{
}

/// A trait implemented on all ServiceErrorKinds that defines a
/// backwards-compatible encoding scheme for each error varinat.
pub trait ErrorCodeConvertible {
    fn to_code(&self) -> ErrorCode;
    fn from_code(code: ErrorCode) -> Self;
}

pub trait HasStatusCode {
    fn get_status_code(&self) -> Status;
}

// --- Error structs --- //

/// Defines the common classes of errors that the `RestClient` can generate.
/// This error should not be used directly. Rather, it serves as an intermediate
/// representation; service api errors must define a `From<CommonError>` impl to
/// ensure they have covered these cases.
pub struct CommonError {
    kind: CommonErrorKind,
    msg: String,
}

/// The primary error type that the backend returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub struct BackendApiError {
    #[source]
    pub kind: BackendErrorKind,
    pub msg: String,
}

/// The primary error type that the runner returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub struct RunnerApiError {
    #[source]
    pub kind: RunnerErrorKind,
    pub msg: String,
}

/// The primary error type that the gateway returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub struct GatewayApiError {
    #[source]
    pub kind: GatewayErrorKind,
    pub msg: String,
}

/// The primary error type that the node returns.
#[derive(Error, Clone, Debug, Eq, PartialEq, Hash)]
#[error("{kind}: {msg}")]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub struct NodeApiError {
    #[source]
    pub kind: NodeErrorKind,
    pub msg: String,
}

// --- Error variants --- //

/// All variants of errors that the rest client can generate.
enum CommonErrorKind {
    QueryStringSerialization,
    JsonSerialization,
    Connect,
    Timeout,
    Decode,
    Reqwest,
}

/// All variants of errors that the backend can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub enum BackendErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Client failed to serialize the given data")]
    Serialization,
    #[error("Couldn't connect to service")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Other reqwest error")]
    Reqwest,

    #[error("Database error")]
    Database,
    #[error("Resource not found")]
    NotFound,
    #[error("Could not convert entity to type")]
    EntityConversion,
    #[error("Invalid signed auth request")]
    InvalidAuthRequest,
    #[error("Auth token or auth request is expired")]
    ExpiredAuthRequest,
}

/// All variants of errors that the runner can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub enum RunnerErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Client failed to serialize the given data")]
    Serialization,
    #[error("Couldn't connect to service")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Other reqwest error")]
    Reqwest,

    #[error("Runner cannot take any more commands")]
    AtCapacity,
    #[error(
        "Runner gave up servicing the request, \
            most likely because it is at capacity."
    )]
    Cancelled,
    #[error("Runner error")]
    Runner,
}

/// All variants of errors that the gateway can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub enum GatewayErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Client failed to serialize the given data")]
    Serialization,
    #[error("Couldn't connect to service")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Other reqwest error")]
    Reqwest,

    #[error("Invalid signed auth request")]
    InvalidAuthRequest,
    #[error("Auth token or auth request is expired")]
    ExpiredAuthRequest,
}

/// All variants of errors that the node can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(all(test, not(target_env = "sgx")), derive(Arbitrary))]
pub enum NodeErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Client failed to serialize the given data")]
    Serialization,
    #[error("Couldn't connect to service")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Other reqwest error")]
    Reqwest,

    #[error("Wrong user pk")]
    WrongUserPk,
    #[error("Given node pk doesn't match node pk derived from seed")]
    WrongNodePk,
    #[error("Error occurred during provisioning")]
    Provision,

    #[error("Could not proxy request to node")]
    Proxy,
}

// --- Misc constructors / helpers --- //

impl NodeApiError {
    pub fn wrong_user_pk(current_pk: UserPk, given_pk: UserPk) -> Self {
        // We don't name these 'expected' and 'actual' because the meaning of
        // those terms is swapped depending on if you're the server or client.
        let msg = format!("Node has '{current_pk}' but received '{given_pk}'");
        let kind = NodeErrorKind::WrongUserPk;
        Self { kind, msg }
    }

    pub fn wrong_node_pk(derived_pk: PublicKey, given_pk: PublicKey) -> Self {
        // We don't name these 'expected' and 'actual' because the meaning of
        // those terms is swapped depending on if you're the server or client.
        let msg = format!("Derived '{derived_pk}' but received '{given_pk}'");
        let kind = NodeErrorKind::WrongNodePk;
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

// --- ErrorCodeConvertible impls --- //

impl ErrorCodeConvertible for BackendApiError {
    fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
    fn from_code(_code: ErrorCode) -> Self {
        unimplemented!("Shouldn't be using this!")
    }
}

impl ErrorCodeConvertible for RunnerApiError {
    fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
    fn from_code(_code: ErrorCode) -> Self {
        unimplemented!("Shouldn't be using this!")
    }
}

impl ErrorCodeConvertible for GatewayApiError {
    fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
    fn from_code(_code: ErrorCode) -> Self {
        unimplemented!("Shouldn't be using this!")
    }
}

impl ErrorCodeConvertible for NodeApiError {
    fn to_code(&self) -> ErrorCode {
        self.kind.to_code()
    }
    fn from_code(_code: ErrorCode) -> Self {
        unimplemented!("Shouldn't be using this!")
    }
}

impl ErrorCodeConvertible for BackendErrorKind {
    fn to_code(&self) -> ErrorCode {
        match self {
            Self::Unknown => 0,
            Self::Serialization => 1,
            Self::Connect => 2,
            Self::Timeout => 3,
            Self::Decode => 4,
            Self::Reqwest => 5,
            Self::Database => 6,
            Self::NotFound => 7,
            Self::EntityConversion => 8,
            Self::InvalidAuthRequest => 9,
            Self::ExpiredAuthRequest => 10,
        }
    }
    fn from_code(code: ErrorCode) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::Serialization,
            2 => Self::Connect,
            3 => Self::Timeout,
            4 => Self::Decode,
            5 => Self::Reqwest,
            6 => Self::Database,
            7 => Self::NotFound,
            8 => Self::EntityConversion,
            9 => Self::InvalidAuthRequest,
            10 => Self::ExpiredAuthRequest,
            _ => Self::Unknown,
        }
    }
}

impl ErrorCodeConvertible for RunnerErrorKind {
    fn to_code(&self) -> ErrorCode {
        match self {
            Self::Unknown => 0,
            Self::Serialization => 1,
            Self::Connect => 2,
            Self::Timeout => 3,
            Self::Decode => 4,
            Self::Reqwest => 5,
            Self::AtCapacity => 6,
            Self::Cancelled => 7,
            Self::Runner => 8,
        }
    }
    fn from_code(code: ErrorCode) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::Serialization,
            2 => Self::Connect,
            3 => Self::Timeout,
            4 => Self::Decode,
            5 => Self::Reqwest,
            6 => Self::AtCapacity,
            7 => Self::Cancelled,
            8 => Self::Runner,
            _ => Self::Unknown,
        }
    }
}

impl ErrorCodeConvertible for GatewayErrorKind {
    fn to_code(&self) -> ErrorCode {
        match self {
            Self::Unknown => 0,
            Self::Serialization => 1,
            Self::Connect => 2,
            Self::Timeout => 3,
            Self::Decode => 4,
            Self::Reqwest => 5,
            Self::InvalidAuthRequest => 6,
            Self::ExpiredAuthRequest => 7,
        }
    }
    fn from_code(code: ErrorCode) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::Serialization,
            2 => Self::Connect,
            3 => Self::Timeout,
            4 => Self::Decode,
            5 => Self::Reqwest,
            6 => Self::InvalidAuthRequest,
            7 => Self::ExpiredAuthRequest,
            _ => Self::Unknown,
        }
    }
}

impl ErrorCodeConvertible for NodeErrorKind {
    fn to_code(&self) -> ErrorCode {
        match self {
            Self::Unknown => 0,
            Self::Serialization => 1,
            Self::Connect => 2,
            Self::Timeout => 3,
            Self::Decode => 4,
            Self::Reqwest => 5,
            Self::WrongUserPk => 6,
            Self::WrongNodePk => 7,
            Self::Provision => 8,
            Self::Proxy => 9,
        }
    }
    fn from_code(code: ErrorCode) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::Serialization,
            2 => Self::Connect,
            3 => Self::Timeout,
            4 => Self::Decode,
            5 => Self::Reqwest,
            6 => Self::WrongUserPk,
            7 => Self::WrongNodePk,
            8 => Self::Provision,
            9 => Self::Proxy,
            _ => Self::Unknown,
        }
    }
}

// --- HasStatusCode impls --- //

impl HasStatusCode for BackendApiError {
    fn get_status_code(&self) -> Status {
        use BackendErrorKind::*;
        match self.kind {
            Unknown => SERVER_500_INTERNAL_SERVER_ERROR,
            Serialization => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Reqwest => CLIENT_400_BAD_REQUEST,
            Database => SERVER_500_INTERNAL_SERVER_ERROR,
            NotFound => CLIENT_404_NOT_FOUND,
            EntityConversion => SERVER_500_INTERNAL_SERVER_ERROR,
            InvalidAuthRequest => CLIENT_400_BAD_REQUEST,
            ExpiredAuthRequest => CLIENT_401_UNAUTHORIZED,
        }
    }
}

impl HasStatusCode for RunnerApiError {
    fn get_status_code(&self) -> Status {
        use RunnerErrorKind::*;
        match self.kind {
            Unknown => SERVER_500_INTERNAL_SERVER_ERROR,
            Serialization => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Reqwest => CLIENT_400_BAD_REQUEST,
            AtCapacity => SERVER_500_INTERNAL_SERVER_ERROR,
            Cancelled => SERVER_500_INTERNAL_SERVER_ERROR,
            Runner => SERVER_500_INTERNAL_SERVER_ERROR,
        }
    }
}

impl HasStatusCode for GatewayApiError {
    fn get_status_code(&self) -> Status {
        use GatewayErrorKind::*;
        match self.kind {
            Unknown => SERVER_500_INTERNAL_SERVER_ERROR,
            Serialization => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Reqwest => CLIENT_400_BAD_REQUEST,
            InvalidAuthRequest => CLIENT_400_BAD_REQUEST,
            ExpiredAuthRequest => CLIENT_401_UNAUTHORIZED,
        }
    }
}

impl HasStatusCode for NodeApiError {
    fn get_status_code(&self) -> Status {
        use NodeErrorKind::*;
        match self.kind {
            Unknown => SERVER_500_INTERNAL_SERVER_ERROR,
            Serialization => CLIENT_400_BAD_REQUEST,
            Connect => SERVER_503_SERVICE_UNAVAILABLE,
            Timeout => SERVER_504_GATEWAY_TIMEOUT,
            Decode => SERVER_502_BAD_GATEWAY,
            Reqwest => CLIENT_400_BAD_REQUEST,
            WrongUserPk => CLIENT_400_BAD_REQUEST,
            WrongNodePk => CLIENT_400_BAD_REQUEST,
            Provision => SERVER_500_INTERNAL_SERVER_ERROR,
            Proxy => SERVER_502_BAD_GATEWAY,
        }
    }
}

// --- Library crate -> CommonError impls --- //

impl From<serde_qs::Error> for CommonError {
    fn from(err: serde_qs::Error) -> Self {
        let kind = CommonErrorKind::QueryStringSerialization;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<serde_json::Error> for CommonError {
    fn from(err: serde_json::Error) -> Self {
        let kind = CommonErrorKind::JsonSerialization;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
// Be more granular than just returning a general reqwest::Error
impl From<reqwest::Error> for CommonError {
    fn from(err: reqwest::Error) -> Self {
        let msg = format!("{err:#}");
        let kind = if err.is_connect() {
            CommonErrorKind::Connect
        } else if err.is_timeout() {
            CommonErrorKind::Timeout
        } else if err.is_decode() {
            CommonErrorKind::Decode
        } else {
            CommonErrorKind::Reqwest
        };
        Self { kind, msg }
    }
}

// --- CommonError -> ServiceApiError impls --- //

impl From<CommonError> for BackendApiError {
    fn from(CommonError { kind, msg }: CommonError) -> Self {
        let kind = BackendErrorKind::from(kind);
        Self { kind, msg }
    }
}

impl From<CommonError> for RunnerApiError {
    fn from(CommonError { kind, msg }: CommonError) -> Self {
        let kind = RunnerErrorKind::from(kind);
        Self { kind, msg }
    }
}

impl From<CommonError> for GatewayApiError {
    fn from(CommonError { kind, msg }: CommonError) -> Self {
        let kind = GatewayErrorKind::from(kind);
        Self { kind, msg }
    }
}

impl From<CommonError> for NodeApiError {
    fn from(CommonError { kind, msg }: CommonError) -> Self {
        let kind = NodeErrorKind::from(kind);
        Self { kind, msg }
    }
}

// --- CommonErrorKind -> ServiceErrorKind impls --- //

impl From<CommonErrorKind> for BackendErrorKind {
    fn from(kind: CommonErrorKind) -> Self {
        use CommonErrorKind::*;
        match kind {
            QueryStringSerialization => Self::Serialization,
            JsonSerialization => Self::Serialization,
            Connect => Self::Connect,
            Timeout => Self::Timeout,
            Decode => Self::Decode,
            Reqwest => Self::Reqwest,
        }
    }
}

impl From<CommonErrorKind> for RunnerErrorKind {
    fn from(kind: CommonErrorKind) -> Self {
        use CommonErrorKind::*;
        match kind {
            QueryStringSerialization => Self::Serialization,
            JsonSerialization => Self::Serialization,
            Connect => Self::Connect,
            Timeout => Self::Timeout,
            Decode => Self::Decode,
            Reqwest => Self::Reqwest,
        }
    }
}

impl From<CommonErrorKind> for GatewayErrorKind {
    fn from(kind: CommonErrorKind) -> Self {
        use CommonErrorKind::*;
        match kind {
            QueryStringSerialization => Self::Serialization,
            JsonSerialization => Self::Serialization,
            Connect => Self::Connect,
            Timeout => Self::Timeout,
            Decode => Self::Decode,
            Reqwest => Self::Reqwest,
        }
    }
}

impl From<CommonErrorKind> for NodeErrorKind {
    fn from(kind: CommonErrorKind) -> Self {
        use CommonErrorKind::*;
        match kind {
            QueryStringSerialization => Self::Serialization,
            JsonSerialization => Self::Serialization,
            Connect => Self::Connect,
            Timeout => Self::Timeout,
            Decode => Self::Decode,
            Reqwest => Self::Reqwest,
        }
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
impl From<auth::Error> for BackendApiError {
    fn from(err: auth::Error) -> Self {
        let kind = match err {
            // TODO(phlip9): user auth token expired case
            auth::Error::ClockDrift => BackendErrorKind::ExpiredAuthRequest,
            _ => BackendErrorKind::InvalidAuthRequest,
        };
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<ed25519::Error> for BackendApiError {
    fn from(err: ed25519::Error) -> Self {
        let kind = BackendErrorKind::InvalidAuthRequest;
        let msg = format!("{err:#}");
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

impl From<auth::Error> for GatewayApiError {
    fn from(err: auth::Error) -> Self {
        let kind = match err {
            // TODO(phlip9): user auth token expired case
            auth::Error::ClockDrift => GatewayErrorKind::ExpiredAuthRequest,
            _ => GatewayErrorKind::InvalidAuthRequest,
        };
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<ed25519::Error> for GatewayApiError {
    fn from(err: ed25519::Error) -> Self {
        let kind = GatewayErrorKind::InvalidAuthRequest;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}

// --- Misc -> NodeApiError impls --- //

// (Placeholder only, to stay consistent)

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use proptest::arbitrary::any;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn context_separation() {
        let backend_error = BackendApiError {
            kind: BackendErrorKind::Unknown,
            msg: "Additional context".to_owned(),
        };
        let runner_error = RunnerApiError {
            kind: RunnerErrorKind::Unknown,
            msg: "Additional context".to_owned(),
        };
        let gateway_error = GatewayApiError {
            kind: GatewayErrorKind::Unknown,
            msg: "Additional context".to_owned(),
        };
        let node_error = NodeApiError {
            kind: NodeErrorKind::Unknown,
            msg: "Additional context".to_owned(),
        };

        // The top-level service error types *are* human readable and should
        // include the base help message defined alongside each variant.
        let model_display = String::from("Unknown error: Additional context");
        assert_eq!(model_display, format!("{backend_error}"));
        assert_eq!(model_display, format!("{runner_error}"));
        assert_eq!(model_display, format!("{gateway_error}"));
        assert_eq!(model_display, format!("{node_error}"));

        // ErrorResponse does not implement Display and is not intended to be
        // human readable as its primary purpose is for serialization /
        // transport. `msg` should only hold the _additional_ context.
        let backend_err_resp = ErrorResponse::from(backend_error);
        let runner_err_resp = ErrorResponse::from(runner_error);
        let gateway_err_resp = ErrorResponse::from(gateway_error);
        let node_err_resp = ErrorResponse::from(node_error);

        let model_err_resp = ErrorResponse {
            code: 0,
            msg: "Additional context".to_owned(),
        };
        assert_eq!(model_err_resp, backend_err_resp);
        assert_eq!(model_err_resp, runner_err_resp);
        assert_eq!(model_err_resp, gateway_err_resp);
        assert_eq!(model_err_resp, node_err_resp);
    }

    proptest! {
        #[test]
        fn error_response_serde_roundtrip(
            code in any::<ErrorCode>(),
            msg in "[A-Za-z0-9]*",
        ) {
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
        }
    }

    proptest! {
        #[test]
        fn backend_error_code_roundtrip(e1 in any::<BackendApiError>()) {
            let e2 = BackendApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
        #[test]
        fn runner_error_code_roundtrip(e1 in any::<RunnerApiError>()) {
            let e2 = RunnerApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
        #[test]
        fn gateway_error_code_roundtrip(e1 in any::<GatewayApiError>()) {
            let e2 = GatewayApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
        #[test]
        fn node_error_code_roundtrip(e1 in any::<NodeApiError>()) {
            let e2 = NodeApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
    }
}
