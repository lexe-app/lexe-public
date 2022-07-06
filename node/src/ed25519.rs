use ring::signature::KeyPair as _;

pub fn from_seed(seed: &[u8; 32]) -> rcgen::KeyPair {
    let key_pair =
        ring::signature::Ed25519KeyPair::from_seed_unchecked(seed.as_slice())
            .expect(
                "This should never fail, as the secret is exactly 32 bytes",
            );
    let key_bytes = seed.as_slice();
    let pubkey_bytes = key_pair.public_key().as_ref();
    let pkcs8_bytes = serialize_pkcs8(key_bytes, pubkey_bytes);

    rcgen::KeyPair::try_from(pkcs8_bytes).expect(
        "Deserializing a freshly serialized ed25519 key pair should never fail",
    )
}

// $ hexdump -C ring/src/ec/curve25519/ed25519/ed25519_pkcs8_v2_template.der
// 00000000  30 53 02 01 01 30 05 06  03 2b 65 70 04 22 04 20
// 00000010  a1 23 03 21 00

const PKCS_TEMPLATE_PREFIX: &[u8] = &[
    0x30, 0x53, 0x02, 0x01, 0x01, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70,
    0x04, 0x22, 0x04, 0x20,
];
const PKCS_TEMPLATE_MIDDLE: &[u8] = &[0xa1, 0x23, 0x03, 0x21, 0x00];
const PKCS_TEMPLATE_KEY_IDX: usize = 16;

/// Formats a private key as `prefix || key || middle || pubkey`, where `prefix`
/// and `middle` are two pre-computed blobs.
///
/// Note: adapted from `ring`, which doesn't let you serialize as pkcs#8 via
/// any public API...
fn serialize_pkcs8(private_key: &[u8], public_key: &[u8]) -> Vec<u8> {
    let len = PKCS_TEMPLATE_PREFIX.len()
        + private_key.len()
        + PKCS_TEMPLATE_MIDDLE.len()
        + public_key.len();
    let mut out = vec![0u8; len];
    let key_start_idx = PKCS_TEMPLATE_KEY_IDX;

    let prefix = PKCS_TEMPLATE_PREFIX;
    let middle = PKCS_TEMPLATE_MIDDLE;

    let key_end_idx = key_start_idx + private_key.len();
    out[..key_start_idx].copy_from_slice(prefix);
    out[key_start_idx..key_end_idx].copy_from_slice(private_key);
    out[key_end_idx..(key_end_idx + middle.len())].copy_from_slice(middle);
    out[(key_end_idx + middle.len())..].copy_from_slice(public_key);

    out
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;
    use ring::signature::{
        Ed25519KeyPair, EdDSAParameters, VerificationAlgorithm,
    };

    use super::*;

    #[test]
    fn test_serialize_pkcs8() {
        let seed = [0x42; 32];
        let key_pair1 =
            Ed25519KeyPair::from_seed_unchecked(seed.as_slice()).unwrap();
        let pubkey_bytes: &[u8] = key_pair1.public_key().as_ref();

        let key_pair1_bytes =
            serialize_pkcs8(seed.as_slice(), key_pair1.public_key().as_ref());

        let key_pair2 = Ed25519KeyPair::from_pkcs8(&key_pair1_bytes).unwrap();

        let msg: &[u8] = b"hello, world".as_slice();
        let sig = key_pair2.sign(msg);
        let sig_bytes: &[u8] = sig.as_ref();

        EdDSAParameters
            .verify(pubkey_bytes.into(), msg.into(), sig_bytes.into())
            .unwrap();
    }

    #[test]
    fn test_from_seed() {
        proptest!(|(seed in any::<[u8; 32]>())| {
            // should never panic
            let key_pair = crate::ed25519::from_seed(&seed);
            assert!(key_pair.is_compatible(&rcgen::PKCS_ED25519));
        })
    }
}
