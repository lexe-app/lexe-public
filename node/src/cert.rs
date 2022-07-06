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
        params.subject_alt_names = Vec::new();

        Ok(Self(rcgen::Certificate::from_params(params)?))
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
}
