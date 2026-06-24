//! The `GetRevocableClientStatus` interface for handshake-time revocation
//! checks.

use std::fmt;

use lexe_crypto::ed25519;

use crate::time::TimestampMs;

/// A handshake-time validity check for a revocable client cert.
///
/// This trait exists mainly so `RevocableClients` can live in `lexe-api-core`
/// rather than `lexe-common`.
///
/// Implemented by `lexe_api_core::revocable_clients::RevocableClientsHandle`.
pub trait GetRevocableClientStatus: fmt::Debug + Send + Sync {
    /// The status of the revocable client identified by `client_pk` at `now`,
    /// or [`None`] if no client with this pubkey exists.
    fn get_client_status(
        &self,
        client_pk: &ed25519::PublicKey,
        now: TimestampMs,
    ) -> Option<RevocableClientStatus>;
}

/// The handshake-time status of a revocable client cert.
pub enum RevocableClientStatus {
    /// Not revoked and not expired — accept the cert.
    ///
    /// NOTE: This only means that the client is allowed to connect; it does NOT
    /// mean that the client is *authorized* to do whatever it is asking to do.
    /// Client scopes and budgets must still be enforced at a higher level.
    Valid,
    /// The client was revoked. Revocation is permanent.
    Revoked,
    /// The client is expired as of the queried time.
    Expired,
}
