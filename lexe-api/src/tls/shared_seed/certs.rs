//! Contains the CA cert and end-entity certs for "shared seed" mTLS.

use common::{ed25519, rng::Crng, root_seed::RootSeed};

use crate::tls::{
    self,
    types::{LxCertificateDer, LxPrivatePkcs8KeyDer},
};

/// The "ephemeral issuing" CA cert derived from the root seed.
/// The keypair is derived from the root seed.
pub struct EphemeralIssuingCaCert(rcgen::Certificate);

/// The ephemeral end-entity cert used by the client.
/// Signed by the "ephemeral issuing" CA cert.
///
/// The key pair for the client cert is sampled.
pub struct EphemeralClientCert(rcgen::Certificate);

/// The ephemeral end-entity cert used by the server.
/// Signed by the "ephemeral issuing" CA cert.
///
/// The key pair for the server cert is sampled.
pub struct EphemeralServerCert(rcgen::Certificate);

impl EphemeralIssuingCaCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    // TODO(max): Ideally rename this to "Lexe ephemeral issuing CA cert", but
    // need to be careful about backwards compatibility. Both client and server
    // would need to trust the old and new CAs before the old CA can be removed.
    const COMMON_NAME: &'static str = "Lexe shared seed CA cert";

    /// Deterministically derive the CA cert from the [`RootSeed`].
    pub fn from_root_seed(root_seed: &RootSeed) -> Self {
        let key_pair = root_seed.derive_ephemeral_issuing_ca_key_pair();
        // We want the cert to be deterministic, so no expiration
        let not_before = rcgen::date_time_ymd(1975, 1, 1);
        let not_after = rcgen::date_time_ymd(4096, 1, 1);

        Self(tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            tls::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            key_pair.into(),
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
        self.0.serialize_der().map(LxCertificateDer)
    }
}

impl EphemeralClientCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe ephemeral client cert";

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
            key_pair.into(),
            |_| (),
        ))
    }

    /// DER-encode the cert and sign it using the CA cert.
    pub fn serialize_der_ca_signed(
        &self,
        ca_cert: &EphemeralIssuingCaCert,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.0
            .serialize_der_with_signer(&ca_cert.0)
            .map(LxCertificateDer)
    }

    /// DER-encode the cert's private key.
    pub fn serialize_key_der(&self) -> LxPrivatePkcs8KeyDer {
        LxPrivatePkcs8KeyDer(self.0.serialize_private_key_der())
    }
}

impl EphemeralServerCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe ephemeral server cert";

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
            key_pair.into(),
            |_| (),
        ))
    }

    /// DER-encode the cert and sign it using the CA cert.
    pub fn serialize_der_ca_signed(
        &self,
        ca_cert: &EphemeralIssuingCaCert,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.0
            .serialize_der_with_signer(&ca_cert.0)
            .map(LxCertificateDer)
    }

    /// DER-encode the cert's private key.
    pub fn serialize_key_der(&self) -> LxPrivatePkcs8KeyDer {
        LxPrivatePkcs8KeyDer(self.0.serialize_private_key_der())
    }
}

#[cfg(test)]
mod test {
    use common::rng::FastRng;

    use super::*;

    #[test]
    fn test_certs_parse_successfully() {
        let mut rng = FastRng::from_u64(20240215);
        let root_seed = RootSeed::from_rng(&mut rng);
        let ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let ca_cert_der = ca_cert.serialize_der_self_signed().unwrap();

        let _ = webpki::TrustAnchor::try_from_cert_der(ca_cert_der.as_slice())
            .unwrap();

        let client_cert = EphemeralClientCert::generate_from_rng(&mut rng);
        let client_cert_der =
            client_cert.serialize_der_ca_signed(&ca_cert).unwrap();

        let _ = webpki::EndEntityCert::try_from(client_cert_der.as_slice())
            .unwrap();

        let dns_name = "run.lexe.app".to_owned();
        let server_cert = EphemeralServerCert::from_rng(&mut rng, dns_name);
        let server_cert_der =
            server_cert.serialize_der_ca_signed(&ca_cert).unwrap();

        let _ = webpki::EndEntityCert::try_from(server_cert_der.as_slice())
            .unwrap();
    }

    /// Check that the derived CA keypair is the same as a snapshot from the
    /// same [`RootSeed`].
    ///
    /// ```
    /// $ cargo test -p common derived_ca_keypair_snapshot_test -- --show-output
    /// ```
    #[test]
    fn derived_ca_keypair_snapshot_test() {
        let root_seed = RootSeed::from_u64(20240514);
        let derived_keypair = root_seed.derive_ephemeral_issuing_ca_key_pair();
        let derived_keypair_seed = derived_keypair.secret_key();

        let snapshot_keypair_seed = hex::decode(
            "1960322cd55473e9a1bdc5b53f3089dada0f825858b9a4da4ab09f9b1008b46d",
        )
        .unwrap();

        assert_eq!(derived_keypair_seed, snapshot_keypair_seed.as_slice());

        // Uncomment to regenerate
        // let derived_keypair_hex = hex::display(derived_keypair_seed);
        // println!("---");
        // println!("{derived_keypair_hex}");
        // println!("---");
    }

    /// Tests that a freshly derived ephemeral issuing CA cert serialized into
    /// DER is bit-for-bit the same as a snapshot from the same [`RootSeed`].
    ///
    /// ```
    /// $ cargo test -p common ca_cert_snapshot_test -- --show-output
    /// ```
    // Bit-for-bit serialization compatibility is a stronger guarantee than we
    // need - I wrote the test this way to save time. If ephemeral issuing CA
    // cert generation needs to change in a backwards compatible way, update
    // this test so that we only check that *handshakes* between the older and
    // newer certs succeed (which is a bit more annoying to write)
    #[test]
    fn ca_cert_snapshot_test() {
        let snapshot_cert_der = hex::decode("308201ae30820160a00302010202142b404543fa6a1885d7615fd0d3313b0dcaf4b47b300506032b65703050310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703121301f06035504030c184c65786520736861726564207365656420434120636572743020170d3735303130313030303030305a180f34303936303130313030303030305a3050310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703121301f06035504030c184c6578652073686172656420736565642043412063657274302a300506032b6570032100ee71f429ce11f0538aeac1d9fae23ddf4fcf831d1b9e111b8144192a3820dcc7a34a304830130603551d11040c300a82086c6578652e617070301d0603551d0e04160414ab404543fa6a1885d7615fd0d3313b0dcaf4b47b30120603551d130101ff040830060101ff020100300506032b6570034100fbfe35aa1ac3c7548aefda98dd03fb181fc317a41c2fa051d169e89d34a7946a95c288d0cc8591824f758060d1df4288237813f445137c3da90d457aa06ca400").unwrap();

        let root_seed = RootSeed::from_u64(20240514);
        let rederived_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let rederived_cert_der =
            rederived_cert.serialize_der_self_signed().unwrap();

        assert_eq!(rederived_cert_der.as_slice(), snapshot_cert_der.as_slice());

        // Uncomment to regenerate
        // let rederived_cert_hex = hex::display(rederived_cert_der.as_bytes());
        // println!("---");
        // println!("{rederived_cert_hex}");
        // println!("---");
    }
}
