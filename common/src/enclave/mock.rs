//! Mock sealing implementation. It just samples a fresh key for every sealing
//! operation and stores the key adjacent to the ciphertext.
//!
//! NOTE: this does not provide any security whatsoever.

use std::borrow::Cow;

use ring::{
    aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM},
    hkdf::{self, HKDF_SHA256},
};

use super::{MOCK_MEASUREMENT, MOCK_SIGNER};
use crate::{
    enclave::{Error, MachineId, Measurement, Sealed, MOCK_MACHINE_ID},
    hex,
    rng::{Crng, RngExt},
};

struct MockKeyRequest {
    keyid: [u8; 32],
}

impl MockKeyRequest {
    fn gen_sealing_request(mut rng: &mut dyn Crng) -> Self {
        let keyid = rng.gen_bytes();
        Self { keyid }
    }

    fn as_bytes(&self) -> &[u8] {
        self.keyid.as_slice()
    }

    fn try_from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let keyid = <[u8; 32]>::try_from(bytes)
            .map_err(|_| Error::InvalidKeyRequestLength)?;
        Ok(Self { keyid })
    }

    fn derive_key(&self, label: &[u8]) -> LessSafeKey {
        LessSafeKey::new(UnboundKey::from(
            hkdf::Salt::new(HKDF_SHA256, &[0x42; 32])
                .extract(self.keyid.as_slice())
                .expand(&[label], &AES_256_GCM)
                .expect("Failed to derive sealing key from key material"),
        ))
    }
}

pub fn seal(
    rng: &mut dyn Crng,
    label: &[u8],
    data: Cow<'_, [u8]>,
) -> Result<Sealed<'static>, Error> {
    let keyrequest = MockKeyRequest::gen_sealing_request(rng);
    let key = keyrequest.derive_key(label);
    let mut ciphertext = data.into_owned();
    let nonce = Nonce::assume_unique_for_key([0u8; 12]);
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| Error::SealInputTooLarge)?;
    Ok(Sealed {
        keyrequest: keyrequest.as_bytes().to_vec().into(),
        ciphertext: Cow::Owned(ciphertext),
    })
}

pub fn unseal(label: &[u8], sealed: Sealed<'_>) -> Result<Vec<u8>, Error> {
    let keyrequest = MockKeyRequest::try_from_bytes(&sealed.keyrequest)?;
    let key = keyrequest.derive_key(label);
    let nonce = Nonce::assume_unique_for_key([0u8; 12]);

    let mut ciphertext = sealed.ciphertext.into_owned();
    let plaintext_ref = key
        .open_in_place(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| Error::UnsealDecryptionError)?;
    let plaintext_len = plaintext_ref.len();

    // unsealing happens in-place. set the length of the now decrypted
    // ciphertext blob and return that.
    ciphertext.truncate(plaintext_len);
    Ok(ciphertext)
}

/// A dev measurement, only applicable to non-SGX dev builds, which allows
/// nearly-identical local binaries to have a differing measurements which are
/// also accessible at run time, without the need to wire through CLI args.
const DEV_MEASUREMENT: Option<Measurement> =
    match option_env!("DEV_MEASUREMENT") {
        // Panics at compile time if DEV_MEASUREMENT isn't valid [u8; 32] hex
        Some(hex) => Some(Measurement::new(hex::decode_const(hex.as_bytes()))),
        // Option::map is not const
        None => None,
    };

/// Prefers [`DEV_MEASUREMENT`], otherwise defaults to [`MOCK_MEASUREMENT`].
pub fn measurement() -> Measurement {
    DEV_MEASUREMENT.unwrap_or(MOCK_MEASUREMENT)
}

pub fn signer() -> Measurement {
    MOCK_SIGNER
}

pub fn machine_id() -> MachineId {
    MOCK_MACHINE_ID
}
