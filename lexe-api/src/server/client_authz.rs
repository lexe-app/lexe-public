//! Client authorization for HTTP endpoints, based on mTLS client certs.
//!
//! `VerifiedClientAuthorization` resolves an authenticated client's granted
//! permissions; `scoped` and `unscoped` are the axum routing verbs that
//! enforce, or deliberately skip, a per-route `Permission` gate.

use axum::{
    extract::{FromRequestParts, Request, State},
    handler::Handler,
    http::request::Parts,
    response::IntoResponse,
    routing::{self, MethodRouter},
};
use lexe_api_core::{
    error::CommonApiError,
    revocable_clients::{
        ListRevocableClientsHandle,
        scopes::{ClientPermissions, Permission, PermissionSet, Scope},
    },
};
use lexe_common::time::TimestampMs;
use lexe_tls::shared_seed::ClientCertKind;

use crate::tls_acceptor::VerifiedTlsClientCert;

/// Represents the authorization given to an mTLS client whose client cert we
/// have authenticated. Includes the client's [`PermissionSet`] resolved from
/// the revocable clients store, and the client's expiration.
///
/// Handlers call [`require`] to declare the [`Permission`] they
/// need. Attenuation of auth scopes and expirations should be enforced using
/// the [`require_permissions_covered`] and [`require_expiration_covered`]
/// methods.
///
/// The resolved [`PermissionSet`] depends on the cert's issuing CA:
///
/// 1. Ephemeral (any root-seed client, e.g. the app): full access
///    (`Scope::Full.resolve()`), never expires.
/// 2. Revocable: the [`ClientPermissions`] and expiration stored in the
///    server's [`RevocableClientsHandle`].
///
/// [`require`]: Self::require
/// [`require_permissions_covered`]: Self::require_permissions_covered
/// [`require_expiration_covered`]: Self::require_expiration_covered
/// [`ClientPermissions`]: lexe_api_core::revocable_clients::scopes::ClientPermissions
/// [`RevocableClientsHandle`]: lexe_api_core::revocable_clients::RevocableClientsHandle
pub struct VerifiedClientAuthorization {
    permissions: PermissionSet,
    /// When this client expires. `None` means it never expires.
    expires_at: Option<TimestampMs>,
}

impl VerifiedClientAuthorization {
    /// Require the client to hold `permission`, else reject (fail-closed).
    pub fn require(
        &self,
        permission: Permission,
    ) -> Result<(), CommonApiError> {
        if self.permissions.contains(permission) {
            Ok(())
        } else {
            Err(CommonApiError::insufficient_scope(permission))
        }
    }

    /// Require `self` to "cover" all permissions in `requested`.
    ///
    /// Used to enforce scope attenuation: a client may only create or update
    /// credentials to be no more powerful than itself.
    pub fn require_permissions_covered(
        &self,
        requested: &ClientPermissions,
    ) -> Result<(), CommonApiError> {
        let unauthorized_permissions =
            requested.resolve().difference(self.permissions);
        if unauthorized_permissions.is_empty() {
            Ok(())
        } else {
            Err(CommonApiError::insufficient_scopes(
                unauthorized_permissions,
            ))
        }
    }

    /// Require `self`'s expiration to cover the `requested` expiration.
    ///
    /// Used to enforce expiration attenuation: a client may only create or
    /// update credentials to expire no later than itself. An unbounded
    /// client (`expires_at: None`) covers any expiration; a bounded client
    /// covers only earlier-or-equal expirations. In particular, a bounded
    /// client cannot extend its own expiration.
    pub fn require_expiration_covered(
        &self,
        requested: Option<TimestampMs>,
    ) -> Result<(), CommonApiError> {
        match (self.expires_at, requested) {
            (None, _) => Ok(()),
            (Some(own), Some(req)) if req <= own => Ok(()),
            (Some(own), req) =>
                Err(CommonApiError::insufficient_expiration(req, own)),
        }
    }
}

/// Reads the [`VerifiedTlsClientCert`] from request extensions and resolves the
/// client's [`PermissionSet`]. The TLS layer already verified the cert,
/// including revocation and expiration.
//
// TODO(max): Consider resolving this once per connection and injecting it into
// request extensions like `VerifiedTlsClientCert`. More efficient, but less
// safe: a revoked or downgraded client would keep its old authorization until
// it disconnects - unless we close its connections on any client update.
impl<S> FromRequestParts<S> for VerifiedClientAuthorization
where
    S: ListRevocableClientsHandle + Send + Sync,
{
    type Rejection = CommonApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let cert = parts.extensions.get::<VerifiedTlsClientCert>().ok_or_else(
            || CommonApiError::client_auth("No TLS client certificate info"),
        )?;
        let cert_der = cert.0.as_deref().ok_or_else(|| {
            CommonApiError::client_auth("No client certificate presented")
        })?;

        // NOTE: This cert came from `VerifiedTlsClientCert` so it is verified.
        let cert_kind = ClientCertKind::from_der_untrusted(cert_der.as_ref())
            .ok_or_else(|| {
            CommonApiError::client_auth(
                "Failed to parse client certificate type",
            )
        })?;

        let (permissions, expires_at) = match cert_kind {
            // Ephemeral (root-seed) client: full access, never expires.
            ClientCertKind::Ephemeral => (Scope::Full.permissions(), None),
            // Revocable client: resolve its stored authorization.
            ClientCertKind::Revocable { client_pk } => {
                let revocable_clients = state.list_revocable_clients_handle();
                let locked_rev_clients = revocable_clients.0.read().unwrap();
                let client = locked_rev_clients
                    .clients
                    .get(&client_pk)
                    .ok_or_else(|| {
                        CommonApiError::client_auth(
                            "Revocable client not found",
                        )
                    })?;
                (client.permissions.resolve(), client.expires_at)
            }
        };

        Ok(VerifiedClientAuthorization {
            permissions,
            expires_at,
        })
    }
}

// --- Scoped routing --- //

/// Axum routing verbs that gate a route on a client [`Permission`].
///
/// ```ignore
/// let router = Router::new()
///     // Gated on a permission:
///     .route("/app/node_info", scoped::get(Permission::NodeInfo, node_info))
///     .route("/app/pay_invoice", scoped::post(Permission::PayInvoice, pay_invoice))
///     // Deliberately ungated — see the `unscoped` module:
///     .route("/lexe/test_event", unscoped::post(test_event))
///     .with_state(state);
/// ```
///
/// Each verb mirrors its axum counterpart with a leading `permission` and runs
/// [`VerifiedClientAuthorization::require`] first, rejecting the request unless
/// the caller holds `permission`.
pub mod scoped {
    use super::*;

    /// Like [`axum::routing::get`], but rejects if the caller doesn't hold
    /// `permission`.
    pub fn get<H, T, S>(permission: Permission, handler: H) -> MethodRouter<S>
    where
        H: Handler<T, S>,
        T: 'static,
        S: ListRevocableClientsHandle + Clone + Send + Sync + 'static,
    {
        routing::get(
            move |permissions: VerifiedClientAuthorization,
                  State(state): State<S>,
                  request: Request| {
                let handler = handler.clone();
                async move {
                    if let Err(rejection) = permissions.require(permission) {
                        return rejection.into_response();
                    }
                    handler.call(request, state).await
                }
            },
        )
    }

    /// Like [`axum::routing::post`], but rejects if the caller doesn't hold
    /// `permission`.
    pub fn post<H, T, S>(permission: Permission, handler: H) -> MethodRouter<S>
    where
        H: Handler<T, S>,
        T: 'static,
        S: ListRevocableClientsHandle + Clone + Send + Sync + 'static,
    {
        routing::post(
            move |permissions: VerifiedClientAuthorization,
                  State(state): State<S>,
                  request: Request| {
                let handler = handler.clone();
                async move {
                    if let Err(rejection) = permissions.require(permission) {
                        return rejection.into_response();
                    }
                    handler.call(request, state).await
                }
            },
        )
    }

    /// Like [`axum::routing::put`], but rejects if the caller doesn't hold
    /// `permission`.
    pub fn put<H, T, S>(permission: Permission, handler: H) -> MethodRouter<S>
    where
        H: Handler<T, S>,
        T: 'static,
        S: ListRevocableClientsHandle + Clone + Send + Sync + 'static,
    {
        routing::put(
            move |permissions: VerifiedClientAuthorization,
                  State(state): State<S>,
                  request: Request| {
                let handler = handler.clone();
                async move {
                    if let Err(rejection) = permissions.require(permission) {
                        return rejection.into_response();
                    }
                    handler.call(request, state).await
                }
            },
        )
    }

    /// Like [`axum::routing::patch`], but rejects if the caller doesn't hold
    /// `permission`.
    pub fn patch<H, T, S>(permission: Permission, handler: H) -> MethodRouter<S>
    where
        H: Handler<T, S>,
        T: 'static,
        S: ListRevocableClientsHandle + Clone + Send + Sync + 'static,
    {
        routing::patch(
            move |permissions: VerifiedClientAuthorization,
                  State(state): State<S>,
                  request: Request| {
                let handler = handler.clone();
                async move {
                    if let Err(rejection) = permissions.require(permission) {
                        return rejection.into_response();
                    }
                    handler.call(request, state).await
                }
            },
        )
    }

    /// Like [`axum::routing::delete`], but rejects if the caller doesn't hold
    /// `permission`.
    pub fn delete<H, T, S>(
        permission: Permission,
        handler: H,
    ) -> MethodRouter<S>
    where
        H: Handler<T, S>,
        T: 'static,
        S: ListRevocableClientsHandle + Clone + Send + Sync + 'static,
    {
        routing::delete(
            move |permissions: VerifiedClientAuthorization,
                  State(state): State<S>,
                  request: Request| {
                let handler = handler.clone();
                async move {
                    if let Err(rejection) = permissions.require(permission) {
                        return rejection.into_response();
                    }
                    handler.call(request, state).await
                }
            },
        )
    }
}

// --- Unscoped routing --- //

/// Axum routing verbs for routes deliberately left off the [`scoped`] gate:
/// reachable by any authenticated client, no [`Permission`] required. The
/// `unscoped::` prefix marks the absent scope as intentional, so it doesn't
/// read as a forgotten [`scoped`] gate.
pub mod unscoped {
    pub use axum::routing::{delete, get, patch, post, put};
}
