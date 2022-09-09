use std::fmt;
use std::str::FromStr;

use anyhow::format_err;
use bitcoin::secp256k1::{PublicKey, Secp256k1};
use bitcoin::util::bip32::{ChildNumber, ExtendedPrivKey};
use bitcoin::{KeyPair, Network};
use rand_core::{CryptoRng, RngCore};
use secrecy::{ExposeSecret, Secret, SecretVec};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::rng::Crng;
use crate::{ed25519, hex};

/// The user's root seed from which we derive all child secrets.
pub struct RootSeed(Secret<[u8; Self::LENGTH]>);

impl RootSeed {
    pub const LENGTH: usize = 32;

    /// An HKDF can't extract more than `255 * hash_output_size` bytes for a
    /// single secret.
    const HKDF_MAX_OUT_LEN: usize = 8160 /* 255*32 */;

    /// The HKDF domain separation value as a human-readable byte string.
    #[cfg(test)]
    const HKDF_SALT_STR: &'static [u8] = b"LEXE-HASH-REALM::RootSeed";

    /// We salt the HKDF for domain separation purposes. The raw bytes here are
    /// equal to the hash value: `SHA-256(b"LEXE-HASH-REALM::RootSeed")`.
    const HKDF_SALT: [u8; 32] = hex::decode_const(
        b"363b116be1690fcd481f2d4014812aaecff2411b861198eec42c6e31d80a28a4",
    );

    pub fn new(bytes: Secret<[u8; Self::LENGTH]>) -> Self {
        Self(bytes)
    }

    pub fn from_rng<R>(rng: &mut R) -> Self
    where
        R: RngCore + CryptoRng,
    {
        let mut seed = [0u8; Self::LENGTH];
        rng.fill_bytes(&mut seed);
        Self(Secret::new(seed))
    }

    fn extract(&self) -> ring::hkdf::Prk {
        let salted_hkdf = ring::hkdf::Salt::new(
            ring::hkdf::HKDF_SHA256,
            Self::HKDF_SALT.as_slice(),
        );
        salted_hkdf.extract(self.0.expose_secret().as_slice())
    }

    /// Derive a new child secret with `label` into a prepared buffer `out`.
    pub fn derive_to_slice(&self, label: &[u8], out: &mut [u8]) {
        struct OkmLength(usize);

        impl ring::hkdf::KeyType for OkmLength {
            fn len(&self) -> usize {
                self.0
            }
        }

        assert!(out.len() <= Self::HKDF_MAX_OUT_LEN);

        let label = &[label];

        self.extract()
            .expand(label, OkmLength(out.len()))
            .expect("should not fail")
            .fill(out)
            .expect("should not fail")
    }

    /// Derive a new child secret with `label` to a hash-output-sized buffer.
    pub fn derive(&self, label: &[u8]) -> Secret<[u8; 32]> {
        let mut out = [0u8; 32];
        self.derive_to_slice(label, &mut out);
        Secret::new(out)
    }

    /// Convenience method to derive a new child secret with `label` into a
    /// `Vec<u8>` of size `out_len`.
    pub fn derive_vec(&self, label: &[u8], out_len: usize) -> SecretVec<u8> {
        let mut out = vec![0u8; out_len];
        self.derive_to_slice(label, &mut out);
        SecretVec::new(out)
    }

    /// Derive the CA cert that endorses client and node certs. These certs
    /// provide mutual authentication for client <-> node connections.
    pub fn derive_client_ca_key_pair(&self) -> rcgen::KeyPair {
        let seed = self.derive(b"client ca key pair");
        ed25519::KeyPair::from_seed(seed.expose_secret()).to_rcgen()
    }

    /// Derive the lightning node key pair directly, without needing to derive
    /// all the other auxiliary node secrets.
    pub fn derive_node_key_pair<R: Crng>(&self, rng: &mut R) -> KeyPair {
        // NOTE: this doesn't affect the output; this randomizes the SECP256K1
        // context for sidechannel resistance.
        let mut secp_randomize = [0u8; 32];
        rng.fill_bytes(&mut secp_randomize);
        let mut secp_ctx = Secp256k1::new();
        secp_ctx.seeded_randomize(&secp_randomize);

        let master_secret =
            ExtendedPrivKey::new_master(Network::Testnet, self.expose_secret())
                .expect("should never fail; the sizes match up");
        let child_number = ChildNumber::from_hardened_idx(0)
            .expect("should never fail; index is in range");
        let node_sk = master_secret
            .ckd_priv(&secp_ctx, child_number)
            .expect("should never fail")
            .private_key;
        KeyPair::from_secret_key(&secp_ctx, node_sk)
    }

    /// Derive the Lightning node pubkey.
    pub fn derive_node_pk<R: Crng>(&self, rng: &mut R) -> PublicKey {
        PublicKey::from(self.derive_node_key_pair(rng))
    }

    #[cfg(test)]
    fn as_bytes(&self) -> &[u8] {
        self.0.expose_secret().as_slice()
    }
}

impl ExposeSecret<[u8; Self::LENGTH]> for RootSeed {
    fn expose_secret(&self) -> &[u8; Self::LENGTH] {
        self.0.expose_secret()
    }
}

impl FromStr for RootSeed {
    type Err = hex::DecodeError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; Self::LENGTH];
        hex::decode_to_slice_ct(hex, bytes.as_mut_slice())
            .map(|()| Self::new(Secret::new(bytes)))
    }
}

impl fmt::Debug for RootSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        f.write_str("RootSeed(..)")
    }
}

impl TryFrom<&[u8]> for RootSeed {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::LENGTH {
            return Err(format_err!("input must be {} bytes", Self::LENGTH));
        }
        let mut out = [0u8; Self::LENGTH];
        out[..].copy_from_slice(bytes);
        Ok(Self::new(Secret::new(out)))
    }
}

struct RootSeedVisitor;

impl<'de> de::Visitor<'de> for RootSeedVisitor {
    type Value = RootSeed;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("hex-encoded RootSeed or raw bytes")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        RootSeed::from_str(v).map_err(serde::de::Error::custom)
    }

    fn visit_bytes<E>(self, b: &[u8]) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        RootSeed::try_from(b).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RootSeed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(RootSeedVisitor)
        } else {
            deserializer.deserialize_bytes(RootSeedVisitor)
        }
    }
}

impl Serialize for RootSeed {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            let hex_str = hex::encode(self.0.expose_secret());
            serializer.serialize_str(&hex_str)
        } else {
            serializer.serialize_bytes(self.0.expose_secret())
        }
    }
}

#[cfg(test)]
impl proptest::arbitrary::Arbitrary for RootSeed {
    type Strategy = proptest::strategy::BoxedStrategy<Self>;
    type Parameters = ();

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use proptest::strategy::Strategy;

        proptest::arbitrary::any::<[u8; 32]>()
            .prop_map(|buf| Self::new(Secret::new(buf)))
            .no_shrink()
            .boxed()
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::collection::vec;
    use proptest::proptest;

    use super::*;
    use crate::sha256;

    // simple implementations of some crypto functions for equivalence testing

    // an inefficient impl of HMAC-SHA256 for equivalence testing
    fn hmac_sha256(key: &[u8], msg: &[u8]) -> sha256::Hash {
        let h_key = sha256::digest(key);
        let mut zero_pad_key = [0u8; 64];

        // make key match the internal block size
        let key = match key.len() {
            len if len > 64 => h_key.as_ref(),
            _ => key,
        };
        zero_pad_key[..key.len()].copy_from_slice(key);
        let key = zero_pad_key.as_slice();
        assert_eq!(key.len(), 64);

        // o_key := [ key_i ^ 0x5c ]_{i in 0..64}
        let mut o_key = [0u8; 64];
        for (o_key_i, key_i) in o_key.iter_mut().zip(key) {
            *o_key_i = key_i ^ 0x5c;
        }

        // i_key := [ key_i ^ 0x36 ]_{i in 0..64}
        let mut i_key = [0u8; 64];
        for (i_key_i, key_i) in i_key.iter_mut().zip(key) {
            *i_key_i = key_i ^ 0x36;
        }

        // h_i := H(i_key || msg)
        let h_i = sha256::digest_many(&[&i_key, msg]);

        // output := H(o_key || H(i_key || msg))
        sha256::digest_many(&[&o_key, h_i.as_ref()])
    }

    // an inefficient impl of HKDF-SHA256 for equivalence testing
    fn hkdf_sha256(
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        out_len: usize,
    ) -> Vec<u8> {
        let prk = hmac_sha256(salt, ikm);

        // N := ceil(out_len / block_size)
        //   := (out_len.saturating_sub(1) / block_size) + 1
        let n = (out_len.saturating_sub(1) / 32) + 1;
        let n = u8::try_from(n).expect("out_len too large");

        // T := T(1) | T(2) | .. | T(N)
        // T(0) := b"" (empty byte string)
        // T(i+1) := hmac_sha256(prk, T(i) || info || [ i+1 ])

        let mut t_i = [0u8; 32];
        let mut out = Vec::new();

        for i in 1..=n {
            // m_i := T(i-1) || info || [ i ]
            let mut m_i = if i == 1 { Vec::new() } else { t_i.to_vec() };
            m_i.extend_from_slice(info);
            m_i.extend_from_slice(&[i]);

            let h_i = hmac_sha256(prk.as_ref(), &m_i);
            t_i.copy_from_slice(h_i.as_ref());

            if i < n {
                out.extend_from_slice(&t_i[..]);
            } else {
                let l = 32 - (((n as usize) * 32) - out_len);
                out.extend_from_slice(&t_i[..l]);
            }
        }

        out
    }

    #[test]
    fn test_root_seed_serde() {
        let input =
            "7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069";
        let input_json = format!("\"{input}\"");
        let seed_bytes = hex::decode(input).unwrap();

        let seed = RootSeed::from_str(input).unwrap();
        assert_eq!(seed.as_bytes(), &seed_bytes);

        let seed2: RootSeed = serde_json::from_str(&input_json).unwrap();
        assert_eq!(seed2.as_bytes(), &seed_bytes);

        #[derive(Deserialize)]
        struct Foo {
            x: u32,
            seed: RootSeed,
            y: String,
        }

        let foo_json = format!(
            "{{\n\
            \"x\": 123,\n\
            \"seed\": \"{input}\",\n\
            \"y\": \"asdf\"\n\
        }}"
        );

        let foo2: Foo = serde_json::from_str(&foo_json).unwrap();
        assert_eq!(foo2.x, 123);
        assert_eq!(foo2.seed.as_bytes(), &seed_bytes);
        assert_eq!(foo2.y, "asdf");
    }

    #[test]
    fn test_root_seed_hkdf_salt() {
        let actual = RootSeed::HKDF_SALT.as_slice();
        let expected = sha256::digest(RootSeed::HKDF_SALT_STR);

        // // print out salt
        // let hex = hex::encode(expected.as_ref());
        // let (chunks, _) = hex.as_bytes().as_chunks::<2>();
        // for &[hi, lo] in chunks {
        //     let hi = hi as char;
        //     let lo = lo as char;
        //     println!("0x{hi}{lo},");
        // }

        // compare hex encode for easier debugging
        assert_eq!(hex::encode(actual), hex::encode(expected.as_ref()));
    }

    #[test]
    fn test_root_seed_derive() {
        let seed = RootSeed::new(Secret::new([0x42; 32]));

        let out8 = seed.derive_vec(b"very cool secret", 8);
        let out16 = seed.derive_vec(b"very cool secret", 16);
        let out32 = seed.derive_vec(b"very cool secret", 32);
        let out32_2 = seed.derive(b"very cool secret");

        assert_eq!("49fb6bebcd2acb22", hex::encode(out8.expose_secret()));
        assert_eq!(
            "49fb6bebcd2acb223a802f726bd5159d",
            hex::encode(out16.expose_secret())
        );
        assert_eq!(
            "49fb6bebcd2acb223a802f726bd5159d4c982732c550c698aa0558f95575e8c1",
            hex::encode(out32.expose_secret())
        );
        assert_eq!(out32.expose_secret(), out32_2.expose_secret());
    }

    // Fuzz our KDF against a basic, readable implementation of HKDF-SHA256.
    #[test]
    fn test_root_seed_derive_equiv() {
        let arb_seed = any::<RootSeed>();
        let arb_label = vec(any::<u8>(), 0..=64);
        let arb_len = 0_usize..=1024;

        proptest!(|(seed in arb_seed, label in arb_label, len in arb_len)| {
            let expected = hkdf_sha256(
                seed.as_bytes(),
                RootSeed::HKDF_SALT.as_slice(),
                &label,
                len,
            );

            let actual = seed.derive_vec(&label, len);

            assert_eq!(&expected, actual.expose_secret());
        });
    }
}
