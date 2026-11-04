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
        ListRevocableClientsHandle, RevocableClient, RevocableClients,
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
/// Route gates use [`require`] on this request's authorization snapshot.
/// Credential mutations must use [`require_create_covered`] or
/// [`require_update_covered`] under the store's write lock. These revalidate
/// the caller and enforce endpoint permission and grant attenuation.
///
/// The resolved [`PermissionSet`] depends on the cert's issuing CA:
///
/// 1. Ephemeral (any root-seed client, e.g. the app): full access
///    (`Scope::Full.resolve()`), never expires.
/// 2. Revocable: the [`ClientPermissions`] and expiration stored in the
///    server's [`RevocableClientsHandle`].
///
/// [`require`]: Self::require
/// [`require_create_covered`]: Self::require_create_covered
/// [`require_update_covered`]: Self::require_update_covered
/// [`ClientPermissions`]: lexe_api_core::revocable_clients::scopes::ClientPermissions
/// [`RevocableClientsHandle`]: lexe_api_core::revocable_clients::RevocableClientsHandle
#[derive(Clone)]
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
    pub fn cert_kind(&self) -> ClientCertKind {
        self.cert_kind
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

    /// Authorize client creation against the current store.
    /// Keep the store write-locked through authorization and insertion.
    pub fn require_create_covered(
        &self,
        client: &RevocableClient,
        clients: &RevocableClients,
    ) -> Result<(), CommonApiError> {
        let current = Self::resolve(self.cert_kind, clients)?;
        current.require(Permission::CreateRevocableClient)?;
        current.require_permissions_covered(&client.permissions)?;
        current.require_expiration_covered(client.expires_at)
    }

    /// Authorize a client update against the current store.
    /// Keep the store write-locked through authorization and mutation.
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
        clients: &RevocableClients,
    ) -> Result<(), CommonApiError> {
        let current = Self::resolve(self.cert_kind, clients)?;
        current.require(Permission::UpdateRevocableClient)?;

        if req.expires_at.is_none() && req.permissions.is_none() {
            return Ok(());
        }

        let pubkey = req.pubkey;
        let target = clients.clients.get(&pubkey).ok_or_else(|| {
            CommonApiError::new(
                CommonErrorKind::Rejection,
                format!("No revocable client with pk {pubkey}"),
            )
        })?;

        let expires_at = req.expires_at.unwrap_or(target.expires_at);
        let permissions =
            req.permissions.as_ref().unwrap_or(&target.permissions);

        current.require_permissions_covered(permissions)?;
        current.require_expiration_covered(expires_at)
    }

    /// Resolve the current authorization for a verified client cert.
    fn resolve(
        cert_kind: ClientCertKind,
        clients: &RevocableClients,
    ) -> Result<Self, CommonApiError> {
        let ClientCertKind::Revocable { client_pk } = cert_kind else {
            return Ok(Self::root_seed());
        };
        let client = clients.clients.get(&client_pk).ok_or_else(|| {
            CommonApiError::general(format!(
                "No revocable client with pk {client_pk}"
            ))
        })?;

        if client.is_revoked {
            return Err(CommonApiError::general(format!(
                "Client {client_pk} is revoked"
            )));
        }
        if client.is_expired_at(TimestampMs::now()) {
            return Err(CommonApiError::general(format!(
                "Client {client_pk} is expired"
            )));
        }

        Ok(Self {
            cert_kind,
            permissions: client.permissions.resolve(),
            expires_at: client.expires_at,
        })
    }

    fn root_seed() -> Self {
        Self {
            cert_kind: ClientCertKind::Ephemeral,
            permissions: Scope::Full.permissions(),
            expires_at: None,
        }
    }

    /// Require `self` to "cover" all permissions in `requested`.
    ///
    /// Used to enforce scope attenuation: a client may only create or update
    /// credentials to be no more powerful than itself.
    fn require_permissions_covered(
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
    fn require_expiration_covered(
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
/// client's current [`PermissionSet`]. Memoized within each request.
impl<S> FromRequestParts<S> for VerifiedClientAuthorization
where
    S: ListRevocableClientsHandle + Send + Sync,
{
    type Rejection = CommonApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        // Get the memoized `VerifiedClientAuthorization`
        if let Some(authz) = parts.extensions.get::<Self>() {
            return Ok(authz.clone());
        }

        // The verified end-entity cert. `None` means no client cert was
        // presented, but that should not happen.
        let cert = parts.extensions.get::<VerifiedTlsClientCert>().ok_or_else(
            || CommonApiError::client_auth("No TLS client certificate info"),
        )?;
        let cert_der = cert.0.as_deref().ok_or_else(|| {
            CommonApiError::client_auth("No client certificate presented")
        })?;

        // NOTE: This cert came from `VerifiedTlsClientCert` so it is verified.
        let cert_kind = ClientCertKind::from_der_untrusted(cert_der.as_ref())
            .ok_or_else(|| {
            CommonApiError::client_auth("Failed to parse client cert kind")
        })?;

        let authz = match cert_kind {
            ClientCertKind::Ephemeral => Self::root_seed(),
            ClientCertKind::Revocable { .. } => {
                let clients =
                    state.list_revocable_clients_handle().0.read().unwrap();
                Self::resolve(cert_kind, &clients)?
            }
        };
        // Request extensions are independent per-request, so any staleness from
        // memoization can't carry over into a second request.
        parts.extensions.insert(authz.clone());
        Ok(authz)
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
    use lexe_crypto::ed25519;

    use super::*;

    #[test]
    fn mutations_recheck_attenuation() {
        let (authz, caller) = caller();
        let mut clients = store([caller.clone()]);
        let req = update(&caller);
        let check = |clients: &RevocableClients| {
            [
                authz.require_create_covered(&caller, clients).is_ok(),
                authz.require_update_covered(&req, clients).is_ok(),
            ]
        };
        assert_eq!(check(&clients), [true; 2]);

        // Retain mutation permissions to isolate grant attenuation.
        clients.clients.get_mut(&caller.pubkey).unwrap().permissions =
            ClientPermissions {
                scopes: [Scope::ReadInfo].into(),
                permissions: [
                    Permission::CreateRevocableClient,
                    Permission::UpdateRevocableClient,
                ]
                .into(),
            };
        assert_eq!(check(&clients), [false; 2]);

        let mut shortened = caller.clone();
        shortened.expires_at = Some(TimestampMs::MAX);
        assert_eq!(check(&store([shortened])), [false; 2]);
    }

    #[test]
    fn mutations_recheck_caller_validity_and_permission() {
        let (authz, caller) = caller();
        let clients = store([caller.clone()]);
        let child = RevocableClient {
            permissions: ClientPermissions::default(),
            expires_at: Some(TimestampMs::MIN),
            ..client(2)
        };
        let req = UpdateClientRequest {
            expires_at: None,
            permissions: None,
            label: Some(Some("label".to_owned())),
            ..update(&caller)
        };
        let check = |clients: &RevocableClients| {
            [
                authz.require_create_covered(&child, clients).is_ok(),
                authz.require_update_covered(&req, clients).is_ok(),
            ]
        };
        assert_eq!(check(&clients), [true; 2]);

        for changed in [
            RevocableClient {
                permissions: ClientPermissions::default(),
                ..caller.clone()
            },
            RevocableClient {
                is_revoked: true,
                ..caller.clone()
            },
            RevocableClient {
                expires_at: child.expires_at,
                ..caller
            },
        ] {
            assert_eq!(check(&store([changed])), [false; 2]);
        }
        assert_eq!(check(&RevocableClients::default()), [false; 2]);
    }

    #[test]
    fn update_attenuation_covers_unchanged_fields() {
        let (authz, mut caller) = caller();
        caller.expires_at = Some(TimestampMs::MAX);
        caller.permissions = ClientPermissions {
            scopes: [Scope::ReadInfo].into(),
            permissions: [Permission::UpdateRevocableClient].into(),
        };
        let target = client(2);
        let clients = store([caller.clone(), target.clone()]);

        // A permissions-only update must cover the target's stored expiration.
        let req = UpdateClientRequest {
            expires_at: None,
            permissions: Some(caller.permissions.clone()),
            ..update(&target)
        };
        let err = authz.require_update_covered(&req, &clients).unwrap_err();
        assert!(matches!(err.kind, CommonErrorKind::InsufficientScope));

        // An expiration-only update must cover the target's stored permissions.
        let req = UpdateClientRequest {
            expires_at: Some(caller.expires_at),
            permissions: None,
            ..req
        };
        let err = authz.require_update_covered(&req, &clients).unwrap_err();
        assert!(matches!(err.kind, CommonErrorKind::InsufficientScope));
    }

    fn client(seed: u64) -> RevocableClient {
        RevocableClient {
            pubkey: *ed25519::KeyPair::for_test(seed).public_key(),
            created_at: TimestampMs::MIN,
            expires_at: None,
            label: None,
            permissions: ClientPermissions::from_single_scope(Scope::Full),
            is_revoked: false,
        }
    }

    fn store(
        clients: impl IntoIterator<Item = RevocableClient>,
    ) -> RevocableClients {
        RevocableClients {
            clients: clients.into_iter().map(|c| (c.pubkey, c)).collect(),
        }
    }

    fn caller() -> (VerifiedClientAuthorization, RevocableClient) {
        let client = client(1);
        let authz = VerifiedClientAuthorization::resolve(
            ClientCertKind::Revocable {
                client_pk: client.pubkey,
            },
            &store([client.clone()]),
        )
        .map_err(|e| e.msg)
        .unwrap();
        (authz, client)
    }

    fn update(client: &RevocableClient) -> UpdateClientRequest {
        UpdateClientRequest {
            pubkey: client.pubkey,
            expires_at: Some(client.expires_at),
            label: None,
            permissions: Some(client.permissions.clone()),
            is_revoked: None,
        }
    }
}
