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
    error::{CommonApiError, CommonErrorKind},
    revocable_clients::{
        ListRevocableClientsHandle, RevocableClientsHandle,
        models::UpdateClientRequest,
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
/// [`require_permissions_covered`] and [`require_expiration_covered`] when
/// creating clients, and [`require_update_covered`] when updating them.
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
/// [`require_update_covered`]: Self::require_update_covered
/// [`ClientPermissions`]: lexe_api_core::revocable_clients::scopes::ClientPermissions
/// [`RevocableClientsHandle`]: lexe_api_core::revocable_clients::RevocableClientsHandle
pub struct VerifiedClientAuthorization {
    /// The verified cert kind: ephemeral (root seed) or revocable.
    cert_kind: ClientCertKind,
    /// The client's resolved permissions: the union of all granted scopes'
    /// permissions plus any explicitly granted permissions.
    permissions: PermissionSet,
    /// When this client expires. `None` means it never expires.
    expires_at: Option<TimestampMs>,
}

impl VerifiedClientAuthorization {
    /// The verified cert kind: ephemeral (root seed) or revocable.
    pub fn cert_kind(&self) -> &ClientCertKind {
        &self.cert_kind
    }

    /// The client's resolved permission set.
    pub fn permission_set(&self) -> PermissionSet {
        self.permissions
    }

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
    /// covers only earlier-or-equal expirations. A bounded client also cannot
    /// extend its own expiration.
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

    /// Enforce scope + expiration attenuation for an [`UpdateClientRequest`].
    ///
    /// If either the expiration or permissions are updated, our own credential
    /// must cover the target's resulting expiration and permissions.
    //
    // The principle here is that a credential should never be able to cause
    // another credential to be more powerful than itself.
    //
    // Example: a `full` caller expiring in an hour cannot grant `spend` to a
    // never-expiring client. Likewise, a `spend` caller cannot extend a `full`
    // client. Either would create a credential the caller could not mint.
    pub fn require_update_covered(
        &self,
        req: &UpdateClientRequest,
        revocable_clients: &RevocableClientsHandle,
    ) -> Result<(), CommonApiError> {
        if req.expires_at.is_none() && req.permissions.is_none() {
            return Ok(());
        }

        let pubkey = req.pubkey;
        let clients = revocable_clients.0.read().unwrap();
        let target = clients.clients.get(&pubkey).ok_or_else(|| {
            CommonApiError::new(
                CommonErrorKind::Rejection,
                format!("No revocable client with pk {pubkey}"),
            )
        })?;

        let expires_at = req.expires_at.unwrap_or(target.expires_at);
        let permissions =
            req.permissions.as_ref().unwrap_or(&target.permissions);

        self.require_permissions_covered(permissions)?;
        self.require_expiration_covered(expires_at)
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

        let (permissions, expires_at) = match &cert_kind {
            // Ephemeral (root-seed) client: full access, never expires.
            ClientCertKind::Ephemeral => (Scope::Full.permissions(), None),
            // Revocable client: resolve its stored authorization.
            ClientCertKind::Revocable { client_pk } => {
                let revocable_clients = state.list_revocable_clients_handle();
                let locked_rev_clients = revocable_clients.0.read().unwrap();
                let client = locked_rev_clients
                    .clients
                    .get(client_pk)
                    .ok_or_else(|| {
                        CommonApiError::client_auth(
                            "Revocable client not found",
                        )
                    })?;
                (client.permissions.resolve(), client.expires_at)
            }
        };

        Ok(VerifiedClientAuthorization {
            cert_kind,
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

#[cfg(test)]
mod test {
    use std::{collections::HashMap, sync::RwLock};

    use lexe_api_core::revocable_clients::{RevocableClient, RevocableClients};
    use lexe_crypto::ed25519;

    use super::*;

    #[test]
    fn update_requires_resulting_authorization_covered() {
        let make_target = |seed, expires_at, scope| {
            let pubkey = *ed25519::KeyPair::for_test(seed).public_key();
            RevocableClient {
                pubkey,
                created_at: TimestampMs::from_u8(0),
                expires_at,
                label: None,
                permissions: ClientPermissions::from_single_scope(scope),
                is_revoked: false,
            }
        };
        let store = |target: RevocableClient| {
            RevocableClientsHandle(RwLock::new(RevocableClients {
                clients: HashMap::from([(target.pubkey, target)]),
            }))
        };
        let expires_at = TimestampMs::from_u8(2);

        // A permissions-only update must be covered for the target's stored
        // expiration.
        let target = make_target(1, None, Scope::ReadInfo);
        let pubkey = target.pubkey;
        let revocable_clients = store(target);
        let caller = VerifiedClientAuthorization {
            cert_kind: ClientCertKind::Ephemeral,
            permissions: Scope::Full.permissions(),
            expires_at: Some(expires_at),
        };
        let req = UpdateClientRequest {
            pubkey,
            expires_at: None,
            label: None,
            permissions: Some(ClientPermissions::from_single_scope(
                Scope::Receive,
            )),
            is_revoked: None,
        };
        let err = caller
            .require_update_covered(&req, &revocable_clients)
            .unwrap_err();
        assert!(err.msg.contains("expiration later than its own"));

        // An expiration-only update must be covered for the target's stored
        // permissions.
        let target = make_target(2, Some(TimestampMs::from_u8(1)), Scope::Full);
        let pubkey = target.pubkey;
        let revocable_clients = store(target);
        let caller = VerifiedClientAuthorization {
            cert_kind: ClientCertKind::Ephemeral,
            permissions: Scope::Spend.permissions(),
            expires_at: Some(expires_at),
        };
        let req = UpdateClientRequest {
            pubkey,
            expires_at: Some(Some(expires_at)),
            label: None,
            permissions: None,
            is_revoked: None,
        };

        let err = caller
            .require_update_covered(&req, &revocable_clients)
            .unwrap_err();
        assert!(err.msg.contains("required permissions"));
    }
}
