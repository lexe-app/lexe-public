//! ECDSA P-256 key pairs, used for webpki TLS certificates.
//!
//! Sadly `ed25519` certs aren't widely supported in webpki TLS yet, so we have
//! to use P-256 keys for now.

use base64::Engine;
use ring::{
    rand::SystemRandom,
    signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING},
};
use rustls::pki_types::pem::PemObject;
use secrecy::{ExposeSecret, Secret};
use thiserror::Error;

/// An ECDSA P-256 key pair.
pub struct KeyPair {
    key_pair: EcdsaKeyPair,
    pkcs8_bytes: Secret<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("derived public key doesn't match expected public key")]
    PublicKeyMismatch,

    #[error("failed deserializing PKCS#8-encoded key pair")]
    KeyDeserializeError,
}

// --- impl KeyPair --- //

impl KeyPair {
    pub fn from_sysrng() -> Result<Self, Error> {
        // We can't impl ring::rand::SecureRandom for any of our rngs because
        // it's a sealed trait...
        let rng = SystemRandom::new();
        // The ring API here is pretty wacky, but whatever.
        let pkcs8_document =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng)
                .map_err(|_| Error::PublicKeyMismatch)?;
        Self::deserialize_pkcs8_der(pkcs8_document.as_ref())
    }

    pub fn deserialize_pkcs8_der(bytes: &[u8]) -> Result<Self, Error> {
        // We can't impl ring::rand::SecureRandom for any of our rngs because
        // it's a sealed trait...
        let rng = SystemRandom::new();
        let key_pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_ASN1_SIGNING,
            bytes,
            &rng,
        )
        .map_err(|_| Error::KeyDeserializeError)?;
        let pkcs8_bytes = Secret::new(bytes.to_vec());
        Ok(Self {
            key_pair,
            pkcs8_bytes,
        })
    }

    pub fn into_ring(self) -> EcdsaKeyPair {
        self.key_pair
    }

    pub fn into_pkcs8_der(self) -> Secret<Vec<u8>> {
        self.pkcs8_bytes
    }

    pub fn as_pkcs8_der(&self) -> &[u8] {
        self.pkcs8_bytes.expose_secret()
    }

    pub fn serialize_pkcs8_pem(&self) -> Secret<String> {
        // Intentionally over-allocate to avoid reallocs (and thus leave secrets
        // smeared around the heap).
        let mut pem = String::with_capacity(512);

        pem.push_str("-----BEGIN PRIVATE KEY-----\n");
        base64::engine::general_purpose::STANDARD
            .encode_string(self.pkcs8_bytes.expose_secret(), &mut pem);
        pem.push_str("\n-----END PRIVATE KEY-----\n");

        Secret::new(pem)
    }

    pub fn deserialize_pkcs8_pem(pem: &[u8]) -> Result<Self, Error> {
        let der = rustls::pki_types::PrivatePkcs8KeyDer::from_pem_slice(pem)
            .map_err(|_| Error::KeyDeserializeError)?;
        Self::deserialize_pkcs8_der(der.secret_pkcs8_der())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_keypair_pkcs8_der_roundtrip() {
        let keypair_1 = KeyPair::from_sysrng().unwrap();
        let pkcs8_1 = keypair_1.as_pkcs8_der();
        let keypair_2 = KeyPair::deserialize_pkcs8_der(pkcs8_1).unwrap();
        let pkcs8_2 = keypair_2.as_pkcs8_der();
        assert_eq!(pkcs8_1, pkcs8_2);
    }

    #[test]
    fn test_keypair_pkcs8_pem_roundtrip() {
        let keypair_1 = KeyPair::from_sysrng().unwrap();
        let pem = keypair_1.serialize_pkcs8_pem();
        let keypair_2 =
            KeyPair::deserialize_pkcs8_pem(pem.expose_secret().as_bytes())
                .unwrap();

        let pkcs8_1 = keypair_1.as_pkcs8_der();
        let pkcs8_2 = keypair_2.as_pkcs8_der();
        assert_eq!(pkcs8_1, pkcs8_2);
    }
}
