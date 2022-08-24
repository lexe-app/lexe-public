use bitcoin::secp256k1::PublicKey;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::arbitrary::Arbitrary;
#[cfg(all(test, not(target_env = "sgx")))]
use proptest::strategy::{BoxedStrategy, Just, Strategy};
#[cfg(all(test, not(target_env = "sgx")))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::api::UserPk;
use crate::hex;

/// The only error struct actually sent across the wire.
/// Everything else is converted to / from it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    code: u16,
    msg: String,
}

/// A trait implemented on all ServiceErrorKinds that defines a
/// backwards-compatible encoding scheme for each error varinat.
pub trait ErrorCodeConvertible {
    fn to_code(self) -> u16;
    fn from_code(code: u16) -> Self;
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
pub enum BackendErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Serialization error")]
    Serialization,
    #[error("Couldn't connect")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Reqwest error")]
    Reqwest,

    #[error("Database error")]
    Database,
    #[error("Not found")]
    NotFound,
    #[error("Could not convert entity to type")]
    EntityConversion,
}

/// All variants of errors that the runner can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum RunnerErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Serialization error")]
    Serialization,
    #[error("Couldn't connect")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Reqwest error")]
    Reqwest,

    #[error("Database error")]
    Database,
    #[error("Mpsc receiver was full or dropped")]
    MpscSend,
    #[error("Oneshot sender was dropped")]
    OneshotRecv,
    #[error("Runner error")]
    Runner,
}

/// All variants of errors that the node can return.
#[derive(Error, Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum NodeErrorKind {
    #[error("Unknown error")]
    Unknown,
    #[error("Serialization error")]
    Serialization,
    #[error("Couldn't connect")]
    Connect,
    #[error("Request timed out")]
    Timeout,
    #[error("Could not decode response")]
    Decode,
    #[error("Reqwest error")]
    Reqwest,

    #[error("Wrong user pk")]
    WrongUserPk,
    #[error("Given node pk doesn't match node pk derived from seed")]
    WrongNodePk,
    #[error("Error occurred during provisioning")]
    Provision,
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
impl From<NodeApiError> for ErrorResponse {
    fn from(NodeApiError { kind, msg }: NodeApiError) -> Self {
        let code = kind.to_code();
        Self { code, msg }
    }
}

// --- ErrorCodeConvertible impls --- //

impl ErrorCodeConvertible for BackendErrorKind {
    fn to_code(self) -> u16 {
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
        }
    }
    fn from_code(code: u16) -> Self {
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
            _ => Self::Unknown,
        }
    }
}

impl ErrorCodeConvertible for RunnerErrorKind {
    fn to_code(self) -> u16 {
        match self {
            Self::Unknown => 0,
            Self::Serialization => 1,
            Self::Connect => 2,
            Self::Timeout => 3,
            Self::Decode => 4,
            Self::Reqwest => 5,
            Self::Database => 6,
            Self::MpscSend => 7,
            Self::OneshotRecv => 8,
            Self::Runner => 9,
        }
    }
    fn from_code(code: u16) -> Self {
        match code {
            0 => Self::Unknown,
            1 => Self::Serialization,
            2 => Self::Connect,
            3 => Self::Timeout,
            4 => Self::Decode,
            5 => Self::Reqwest,
            6 => Self::Database,
            7 => Self::MpscSend,
            8 => Self::OneshotRecv,
            9 => Self::Runner,
            _ => Self::Unknown,
        }
    }
}

impl ErrorCodeConvertible for NodeErrorKind {
    fn to_code(self) -> u16 {
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
        }
    }
    fn from_code(code: u16) -> Self {
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
            _ => Self::Unknown,
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

// --- Library -> BackendApiError impls --- //

// Don't want the node to depend on sea-orm via the common crate
#[cfg(not(target_env = "sgx"))]
impl From<sea_orm::DbErr> for BackendApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        let kind = BackendErrorKind::Database;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
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

// --- Library -> RunnerApiError impls --- //

// Don't want the node to depend on sea-orm via the common crate
#[cfg(not(target_env = "sgx"))]
impl From<sea_orm::DbErr> for RunnerApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        let kind = RunnerErrorKind::Database;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl<T> From<mpsc::error::SendError<T>> for RunnerApiError {
    fn from(err: mpsc::error::SendError<T>) -> Self {
        let kind = RunnerErrorKind::MpscSend;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}
impl From<oneshot::error::RecvError> for RunnerApiError {
    fn from(err: oneshot::error::RecvError) -> Self {
        let kind = RunnerErrorKind::OneshotRecv;
        let msg = format!("{err:#}");
        Self { kind, msg }
    }
}

// --- Library -> NodeApiError impls --- //

// --- Arbitrary impls --- //

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for BackendErrorKind {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // You have been brought here because you added an error variant.
        // Add another `Just` entry for the variant you just added below.
        match Self::Unknown {
            Self::Unknown
            | Self::Serialization
            | Self::Connect
            | Self::Timeout
            | Self::Decode
            | Self::Reqwest
            | Self::Database
            | Self::NotFound
            | Self::EntityConversion => {}
        }

        proptest::prop_oneof! {
            Just(Self::Unknown),
            Just(Self::Serialization),
            Just(Self::Connect),
            Just(Self::Timeout),
            Just(Self::Decode),
            Just(Self::Reqwest),
            Just(Self::Database),
            Just(Self::NotFound),
            Just(Self::EntityConversion),
        }
        .boxed()
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for RunnerErrorKind {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // You have been brought here because you added an error variant.
        // Add another `Just` entry for the variant you just added below.
        match Self::Unknown {
            Self::Unknown
            | Self::Serialization
            | Self::Connect
            | Self::Timeout
            | Self::Decode
            | Self::Reqwest
            | Self::Database
            | Self::MpscSend
            | Self::OneshotRecv
            | Self::Runner => {}
        }

        proptest::prop_oneof! {
            Just(Self::Unknown),
            Just(Self::Serialization),
            Just(Self::Connect),
            Just(Self::Timeout),
            Just(Self::Decode),
            Just(Self::Reqwest),
            Just(Self::Database),
            Just(Self::MpscSend),
            Just(Self::OneshotRecv),
            Just(Self::Runner),
        }
        .boxed()
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
impl Arbitrary for NodeErrorKind {
    type Parameters = ();
    type Strategy = BoxedStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        // You have been brought here because you added an error variant.
        // Add another `Just` entry for the variant you just added below.
        match Self::Unknown {
            Self::Unknown
            | Self::Serialization
            | Self::Connect
            | Self::Timeout
            | Self::Decode
            | Self::Reqwest
            | Self::WrongUserPk
            | Self::WrongNodePk
            | Self::Provision => {}
        }

        proptest::prop_oneof! {
            Just(Self::Unknown),
            Just(Self::Serialization),
            Just(Self::Connect),
            Just(Self::Timeout),
            Just(Self::Decode),
            Just(Self::Reqwest),
            Just(Self::WrongUserPk),
            Just(Self::WrongNodePk),
            Just(Self::Provision),
        }
        .boxed()
    }
}

#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use proptest::arbitrary::any;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    proptest! {
        #[test]
        fn backend_error_roundtrip(e1 in any::<BackendApiError>()) {
            let e2 = BackendApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
        #[test]
        fn runner_error_roundtrip(e1 in any::<RunnerApiError>()) {
            let e2 = RunnerApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
        #[test]
        fn node_error_roundtrip(e1 in any::<NodeApiError>()) {
            let e2 = NodeApiError::from(ErrorResponse::from(e1.clone()));
            prop_assert_eq!(e1, e2);
        }
    }
}
