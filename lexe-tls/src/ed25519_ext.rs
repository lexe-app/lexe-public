use base64::Engine as _;
use common::ed25519;
use rustls::pki_types::pem::PemObject;
use secrecy::Zeroize;

pub trait Ed25519KeyPairExt: Sized {
    /// Serialize the [`ed25519::KeyPair`] into a PKCS#8 PEM string.
    fn serialize_pkcs8_pem(&self) -> String;

    /// Deserialize the [`ed25519::KeyPair`] from a PKCS#8 PEM string.
    fn deserialize_pkcs8_pem(pem: &[u8]) -> Result<Self, ed25519::Error>;
}

impl Ed25519KeyPairExt for ed25519::KeyPair {
    fn serialize_pkcs8_pem(&self) -> String {
        let mut der = self.serialize_pkcs8_der();
        // Reserve enough space to always avoid reallocs (and thus avoid secrets
        // smeared around the heap).
        let mut pem = String::with_capacity(171);

        pem.push_str("-----BEGIN PRIVATE KEY-----\n");
        // TODO(phlip9): b64_ct
        base64::engine::general_purpose::STANDARD.encode_string(der, &mut pem);
        pem.push_str("\n-----END PRIVATE KEY-----\n");

        der.zeroize();
        pem
    }

    fn deserialize_pkcs8_pem(pem: &[u8]) -> Result<Self, ed25519::Error> {
        let der = rustls::pki_types::PrivatePkcs8KeyDer::from_pem_slice(pem)
            .map_err(|_| ed25519::Error::KeyDeserializeError)?;
        ed25519::KeyPair::deserialize_pkcs8_der(der.secret_pkcs8_der())
    }
}

#[cfg(test)]
mod test {
    use proptest::proptest;

    use super::*;

    #[test]
    fn test_keypair_pkcs8_pem_roundtrip() {
        proptest!(|(key_1: ed25519::KeyPair)| {
            let pem_1 = key_1.serialize_pkcs8_pem();
            let key_2 = ed25519::KeyPair::deserialize_pkcs8_pem(pem_1.as_bytes()).unwrap();
            let pem_2 = key_2.serialize_pkcs8_pem();
            assert_eq!(key_1.secret_key(), key_2.secret_key());
            assert_eq!(pem_1, pem_2);
        });
    }
}
