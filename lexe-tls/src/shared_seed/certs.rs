//! Contains the CA cert and end-entity certs for "shared seed" mTLS.

use common::{ed25519, rng::Crng, root_seed::RootSeed};

use crate as tls;
use crate::types::{LxCertificateDer, LxPrivatePkcs8KeyDer};

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

/// The "revocable issuing" CA cert derived from the root seed.
pub struct RevocableIssuingCaCert(rcgen::Certificate);

/// The revocable end-entity cert used by the client.
/// Signed by the "revocable issuing" CA cert.
pub struct RevocableClientCert {
    pubkey: ed25519::PublicKey,
    cert: rcgen::Certificate,
}

// --- impl ephemeral certs --- //

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
            crate::DEFAULT_SUBJECT_ALT_NAMES.clone(),
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
            crate::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            &key_pair,
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
            &key_pair,
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

// --- impl revocable --- //

impl RevocableIssuingCaCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe revocable issuing CA cert";

    /// Deterministically derive the CA cert from the [`RootSeed`].
    pub fn from_root_seed(root_seed: &RootSeed) -> Self {
        let key_pair = root_seed.derive_revocable_issuing_ca_key_pair();
        // We want the cert to be deterministic, so no expiration
        let not_before = rcgen::date_time_ymd(1975, 1, 1);
        let not_after = rcgen::date_time_ymd(4096, 1, 1);

        Self(tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            crate::DEFAULT_SUBJECT_ALT_NAMES.clone(),
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
        self.0.serialize_der().map(LxCertificateDer)
    }
}

impl RevocableClientCert {
    /// The Common Name (CN) component of this cert's Distinguished Name (DN).
    const COMMON_NAME: &'static str = "Lexe revocable client cert";

    /// Generate an revocable client cert with a randomly-sampled keypair.
    pub fn generate_from_rng(rng: &mut impl Crng) -> Self {
        let key_pair = ed25519::KeyPair::from_rng(rng);
        let pubkey = *key_pair.public_key();
        // Since the certs are revocable they should also have no hard-coded
        // expiration, so SDK integrators can avoid the hassle of having to
        // rotate their client certs when it is not needed.
        let not_before = rcgen::date_time_ymd(1975, 1, 1);
        let not_after = rcgen::date_time_ymd(4096, 1, 1);

        let cert = tls::build_rcgen_cert(
            Self::COMMON_NAME,
            not_before,
            not_after,
            // Client auth fails without a SAN, even though it is ignored..
            crate::DEFAULT_SUBJECT_ALT_NAMES.clone(),
            &key_pair,
            |_| (),
        );

        Self { cert, pubkey }
    }

    pub fn public_key(&self) -> ed25519::PublicKey {
        self.pubkey
    }

    /// DER-encode the cert and sign it using the CA cert.
    pub fn serialize_der_ca_signed(
        &self,
        ca_cert: &RevocableIssuingCaCert,
    ) -> Result<LxCertificateDer, rcgen::Error> {
        self.cert
            .serialize_der_with_signer(&ca_cert.0)
            .map(LxCertificateDer)
    }

    /// DER-encode the cert's private key.
    pub fn serialize_key_der(&self) -> LxPrivatePkcs8KeyDer {
        LxPrivatePkcs8KeyDer(self.cert.serialize_private_key_der())
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

        let assert_parseable = |cert_der: LxCertificateDer| {
            let _ =
                webpki::EndEntityCert::try_from(cert_der.as_slice()).unwrap();
        };

        let eph_ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let eph_ca_cert_der = eph_ca_cert.serialize_der_self_signed().unwrap();
        assert_parseable(eph_ca_cert_der);

        let eph_client_cert = EphemeralClientCert::generate_from_rng(&mut rng);
        let eph_client_cert_der = eph_client_cert
            .serialize_der_ca_signed(&eph_ca_cert)
            .unwrap();
        assert_parseable(eph_client_cert_der);

        let dns_name = "run.lexe.app".to_owned();
        let eph_server_cert = EphemeralServerCert::from_rng(&mut rng, dns_name);
        let eph_server_cert_der = eph_server_cert
            .serialize_der_ca_signed(&eph_ca_cert)
            .unwrap();
        assert_parseable(eph_server_cert_der);

        let rev_ca_cert = RevocableIssuingCaCert::from_root_seed(&root_seed);
        let rev_ca_cert_der = rev_ca_cert.serialize_der_self_signed().unwrap();
        assert_parseable(rev_ca_cert_der);

        let rev_client_cert = RevocableClientCert::generate_from_rng(&mut rng);
        let rev_client_cert_der = rev_client_cert
            .serialize_der_ca_signed(&rev_ca_cert)
            .unwrap();
        assert_parseable(rev_client_cert_der);
    }

    /// Check that the derived CA keypairs are the same as a snapshot from the
    /// same [`RootSeed`].
    ///
    /// ```
    /// $ cargo test -p lexe-api derived_ca_keypair_snapshot_test -- --show-output
    /// ```
    #[test]
    fn derived_ca_keypair_snapshot_test() {
        let root_seed = RootSeed::from_u64(20240514);

        fn do_keypair_snapshot_test(
            derived_keypair: ed25519::KeyPair,
            snapshot_keypair_seed_hex: &str,
        ) {
            let derived_keypair_seed = derived_keypair.secret_key();

            // Uncomment to regenerate
            // let derived_keypair_hex = hex::display(derived_keypair_seed);
            // println!("---");
            // println!("{derived_keypair_hex}");
            // println!("---");

            let snapshot_keypair_seed =
                hex::decode(snapshot_keypair_seed_hex).unwrap();

            assert_eq!(derived_keypair_seed, snapshot_keypair_seed.as_slice());
        }

        do_keypair_snapshot_test(
            root_seed.derive_ephemeral_issuing_ca_key_pair(),
            "1960322cd55473e9a1bdc5b53f3089dada0f825858b9a4da4ab09f9b1008b46d",
        );

        do_keypair_snapshot_test(
            root_seed.derive_revocable_issuing_ca_key_pair(),
            "79231824b6713a476127f927b03e5ff10bac59184b36aa7716ad423f2b0972fa",
        );
    }

    /// Tests that a freshly derived ephemeral issuing CA cert serialized into
    /// DER is bit-for-bit the same as a snapshot from the same [`RootSeed`].
    ///
    /// ```
    /// $ cargo test -p lexe-api ca_cert_snapshot_test -- --show-output
    /// ```
    // Bit-for-bit serialization compatibility is a stronger guarantee than we
    // need - I wrote the test this way to save time. If ephemeral issuing CA
    // cert generation needs to change in a backwards compatible way, update
    // this test so that we only check that *handshakes* between the older and
    // newer certs succeed (which is a bit more annoying to write)
    #[test]
    fn ca_cert_snapshot_test() {
        let root_seed = RootSeed::from_u64(20240514);

        fn do_ca_cert_snapshot_test(
            derived_cert_der: LxCertificateDer,
            snapshot_cert_der_hex: &str,
        ) {
            // Uncomment to regenerate
            // let derived_cert_hex = hex::display(derived_cert_der.as_slice());
            // println!("---");
            // println!("{derived_cert_hex}");
            // println!("---");

            let snapshot_cert_der = hex::decode(snapshot_cert_der_hex).unwrap();

            assert_eq!(
                derived_cert_der.as_slice(),
                snapshot_cert_der.as_slice(),
            );
        }

        {
            let derived_eph_ca_cert =
                EphemeralIssuingCaCert::from_root_seed(&root_seed);
            let derived_eph_ca_cert_der =
                derived_eph_ca_cert.serialize_der_self_signed().unwrap();
            do_ca_cert_snapshot_test(
                derived_eph_ca_cert_der,
                "308201ae30820160a00302010202142b404543fa6a1885d7615fd0d3313b0dcaf4b47b300506032b65703050310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703121301f06035504030c184c65786520736861726564207365656420434120636572743020170d3735303130313030303030305a180f34303936303130313030303030305a3050310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703121301f06035504030c184c6578652073686172656420736565642043412063657274302a300506032b6570032100ee71f429ce11f0538aeac1d9fae23ddf4fcf831d1b9e111b8144192a3820dcc7a34a304830130603551d11040c300a82086c6578652e617070301d0603551d0e04160414ab404543fa6a1885d7615fd0d3313b0dcaf4b47b30120603551d130101ff040830060101ff020100300506032b6570034100fbfe35aa1ac3c7548aefda98dd03fb181fc317a41c2fa051d169e89d34a7946a95c288d0cc8591824f758060d1df4288237813f445137c3da90d457aa06ca400",
            );
        }

        {
            let derived_rev_ca_cert =
                RevocableIssuingCaCert::from_root_seed(&root_seed);
            let derived_rev_ca_cert_der =
                derived_rev_ca_cert.serialize_der_self_signed().unwrap();

            do_ca_cert_snapshot_test(
                derived_rev_ca_cert_der,
                "308201ba3082016ca0030201020214481939aa2918a50e1c241c466370d9fdbe7d60f5300506032b65703056310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703127302506035504030c1e4c657865207265766f6361626c652069737375696e6720434120636572743020170d3735303130313030303030305a180f34303936303130313030303030305a3056310b300906035504060c025553310b300906035504080c0243413111300f060355040a0c086c6578652d6170703127302506035504030c1e4c657865207265766f6361626c652069737375696e672043412063657274302a300506032b65700321004dec474dad3978c88678c624e9d89fc8739a22aeb411a9889b1720c119b8edc8a34a304830130603551d11040c300a82086c6578652e617070301d0603551d0e04160414481939aa2918a50e1c241c466370d9fdbe7d60f530120603551d130101ff040830060101ff020100300506032b65700341009b399c16f9c4c89a172aa70b29115c3541e5dd959e99d05ddd9bb2ee8764020604a174673893f698494d116c6e85ad9805cf540d0d9b79b73da0916746db3904"
            );
        }
    }
}
