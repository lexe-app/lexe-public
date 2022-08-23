use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::api::UserPk;
use crate::hex;

// pub struct ErrorResponse {
//     code: usize,
//     msg: String,
// }

// pub enum BackendApiError {
//     Common(CommonError),
//     Backend(BackendErrorKind),
// }
// pub enum RunnerApiError {
//     Common(CommonError),
//     Runner(RunnerErrorKind),
// }
// pub enum NodeApiError {
//     Common(CommonError),
//     Node(NodeErrorKind),
// }

// TODO(max): Make more generic, split out Reqwest by network, timeout, etc
/// Common errors generated during serialization, request, etc
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum CommonError {
    #[error("Reqwest error: {0}")]
    Reqwest(String),
    #[error("JSON serialization error: {0}")]
    JsonSerialization(String),
    #[error("Query string serialization error: {0}")]
    QueryStringSerialization(String),
    #[error("JSON serialization")]
    ResponseJsonSerialization(String),
}

/// All API errors that the backend can return.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum BackendApiError {
    #[error("Common error: {0:#}")]
    Common(#[from] CommonError),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Not found")]
    NotFound,
    #[error("Could not convert entity to type: {0}")]
    EntityConversion(String),
}

/// All API errors that the runner can return.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum RunnerApiError {
    #[error("Common error: {0:#}")]
    Common(#[from] CommonError),
    #[error("Database error: {0}")]
    Database(String),
    #[error("Mpsc receiver was full or dropped")]
    MpscSend,
    #[error("Oneshot sender was dropped")]
    OneshotRecv,
    #[error("Runner error: {0}")]
    Runner(String),
}

/// All API errors that the node can return.
#[derive(Error, Debug, Serialize, Deserialize)]
pub enum NodeApiError {
    #[error("Common error: {0:#}")]
    Common(#[from] CommonError),
    #[error("Wrong user pk: Node has '{saved_pk}' but received '{given_pk}'")]
    WrongUserPk { saved_pk: UserPk, given_pk: UserPk },
}

// --- CommonError From impls --- //

// Have to serialize to string because these error types don't implement ser/de
impl From<reqwest::Error> for CommonError {
    fn from(err: reqwest::Error) -> Self {
        // TODO(max): More granularity here
        Self::Reqwest(format!("{err:#}"))
    }
}
impl From<serde_json::Error> for CommonError {
    fn from(err: serde_json::Error) -> Self {
        Self::Reqwest(format!("{err:#}"))
    }
}
impl From<serde_qs::Error> for CommonError {
    fn from(err: serde_qs::Error) -> Self {
        Self::Reqwest(format!("{err:#}"))
    }
}

// --- BackendApiError From impls --- //

// Don't want the node to depend on sea-orm via the common crate
#[cfg(not(target_env = "sgx"))]
impl From<sea_orm::DbErr> for BackendApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Database(format!("{err:#}"))
    }
}
impl From<bitcoin::secp256k1::Error> for BackendApiError {
    fn from(err: bitcoin::secp256k1::Error) -> Self {
        Self::EntityConversion(format!("Pubkey decode err: {err:#}"))
    }
}
impl From<hex::DecodeError> for BackendApiError {
    fn from(err: hex::DecodeError) -> Self {
        Self::EntityConversion(format!("Hex decode error: {err:#}"))
    }
}

// --- RunnerApiError From impls --- //

// Don't want the node to depend on sea-orm via the common crate
#[cfg(not(target_env = "sgx"))]
impl From<sea_orm::DbErr> for RunnerApiError {
    fn from(err: sea_orm::DbErr) -> Self {
        Self::Database(format!("{err:#}"))
    }
}
impl<T> From<mpsc::error::SendError<T>> for RunnerApiError {
    fn from(_err: mpsc::error::SendError<T>) -> Self {
        Self::MpscSend
    }
}
impl From<oneshot::error::RecvError> for RunnerApiError {
    fn from(_err: oneshot::error::RecvError) -> Self {
        Self::OneshotRecv
    }
}
