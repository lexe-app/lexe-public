//! Generate the root CA cert and end-entity certs for both clients and nodes
//! after provisioning.
//!
//! ## Background
//!
//! Clients need to send commands to their nodes after provisioning. To do this
//! safely, both the client and the node need to authenticate each other. The
//! client wants to verify that they're talking to one of their previously
//! provisioned nodes. Likewise, the node wants to verify the inbound connection
//! is coming from the right client.
//!
//! This module contains the core types for generating the x509 certs needed so
//! that clients and nodes can authenticate each other and build a secure
//! channel via mTLS (mutual-auth TLS).

#![allow(dead_code)]

use rcgen::{
    date_time_ymd, BasicConstraints, CertificateParams, DistinguishedName,
    DnType, IsCa, RcgenError, SanType,
};

use crate::ed25519;

pub struct CaCert(rcgen::Certificate);

pub struct ClientCert(rcgen::Certificate);

pub struct NodeCert(rcgen::Certificate);

pub fn lexe_distinguished_name_prefix() -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CountryName, "US");
    name.push(DnType::StateOrProvinceName, "CA");
    name.push(DnType::OrganizationName, "lexe-tech");
    name
}

// -- impl CaCert -- //

impl CaCert {
    pub fn from_key_pair(key_pair: rcgen::KeyPair) -> Result<Self, RcgenError> {
        let mut name = lexe_distinguished_name_prefix();
        name.push(DnType::CommonName, "client CA cert");

        let mut params = CertificateParams::default();
        params.alg = &rcgen::PKCS_ED25519;
        params.key_pair = Some(ed25519::verify_compatible(key_pair)?);
        // no expiration
        params.not_before = date_time_ymd(1975, 1, 1);
        params.not_after = date_time_ymd(4096, 1, 1);
        params.distinguished_name = name;
        // TODO(phlip9): should have no intermediate certs
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        // TODO(phlip9): add name constraints
        // params.name_constraints = Some(NameConstraints { .. })
        // TODO(phlip9): does CA need subject alt name?
        // params.subject_alt_names = Vec::new();

        Ok(Self(rcgen::Certificate::from_params(params)?))
    }

    /// Serialize and sign the CA cert. The CA cert is self-signed.
    pub fn serialize_der_signed(&self) -> Result<Vec<u8>, RcgenError> {
        self.0.serialize_der()
    }
}

// -- impl ClientCert -- //

impl ClientCert {
    pub fn from_key_pair(key_pair: rcgen::KeyPair) -> Result<Self, RcgenError> {
        let mut name = lexe_distinguished_name_prefix();
        name.push(DnType::CommonName, "client cert");

        let mut params = CertificateParams::default();
        params.alg = &rcgen::PKCS_ED25519;
        params.key_pair = Some(ed25519::verify_compatible(key_pair)?);
        // no expiration
        // TODO(phlip9): client certs should be short lived or ephem. (1 week?)
        params.not_before = date_time_ymd(1975, 1, 1);
        params.not_after = date_time_ymd(4096, 1, 1);
        params.distinguished_name = name;
        // The client auth expects a subject alt name extension, even though it
        // ignores it...
        // TODO(phlip9): is there something cleaner we can do here?
        params.subject_alt_names =
            vec![SanType::DnsName("example.com".to_owned())];

        Ok(Self(rcgen::Certificate::from_params(params)?))
    }

    /// Serialize and sign the client cert. The client cert is signed by the
    /// client CA.
    pub fn serialize_der_signed(
        &self,
        ca: &CaCert,
    ) -> Result<Vec<u8>, RcgenError> {
        self.0.serialize_der_with_signer(&ca.0)
    }

    pub fn serialize_key_der(&self) -> Vec<u8> {
        self.0.serialize_private_key_der()
    }
}

// -- impl NodeCert -- //

impl NodeCert {
    /// Node certs need to bind to the DNS name we serve them from.
    pub fn from_key_pair(
        key_pair: rcgen::KeyPair,
        dns_names: Vec<String>,
    ) -> Result<Self, RcgenError> {
        let mut name = lexe_distinguished_name_prefix();
        name.push(DnType::CommonName, "node cert");

        let subject_alt_names = dns_names
            .into_iter()
            .map(SanType::DnsName)
            .collect::<Vec<_>>();

        let mut params = CertificateParams::default();
        params.alg = &rcgen::PKCS_ED25519;
        params.key_pair = Some(ed25519::verify_compatible(key_pair)?);
        // no expiration
        // TODO(phlip9): node certs should be short lived or ephemeral (1 week?)
        params.not_before = date_time_ymd(1975, 1, 1);
        params.not_after = date_time_ymd(4096, 1, 1);
        params.distinguished_name = name;
        params.subject_alt_names = subject_alt_names;

        Ok(Self(rcgen::Certificate::from_params(params)?))
    }

    /// Serialize and sign the node cert. The node cert is signed by the
    /// client CA.
    pub fn serialize_der_signed(
        &self,
        ca: &CaCert,
    ) -> Result<Vec<u8>, RcgenError> {
        self.0.serialize_der_with_signer(&ca.0)
    }

    pub fn serialize_key_der(&self) -> Vec<u8> {
        self.0.serialize_private_key_der()
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use futures::future::join;
    use secrecy::Secret;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::rustls;

    use super::*;
    use crate::types::RootSeed;

    #[test]
    fn test_certs_parse_successfully() {
        let ca_key_pair = ed25519::from_seed(&[0x11; 32]);
        let ca_cert = CaCert::from_key_pair(ca_key_pair).unwrap();
        let ca_cert_der = ca_cert.serialize_der_signed().unwrap();

        let _ = webpki::TrustAnchor::try_from_cert_der(&ca_cert_der).unwrap();

        let client_key_pair = ed25519::from_seed(&[0x22; 32]);
        let client_cert = ClientCert::from_key_pair(client_key_pair).unwrap();
        let client_cert_der =
            client_cert.serialize_der_signed(&ca_cert).unwrap();

        let _ = webpki::EndEntityCert::try_from(client_cert_der.as_slice())
            .unwrap();

        let node_key_pair = ed25519::from_seed(&[0x33; 32]);
        let node_names = vec!["example.node.lexe.tech".to_owned()];
        let node_cert =
            NodeCert::from_key_pair(node_key_pair, node_names).unwrap();
        let node_cert_der = node_cert.serialize_der_signed(&ca_cert).unwrap();

        let _ =
            webpki::EndEntityCert::try_from(node_cert_der.as_slice()).unwrap();
    }

    #[test]
    fn test_tls_handshake_succeeds() {
        // a fake pair of connected streams
        let (client_stream, server_stream) = duplex(4096);

        let seed = [0x42; 32];
        let dns_name = "example.com";

        // client tries to connect
        let client = async move {
            // should be able to independently derive CA key pair
            let seed = RootSeed::new(Secret::new(seed));
            let ca_key_pair = seed.derive_client_ca_key_pair();
            let ca_cert = CaCert::from_key_pair(ca_key_pair).unwrap();
            let ca_cert_der = ca_cert.serialize_der_signed().unwrap();

            let client_key_pair = ed25519::from_seed(&[0x69; 32]);
            let client_cert =
                ClientCert::from_key_pair(client_key_pair).unwrap();
            let client_cert_der =
                client_cert.serialize_der_signed(&ca_cert).unwrap();
            let client_key_der = client_cert.serialize_key_der();

            let mut certs = rustls::RootCertStore::empty();
            certs.add(&rustls::Certificate(ca_cert_der)).unwrap();

            let config = rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(certs)
                .with_single_cert(
                    vec![rustls::Certificate(client_cert_der)],
                    rustls::PrivateKey(client_key_der),
                )
                .unwrap();

            let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
            let sni = rustls::ServerName::try_from(dns_name).unwrap();
            let mut stream =
                connector.connect(sni, client_stream).await.unwrap();

            stream.write_all(b"hello").await.unwrap();
            stream.flush().await.unwrap();
            stream.shutdown().await.unwrap();

            let mut resp = Vec::new();
            stream.read_to_end(&mut resp).await.unwrap();

            assert_eq!(&resp, b"goodbye");
        };

        // node should accept handshake
        let node = async move {
            // should be able to independently derive CA key pair
            let seed = RootSeed::new(Secret::new(seed));
            let ca_key_pair = seed.derive_client_ca_key_pair();
            let ca_cert = CaCert::from_key_pair(ca_key_pair).unwrap();
            let ca_cert_der = ca_cert.serialize_der_signed().unwrap();

            let node_key_pair = ed25519::from_seed(&[0xf0; 32]);
            let dns_names = vec![dns_name.to_owned()];
            let node_cert =
                NodeCert::from_key_pair(node_key_pair, dns_names).unwrap();
            let node_cert_der =
                node_cert.serialize_der_signed(&ca_cert).unwrap();
            let node_key_der = node_cert.serialize_key_der();

            let mut certs = rustls::RootCertStore::empty();
            certs.add(&rustls::Certificate(ca_cert_der)).unwrap();

            let client_verifier =
                rustls::server::AllowAnyAuthenticatedClient::new(certs);
            let config = rustls::ServerConfig::builder()
                .with_safe_defaults()
                // .with_no_client_auth()
                .with_client_cert_verifier(client_verifier)
                .with_single_cert(
                    vec![rustls::Certificate(node_cert_der)],
                    rustls::PrivateKey(node_key_der),
                )
                .unwrap();

            let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
            let mut stream = acceptor.accept(server_stream).await.unwrap();

            let mut req = Vec::new();
            stream.read_to_end(&mut req).await.unwrap();

            assert_eq!(&req, b"hello");

            stream.write_all(b"goodbye").await.unwrap();
            stream.shutdown().await.unwrap();
        };

        futures::executor::block_on(join(client, node));
    }
}
