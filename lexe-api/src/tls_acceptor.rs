//! TLS acceptor that injects the verified client certificate into request
//! extensions after the TLS handshake completes successfully.
//!
//! [`CertInjectorAcceptor`] wraps axum-server's `RustlsAcceptor` to extract the
//! client's end-entity cert and inject it as [`VerifiedTlsClientCert`].
//!
//! [`CertInjectorAcceptor`]: crate::tls_acceptor::CertInjectorAcceptor
//! [`VerifiedTlsClientCert`]: crate::tls_acceptor::VerifiedTlsClientCert

use std::io;

use axum_server::{accept::Accept, tls_rustls::RustlsAcceptor};
use futures::{FutureExt, future::Map};
use rustls::pki_types::CertificateDer;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::server::TlsStream;
use tower::Layer;
use tower_http::add_extension::{AddExtension, AddExtensionLayer};

/// The verified end-entity client certificate from an mTLS connection.
///
/// During the mTLS handshake the client presents its cert chain leaf-first;
/// this is that leaf (end-entity) cert, after it passed verification against
/// the server's configured [`ClientCertVerifier`]. The rest of the chain is
/// discarded: the client typically doesn't send the CA cert, and we only need
/// the end-entity cert to identify the client.
///
/// `None` if the client presented no certificate (non-mTLS connection).
///
/// [`ClientCertVerifier`]: rustls::server::danger::ClientCertVerifier
#[derive(Clone, Debug)]
pub struct VerifiedTlsClientCert(pub Option<CertificateDer<'static>>);

/// An acceptor that wraps [`RustlsAcceptor`] to inject the verified client
/// certificate into request extensions.
///
/// After the TLS handshake completes, this acceptor extracts the end-entity
/// peer certificate from the [`TlsStream`] and wraps the service with an
/// [`AddExtension`] layer containing [`VerifiedTlsClientCert`].
///
/// # Example
///
/// ```ignore
/// use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
/// use lexe_api::tls_acceptor::CertInjectorAcceptor;
///
/// let rustls_config = RustlsConfig::from_config(tls_config);
/// let rustls_acceptor = RustlsAcceptor::new(rustls_config);
/// let acceptor = CertInjectorAcceptor::new(rustls_acceptor);
///
/// axum_server::from_tcp(listener)
///     .acceptor(acceptor)
///     .serve(make_service)
///     .await
/// ```
#[derive(Clone, Debug)]
pub struct CertInjectorAcceptor {
    inner: RustlsAcceptor,
}

impl CertInjectorAcceptor {
    /// Create a new [`CertInjectorAcceptor`] wrapping the given
    /// [`RustlsAcceptor`].
    pub fn new(inner: RustlsAcceptor) -> Self {
        Self { inner }
    }
}

impl<I, S> Accept<I, S> for CertInjectorAcceptor
where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = TlsStream<I>;
    type Service = AddExtension<S, VerifiedTlsClientCert>;
    type Future = Map<
        <RustlsAcceptor as Accept<I, S>>::Future,
        fn(
            io::Result<(TlsStream<I>, S)>,
        ) -> io::Result<(
            TlsStream<I>,
            AddExtension<S, VerifiedTlsClientCert>,
        )>,
    >;

    fn accept(&self, stream: I, service: S) -> Self::Future {
        // Perform the TLS handshake via the inner rustls acceptor
        self.inner
            .accept(stream, service)
            .map(|result| -> io::Result<_> {
                let (tls_stream, service) = result?;

                // Extract the verified end-entity client cert, if any.
                // The cert chain returned by `peer_certificates()` is leaf
                // first, i.e. the end-entity cert comes first.
                let (_, server_conn) = tls_stream.get_ref();
                let client_cert = server_conn
                    .peer_certificates()
                    .and_then(|chain| chain.first())
                    .cloned();
                let verified_cert = VerifiedTlsClientCert(client_cert);

                // Wrap service to inject the cert into http request extensions
                let service =
                    AddExtensionLayer::new(verified_cert).layer(service);
                Ok((tls_stream, service))
            })
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use axum::{Router, extract::Request, routing::post};
    use lexe_api_core::error::BackendApiError;
    use lexe_common::root_seed::RootSeed;
    use lexe_crypto::rng::FastRng;
    use lexe_tls::{
        LEXE_ALPN_PROTOCOLS, LEXE_CRYPTO_PROVIDER, client_config_builder,
        server_config_builder,
        shared_seed::certs::{
            EphemeralClientCert, EphemeralIssuingCaCert, EphemeralServerCert,
        },
    };
    use rustls::{RootCertStore, server::WebPkiClientVerifier};

    use super::VerifiedTlsClientCert;
    use crate::{
        rest::RestClient, server::LxJson, test_utils::with_test_server,
    };

    /// A client presenting a cert via mTLS should have its end-entity cert
    /// extracted by [`CertInjectorAcceptor`] and injected into the handler's
    /// extensions, byte-for-byte.
    #[tokio::test]
    async fn injects_client_cert_into_handler() {
        const DNS: &str = "localhost";
        let mut rng = FastRng::from_u64(20240514);

        // The ephemeral issuing CA that signs both end-entity certs.
        let ca_cert = EphemeralIssuingCaCert::from_root_seed(
            &RootSeed::from_rng(&mut rng),
        );
        let ca_cert_der = ca_cert.serialize_der_self_signed().unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(ca_cert_der.into()).unwrap();
        let roots = Arc::new(roots);

        // Server cert (SAN=localhost) and client cert, both CA-signed.
        let server_cert =
            EphemeralServerCert::from_rng(&mut rng, &[DNS]).unwrap();
        let server_cert_der =
            server_cert.serialize_der_ca_signed(&ca_cert).unwrap();
        let client_cert = EphemeralClientCert::generate_from_rng(&mut rng);
        let client_cert_der =
            client_cert.serialize_der_ca_signed(&ca_cert).unwrap();
        // The exact DER bytes we expect to recover from the handler.
        let expected_cert_der = client_cert_der.0.clone();

        // Server requires + verifies client certs signed by the CA.
        let client_verifier = WebPkiClientVerifier::builder_with_provider(
            roots.clone(),
            LEXE_CRYPTO_PROVIDER.clone(),
        )
        .build()
        .unwrap();
        let mut server_config = server_config_builder()
            .with_client_cert_verifier(client_verifier)
            .with_single_cert(
                vec![server_cert_der.into()],
                server_cert.serialize_key_der().into(),
            )
            .unwrap();
        server_config
            .alpn_protocols
            .clone_from(&LEXE_ALPN_PROTOCOLS);
        let server_config = Arc::new(server_config);

        // Client trusts the CA and presents its client cert.
        let mut client_config = client_config_builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(
                vec![client_cert_der.into()],
                client_cert.serialize_key_der().into(),
            )
            .unwrap();
        client_config
            .alpn_protocols
            .clone_from(&LEXE_ALPN_PROTOCOLS);

        // Echoes back the DER of the end-entity client cert the injector
        // recovered, so the test can verify it survived the round trip.
        async fn handler(req: Request) -> LxJson<Option<Vec<u8>>> {
            let cert_der = req
                .extensions()
                .get::<VerifiedTlsClientCert>()
                .and_then(|cert| cert.0.as_ref())
                .map(|cert| cert.as_ref().to_vec());
            LxJson(cert_der)
        }

        let router = Router::new().route("/client_cert", post(handler));
        with_test_server(server_config, DNS, router, |server_url| async move {
            let rest =
                RestClient::new("test-client", "test-server", client_config);
            let url = format!("{server_url}/client_cert");
            let http_req = rest.post(url, &());
            let recovered_cert_der: Option<Vec<u8>> = rest
                .send::<_, BackendApiError>(http_req)
                .await
                .expect("Request failed");

            // The injector recovered the exact end-entity cert, byte-for-byte.
            assert_eq!(recovered_cert_der, Some(expected_cert_der));
        })
        .await;
    }
}
