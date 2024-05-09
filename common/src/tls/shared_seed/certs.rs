//! Contains the CA cert and end-entity certs for "shared seed" mTLS.

use crate::{
    ed25519,
    rng::Crng,
    root_seed::RootSeed,
    tls::{
        self,
        types::{LxCertificateDer, LxPrivateKeyDer, LxPrivateKeyDerKind},
    },
};

/// The derived CA cert used as the trust anchor for both client and server.
///
/// The keypair for this CA cert is derived from the shared [`RootSeed`], and
/// the cert itself is deterministically derived from the cert keypair.
/// Thus, both the client and server can independently derive a shared trust
/// root after the [`RootSeed`] has been provisioned.
pub struct SharedSeedCaCert(rcgen::Certificate);

/// The end-entity cert used by the client. Signed by the CA cert.
///
/// The key pair for the client cert is sampled.
pub struct SharedSeedClientCert(rcgen::Certificate);

/// The end-entity cert used by the server. Signed by the CA cert.
///
/// The key pair for the server cert is sampled.
pub struct SharedSeedServerCert(rcgen::Certificate);

impl SharedSeedCaCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe shared seed CA cert";

    /// Deterministically derive the shared seed CA cert from the [`RootSeed`].
    pub fn from_root_seed(root_seed: &RootSeed) -> Self {
        let key_pair = root_seed.derive_shared_seed_tls_ca_key_pair();
        // We want the cert to be deterministic, so no expiration
        let not_before = rcgen::date_time_ymd(1975, 1, 1);
        let not_after = rcgen::date_time_ymd(4096, 1, 1);

        Self(tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            tls::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            &key_pair,
            |params: &mut rcgen::CertificateParams| {
                // This is a CA cert, and there should be 0 intermediate certs.
                params.is_ca =
                    rcgen::IsCa::Ca(rcgen::BasicConstraints::Constrained(0));
            },
        ))
    }

    /// DER-encode and self-sign the CA cert.
    pub fn serialize_der_self_signed(
        &self,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.0.serialize_der().map(LxCertificateDer::from)
    }
}

impl SharedSeedClientCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe shared seed client cert";

    /// Generate an ephemeral client cert with a randomly-sampled keypair.
    pub fn generate_from_rng(rng: &mut impl Crng) -> Self {
        let key_pair = ed25519::KeyPair::from_rng(rng);
        let now = time::OffsetDateTime::now_utc();
        let not_before = now - time::Duration::HOUR;
        // TODO(max): We want ephemeral cert lifetimes (+-1 hour), but some
        // nodes might live longer than that, causing TLS handshakes to fail.
        // We use a long default for now (90 days) until automatic cert rotation
        // is implemented. Most likely design is to spawn a tokio task that
        // regenerates the cert every once in a while, then `ArcSwap`s the new
        // cert into the `ResolvesClientCert`/`ResolvesServerCert` resolver used
        // on both the client and server side.
        let not_after = now + (90 * time::Duration::DAY);
        // let not_after = now + time::Duration::HOUR;

        Self(tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            // Client auth fails without a SAN, even though it is ignored..
            tls::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            &key_pair,
            |_| (),
        ))
    }

    /// DER-encode the cert and sign it using the CA cert.
    pub fn serialize_der_ca_signed(
        &self,
        ca_cert: &SharedSeedCaCert,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.0
            .serialize_der_with_signer(&ca_cert.0)
            .map(LxCertificateDer::from)
    }

    /// DER-encode the cert's private key.
    pub fn serialize_key_der(&self) -> LxPrivateKeyDer {
        let kind = LxPrivateKeyDerKind::Pkcs8;
        let der_bytes = self.0.serialize_private_key_der();
        LxPrivateKeyDer::new(kind, der_bytes)
    }
}

impl SharedSeedServerCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe shared seed server cert";

    /// Generate an ephemeral server cert with a randomly-sampled keypair.
    pub fn from_rng(rng: &mut impl Crng, dns_name: String) -> Self {
        let key_pair = ed25519::KeyPair::from_rng(rng);
        let now = time::OffsetDateTime::now_utc();
        let not_before = now - time::Duration::HOUR;
        // TODO(max): We want ephemeral cert lifetimes (+-1 hour), but some
        // nodes might live longer than that, causing TLS handshakes to fail.
        // We use a long default for now (90 days) until automatic cert rotation
        // is implemented. Most likely design is to spawn a tokio task that
        // regenerates the cert every once in a while, then `ArcSwap`s the new
        // cert into the `ResolvesClientCert`/`ResolvesServerCert` resolver used
        // on both the client and server side.
        let not_after = now + (90 * time::Duration::DAY);
        let subject_alt_names = vec![rcgen::SanType::DnsName(dns_name)];

        Self(tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            subject_alt_names,
            &key_pair,
            |_| (),
        ))
    }

    /// DER-encode the cert and sign it using the CA cert.
    pub fn serialize_der_ca_signed(
        &self,
        ca_cert: &SharedSeedCaCert,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.0
            .serialize_der_with_signer(&ca_cert.0)
            .map(LxCertificateDer::from)
    }

    /// DER-encode the cert's private key.
    pub fn serialize_key_der(&self) -> LxPrivateKeyDer {
        let kind = LxPrivateKeyDerKind::Pkcs8;
        let der_bytes = self.0.serialize_private_key_der();
        LxPrivateKeyDer::new(kind, der_bytes)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::rng::WeakRng;

    #[test]
    fn test_certs_parse_successfully() {
        let mut rng = WeakRng::from_u64(20240215);
        let root_seed = RootSeed::from_rng(&mut rng);
        let ca_cert = SharedSeedCaCert::from_root_seed(&root_seed);
        let ca_cert_der = ca_cert.serialize_der_self_signed().unwrap();

        let _ = webpki::TrustAnchor::try_from_cert_der(ca_cert_der.as_bytes())
            .unwrap();

        let client_cert = SharedSeedClientCert::generate_from_rng(&mut rng);
        let client_cert_der =
            client_cert.serialize_der_ca_signed(&ca_cert).unwrap();

        let _ = webpki::EndEntityCert::try_from(client_cert_der.as_bytes())
            .unwrap();

        let dns_name = "run.lexe.app".to_owned();
        let server_cert = SharedSeedServerCert::from_rng(&mut rng, dns_name);
        let server_cert_der =
            server_cert.serialize_der_ca_signed(&ca_cert).unwrap();

        let _ = webpki::EndEntityCert::try_from(server_cert_der.as_bytes())
            .unwrap();
    }
}
