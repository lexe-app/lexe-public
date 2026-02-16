use std::{fmt, fs, io::Write, num::NonZeroU32, path::Path, str::FromStr};

use anyhow::{Context, anyhow, bail, ensure};
use bitcoin::{
    Network,
    bip32::{self, ChildNumber},
    secp256k1,
};
use lexe_std::array;
use secrecy::{ExposeSecret, Secret, SecretVec, Zeroize};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{
    aes::{self, AesMasterKey},
    api::user::{NodePk, UserPk},
    ed25519,
    ln::network::LxNetwork,
    password,
    rng::{Crng, RngExt},
};

// TODO(phlip9): [perf] consider storing extracted `Prk` alongside seed to
//               reduce key derivation time by ~60-70% : )

/// The user's root seed from which we derive all child secrets.
pub struct RootSeed(Secret<[u8; Self::LENGTH]>);

impl RootSeed {
    pub const LENGTH: usize = 32;

    /// An HKDF can't extract more than `255 * hash_output_size` bytes for a
    /// single secret.
    const HKDF_MAX_OUT_LEN: usize = 8160 /* 255*32 */;

    /// We salt the HKDF for domain separation purposes.
    const HKDF_SALT: [u8; 32] = array::pad(*b"LEXE-REALM::RootSeed");

    /// Buffer size for writing a BIP39 mnemonic sentence.
    /// 24 words * max 8 chars + 23 spaces = 215 <= 216 bytes max
    const BIP39_MNEMONIC_BUF_SIZE: usize = 216;

    pub fn new(bytes: Secret<[u8; Self::LENGTH]>) -> Self {
        Self(bytes)
    }

    /// Quickly create a `RootSeed` for tests.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_u64(v: u64) -> Self {
        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(&v.to_le_bytes());
        Self::new(Secret::new(seed))
    }

    pub fn from_rng<R: Crng>(rng: &mut R) -> Self {
        Self(Secret::new(rng.gen_bytes()))
    }

    // --- BIP39 Mnemonics --- //

    /// Creates a [`bip39::Mnemonic`] from this [`RootSeed`]. Use
    /// [`bip39::Mnemonic`]'s `Display` / `FromStr` impls to convert from / to
    /// user-facing strings.
    pub fn to_mnemonic(&self) -> bip39::Mnemonic {
        bip39::Mnemonic::from_entropy_in(
            bip39::Language::English,
            self.0.expose_secret().as_slice(),
        )
        .expect("Always succeeds for 256 bits")
    }

    /// Derives the BIP39-compatible 64-byte seed from this [`RootSeed`].
    ///
    /// This uses the standard BIP39 derivation:
    /// `PBKDF2(password=mnemonic, salt="mnemonic", 2048 rounds, HMAC-SHA512)`
    ///
    /// The resulting seed is compatible with standard wallets when used to
    /// derive a BIP32 master xpriv.
    ///
    /// New Lexe wallets created > node-v0.9.1 use this to derive their
    /// on-chain wallet BIP32 master xprivs.
    ///
    /// Old Lexe on-chain wallets use the [`Self::derive_legacy_master_xprv`]
    /// instead.
    pub fn derive_bip39_seed(&self) -> Secret<[u8; 64]> {
        // RootSeed ("entropy") -> mnemonic
        let mnemonic = self.to_mnemonic();

        // Write out mnemonic words separated by spaces. Do it on the stack
        // to avoid allocations.
        let mut buf = [0u8; Self::BIP39_MNEMONIC_BUF_SIZE];
        let mut len = 0;
        for (i, word) in mnemonic.words().enumerate() {
            if i > 0 {
                buf[len] = b' ';
                len += 1;
            }
            let word_bytes = word.as_bytes();
            buf[len..len + word_bytes.len()].copy_from_slice(word_bytes);
            len += word_bytes.len();
        }
        let mnemonic_bytes = &buf[..len];

        // BIP39 salt is "mnemonic" + passphrase (empty for standard wallets)
        let salt = b"mnemonic";

        // mnemonic -- PBKDF2 -> BIP39 seed
        let mut seed = [0u8; 64];
        ring::pbkdf2::derive(
            ring::pbkdf2::PBKDF2_HMAC_SHA512,
            const { NonZeroU32::new(2048).unwrap() },
            salt,
            mnemonic_bytes,
            &mut seed,
        );

        // Zeroize the temporary buffer
        buf.zeroize();

        Secret::new(seed)
    }

    // --- Key derivations --- //

    fn extract(&self) -> ring::hkdf::Prk {
        let salted_hkdf = ring::hkdf::Salt::new(
            ring::hkdf::HKDF_SHA256,
            Self::HKDF_SALT.as_slice(),
        );
        salted_hkdf.extract(self.0.expose_secret().as_slice())
    }

    /// Derive a new child secret with `label` into a prepared buffer `out`.
    pub fn derive_to_slice(&self, label: &[&[u8]], out: &mut [u8]) {
        struct OkmLength(usize);

        impl ring::hkdf::KeyType for OkmLength {
            fn len(&self) -> usize {
                self.0
            }
        }

        assert!(out.len() <= Self::HKDF_MAX_OUT_LEN);

        self.extract()
            .expand(label, OkmLength(out.len()))
            .expect("should not fail")
            .fill(out)
            .expect("should not fail")
    }

    /// Derive a new child secret with `label` to a hash-output-sized buffer.
    pub fn derive(&self, label: &[&[u8]]) -> Secret<[u8; 32]> {
        let mut out = [0u8; 32];
        self.derive_to_slice(label, &mut out);
        Secret::new(out)
    }

    /// Convenience method to derive a new child secret with `label` into a
    /// `Vec<u8>` of size `out_len`.
    pub fn derive_vec(&self, label: &[&[u8]], out_len: usize) -> SecretVec<u8> {
        let mut out = vec![0u8; out_len];
        self.derive_to_slice(label, &mut out);
        SecretVec::new(out)
    }

    /// Derive the keypair for the "ephemeral issuing" CA that endorses
    /// client and server certs under the "shared seed" mTLS construction.
    pub fn derive_ephemeral_issuing_ca_key_pair(&self) -> ed25519::KeyPair {
        // TODO(max): Ideally rename to "ephemeral issuing ca key pair", but
        // need to ensure backwards compatibility. Both client and server need
        // to trust the old + new CAs before the old CA can be removed.
        let seed = self.derive(&[b"shared seed tls ca key pair"]);
        ed25519::KeyPair::from_seed(seed.expose_secret())
    }

    /// Derive the keypair for the "revocable issuing" CA that endorses
    /// client and server certs under the "shared seed" mTLS construction.
    pub fn derive_revocable_issuing_ca_key_pair(&self) -> ed25519::KeyPair {
        let seed = self.derive(&[b"revocable issuing ca key pair"]);
        ed25519::KeyPair::from_seed(seed.expose_secret())
    }

    /// Derive the user key pair, which is the key behind the [`UserPk`]. This
    /// key pair is also used to sign up and authenticate as the user against
    /// the lexe backend.
    ///
    /// [`UserPk`]: crate::api::user::UserPk
    pub fn derive_user_key_pair(&self) -> ed25519::KeyPair {
        let seed = self.derive(&[b"user key pair"]);
        ed25519::KeyPair::from_seed(seed.expose_secret())
    }

    /// Convenience function to derive the [`UserPk`].
    pub fn derive_user_pk(&self) -> UserPk {
        UserPk::new(self.derive_user_key_pair().public_key().into_inner())
    }

    /// Derive the BIP32 master xpriv using the BIP39-compatible derived 64-byte
    /// seed.
    ///
    /// This is used for new Lexe on-chain wallets created > node-v0.9.1.
    /// Wallets created before then use the [`Self::derive_legacy_master_xprv`].
    ///
    /// This produces keys compatible with standard wallets that follow the
    /// BIP39 spec.
    pub fn derive_bip32_master_xprv(&self, network: LxNetwork) -> bip32::Xpriv {
        let bip39_seed = self.derive_bip39_seed();
        bip32::Xpriv::new_master(
            network.to_bitcoin(),
            bip39_seed.expose_secret(),
        )
        .expect("Should never fail")
    }

    /// Derive the "legacy" BIP32 master xpriv by feeding the 32-byte
    /// [`RootSeed`] directly into BIP32's HMAC-SHA512.
    ///
    /// This is used for LDK seed derivation (via [`Self::derive_ldk_seed`]) and
    /// for existing on-chain wallets created before BIP39 compatibility.
    ///
    /// It's called "legacy" because standard BIP39 wallets derive the master
    /// xpriv from a 64-byte seed (produced by PBKDF2), not the original 32-byte
    /// entropy. This makes Lexe's old on-chain addresses incompatible with
    /// external wallets. New on-chain wallets use the BIP39-compatible
    /// derivation instead.
    pub fn derive_legacy_master_xprv(
        &self,
        network: LxNetwork,
    ) -> bip32::Xpriv {
        bip32::Xpriv::new_master(network.to_bitcoin(), self.0.expose_secret())
            .expect("Should never fail")
    }

    /// Derives the root seed used in LDK. The `KeysManager` is initialized
    /// using this seed, and `secp256k1` keys are derived from this seed.
    pub fn derive_ldk_seed<R: Crng>(&self, rng: &mut R) -> Secret<[u8; 32]> {
        // The [u8; 32] output will be the same regardless of the network the
        // master_xprv uses, as tested in `when_does_network_matter`
        let master_xprv = self.derive_legacy_master_xprv(LxNetwork::Mainnet);

        // Derive the hardened child key at `m/535h`, where 535 is T9 for "LDK"
        let secp_ctx = rng.gen_secp256k1_ctx();
        let m_535h =
            ChildNumber::from_hardened_idx(535).expect("Is within [0, 2^31-1]");
        let ldk_xprv = master_xprv
            .derive_priv(&secp_ctx, &m_535h)
            .expect("Should always succeed");

        Secret::new(ldk_xprv.private_key.secret_bytes())
    }

    /// Derive the Lightning node key pair without needing to derive all the
    /// other auxiliary node secrets used in the `KeysManager`.
    pub fn derive_node_key_pair<R: Crng>(
        &self,
        rng: &mut R,
    ) -> secp256k1::Keypair {
        // Derive the LDK seed first.
        let ldk_seed = self.derive_ldk_seed(rng);

        // When deriving a secp256k1 key, the network doesn't matter.
        // This is checked in when_does_network_matter.
        let ldk_xprv = bip32::Xpriv::new_master(
            Network::Bitcoin,
            ldk_seed.expose_secret(),
        )
        .expect("should never fail; the sizes match up");

        let secp_ctx = rng.gen_secp256k1_ctx();
        let m_0h = ChildNumber::from_hardened_idx(0)
            .expect("should never fail; index is in range");
        let node_sk = ldk_xprv
            .derive_priv(&secp_ctx, &m_0h)
            .expect("should never fail")
            .private_key;

        secp256k1::Keypair::from_secret_key(&secp_ctx, &node_sk)
    }

    /// Convenience function to derive the Lightning node pubkey.
    pub fn derive_node_pk<R: Crng>(&self, rng: &mut R) -> NodePk {
        NodePk(self.derive_node_key_pair(rng).public_key())
    }

    pub fn derive_vfs_master_key(&self) -> AesMasterKey {
        let secret = self.derive(&[b"vfs master key"]);
        AesMasterKey::new(secret.expose_secret())
    }

    #[cfg(any(test, feature = "test-utils"))]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.expose_secret().as_slice()
    }

    // --- Password encryption --- //

    /// Attempts to encrypt this root seed under the given password.
    ///
    /// The password must have at least [`MIN_PASSWORD_LENGTH`] characters and
    /// must not have any more than [`MAX_PASSWORD_LENGTH`] characters.
    ///
    /// Returns a [`Vec<u8>`] which can be persisted and later decrypted using
    /// only the given password.
    ///
    /// [`MIN_PASSWORD_LENGTH`]: crate::password::MIN_PASSWORD_LENGTH
    /// [`MAX_PASSWORD_LENGTH`]: crate::password::MAX_PASSWORD_LENGTH
    pub fn password_encrypt(
        &self,
        rng: &mut impl Crng,
        password: &str,
    ) -> anyhow::Result<Vec<u8>> {
        // Sample a completely random salt for maximum security.
        let salt = rng.gen_bytes();

        // Obtain the password-encrypted AES ciphertext.
        let mut aes_ciphertext =
            password::encrypt(rng, password, &salt, self.0.expose_secret())
                .context("Password encryption failed")?;

        // Final persistable value is `salt || aes_ciphertext`
        let mut combined = Vec::from(salt);
        combined.append(&mut aes_ciphertext);

        // Sanity check the length of the combined salt + aes_ciphertext.
        // Combined length is 32 bytes (salt) + encrypted length of 32 byte seed
        let expected_combined_len = 32 + aes::encrypted_len(32);
        assert!(combined.len() == expected_combined_len);

        Ok(combined)
    }

    /// Attempts to construct a [`RootSeed`] given a decryption password and the
    /// [`Vec<u8>`] returned from a previous call to [`password_encrypt`].
    ///
    /// [`password_encrypt`]: Self::password_encrypt
    pub fn password_decrypt(
        password: &str,
        mut combined: Vec<u8>,
    ) -> anyhow::Result<Self> {
        // Combined length is 32 bytes (salt) + encrypted length of 32 byte seed
        let expected_combined_len = 32 + aes::encrypted_len(32);
        ensure!(
            combined.len() == expected_combined_len,
            "Combined bytes had the wrong length"
        );

        // Split `salt || aes_ciphertext` into component parts
        let aes_ciphertext = combined.split_off(32);
        let unsized_salt = combined.into_boxed_slice();
        let salt = Box::<[u8; 32]>::try_from(unsized_salt)
            .expect("We split off at 32, so there are exactly 32 bytes");

        // Password-decrypt.
        let root_seed_bytes =
            password::decrypt(password, &salt, aes_ciphertext)
                .map(Secret::new)
                .context("Password decryption failed")?;

        // Construct the RootSeed
        Self::try_from(root_seed_bytes.expose_secret().as_slice())
    }

    // --- Seedphrase file I/O --- //

    /// Reads a [`RootSeed`] from a seedphrase file containing a BIP39 mnemonic.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    pub fn read_from_seedphrase_file(
        path: impl AsRef<Path>,
    ) -> anyhow::Result<Option<Self>> {
        let path = path.as_ref();
        match fs::read_to_string(path) {
            Ok(contents) => {
                let mnemonic = bip39::Mnemonic::from_str(contents.trim())
                    .map_err(|e| anyhow!("Invalid mnemonic: {e}"))?;
                let root_seed = Self::try_from(mnemonic)
                    .context("Failed to derive root seed from mnemonic")?;
                Ok(Some(root_seed))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("Failed to read seedphrase file"),
        }
    }

    /// Writes this [`RootSeed`]'s mnemonic to a seedphrase file.
    ///
    /// Creates parent directories if needed. Returns an error if the file
    /// already exists.
    pub fn write_to_seedphrase_file(
        &self,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let path = path.as_ref();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context("Failed to create data directory")?;
        }

        // Open with create_new to fail if file exists
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .with_context(|| {
                format!("Seedphrase file already exists: {}", path.display())
            })?;

        let mnemonic = self.to_mnemonic();
        writeln!(file, "{mnemonic}")
            .context("Failed to write seedphrase file")?;

        Ok(())
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
        hex::decode_to_slice(hex, bytes.as_mut_slice())
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
            bail!("input must be {} bytes", Self::LENGTH);
        }
        let mut out = [0u8; Self::LENGTH];
        out[..].copy_from_slice(bytes);
        Ok(Self::new(Secret::new(out)))
    }
}

impl TryFrom<bip39::Mnemonic> for RootSeed {
    type Error = anyhow::Error;

    fn try_from(mnemonic: bip39::Mnemonic) -> Result<Self, Self::Error> {
        use lexe_std::array::ArrayExt;

        // to_entropy_array() returns [u8; 33]
        let (entropy, entropy_len) = mnemonic.to_entropy_array();
        let entropy = secrecy::zeroize::Zeroizing::new(entropy);

        ensure!(entropy_len == 32, "Should contain exactly 32 bytes");

        let (seed_buf, _remainder) = entropy.split_array_ref_stable::<32>();

        Ok(Self(Secret::new(*seed_buf)))
    }
}

struct RootSeedVisitor;

impl de::Visitor<'_> for RootSeedVisitor {
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

#[cfg(any(test, feature = "test-utils"))]
mod test_impls {
    use proptest::{
        arbitrary::{Arbitrary, any},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for RootSeed {
        type Strategy = BoxedStrategy<Self>;
        type Parameters = ();

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>()
                .prop_map(|buf| Self::new(Secret::new(buf)))
                .no_shrink()
                .boxed()
        }
    }

    // only impl PartialEq in tests; not safe to compare root seeds w/o constant
    // time comparison.
    impl PartialEq for RootSeed {
        fn eq(&self, other: &Self) -> bool {
            self.expose_secret() == other.expose_secret()
        }
    }
}

#[cfg(test)]
mod test {
    use bitcoin::NetworkKind;
    use proptest::{
        arbitrary::any, collection::vec, prop_assert_eq, proptest,
        strategy::Strategy, test_runner::Config,
    };

    use super::*;
    use crate::{ln::network::LxNetwork, rng::FastRng};

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
        info: &[&[u8]],
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
            for info_part in info {
                m_i.extend_from_slice(info_part);
            }
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

    /// ```bash
    /// $ cargo test -p common -- dump_root_seed --ignored --show-output
    /// ```
    #[ignore]
    #[test]
    fn dump_root_seed() {
        let mut rng = FastRng::from_u64(1234);
        let root_seed = RootSeed::from_u64(20240506);
        let root_seed_hex = hex::encode(root_seed.expose_secret());
        let user_pk = root_seed.derive_user_pk();
        let node_pk = root_seed.derive_node_pk(&mut rng);

        println!(
            "root_seed: '{root_seed_hex}', \
             user_pk: '{user_pk}', node_pk: '{node_pk}'"
        );
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
    fn test_root_seed_derive() {
        let seed = RootSeed::from_u64(0x42);

        let out8 = seed.derive_vec(&[b"very cool secret"], 8);
        let out16 = seed.derive_vec(&[b"very cool secret"], 16);
        let out32 = seed.derive_vec(&[b"very cool secret"], 32);
        let out32_2 = seed.derive(&[b"very cool secret"]);

        assert_eq!("c724f46ae4c48017", hex::encode(out8.expose_secret()));
        assert_eq!(
            "c724f46ae4c480172a75cf775dbb64b1",
            hex::encode(out16.expose_secret())
        );
        assert_eq!(
            "c724f46ae4c480172a75cf775dbb64b160beb74137eb7d0cef72fde0523674de",
            hex::encode(out32.expose_secret())
        );
        assert_eq!(out32.expose_secret(), out32_2.expose_secret());
    }

    // Fuzz our KDF against a basic, readable implementation of HKDF-SHA256.
    #[test]
    fn test_root_seed_derive_equiv() {
        let arb_seed = any::<RootSeed>();
        let arb_label = vec(vec(any::<u8>(), 0..=64), 0..=4);
        let arb_len = 0_usize..=1024;

        proptest!(|(seed in arb_seed, label in arb_label, len in arb_len)| {
            let label = label
                .iter()
                .map(|x| x.as_slice())
                .collect::<Vec<_>>();

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

    /// A series of tests that demonstrate when the [`LxNetwork`] affects the
    /// partial equality of key material at various stages of derivation.
    /// This helps determine whether our APIs should take a [`Network`] as a
    /// parameter, or if setting a default would be sufficient.
    #[test]
    fn when_does_network_matter() {
        proptest!(|(
            mut rng in any::<FastRng>(),
            root_seed in any::<RootSeed>(),
            network1 in any::<LxNetwork>(),
            network2 in any::<LxNetwork>(),
        )| {
            let network_kind1 = NetworkKind::from(network1.to_bitcoin());
            let network_kind2 = NetworkKind::from(network2.to_bitcoin());
            let secp_ctx = rng.gen_secp256k1_ctx();

            // Network DOES matter for master xprvs (and all xprvs in general),
            // but only to the extent that their `NetworkKind` is different.
            // i.e. a `Signet` and `Testnet` xprv may be considered the same.
            let master_xprv1 = root_seed.derive_legacy_master_xprv(network1);
            let master_xprv2 = root_seed.derive_legacy_master_xprv(network2);
            // Assert: "master xprvs are equal iff network kinds are equal"
            let master_xprvs_equal = master_xprv1 == master_xprv2;
            let network_kinds_equal = network_kind1 == network_kind2;
            prop_assert_eq!(master_xprvs_equal, network_kinds_equal);

            // Test derive_ldk_seed(): The [u8; 32] LDK seed should be the same
            // regardless of the network of the master_xprv it was based on
            let m_535h = ChildNumber::from_hardened_idx(535)
                .expect("Is within [0, 2^31-1]");
            let ldk_seed1 = master_xprv1
                .derive_priv(&secp_ctx, &m_535h)
                .expect("Should always succeed")
                .private_key
                .secret_bytes();
            let ldk_seed2 = master_xprv2
                .derive_priv(&secp_ctx, &m_535h)
                .expect("Should always succeed")
                .private_key
                .secret_bytes();
            prop_assert_eq!(ldk_seed1, ldk_seed2);
            let ldk_seed = ldk_seed1;

            // Test derive_node_key_pair() and derive_node_pk(): The outputted
            // secp256k1::Keypair should be the same regardless of the network
            // of the ldk_xprv it was based on
            let ldk_xprv1 = bip32::Xpriv::new_master(network1.to_bitcoin(), &ldk_seed)
                .expect("Should never fail");
            let ldk_xprv2 = bip32::Xpriv::new_master(network2.to_bitcoin(), &ldk_seed)
                .expect("Should never fail");
            // Assert: "ldk_xprvs are equal iff network kinds are equal"
            let ldk_xprvs_equal = ldk_xprv1 == ldk_xprv2;
            prop_assert_eq!(ldk_xprvs_equal, network_kinds_equal);
            // First check the node_sks
            let m_0h = ChildNumber::from_hardened_idx(0)
                .expect("should never fail; index is in range");
            let node_sk1 = ldk_xprv1
                .derive_priv(&secp_ctx, &m_0h)
                .expect("should never fail")
                .private_key;
            let node_sk2 = ldk_xprv2
                .derive_priv(&secp_ctx, &m_0h)
                .expect("should never fail")
                .private_key;
            prop_assert_eq!(node_sk1, node_sk2);
            // Then check the keypairs
            let keypair1 =
                secp256k1::Keypair::from_secret_key(&secp_ctx, &node_sk1);
            let keypair2 =
                secp256k1::Keypair::from_secret_key(&secp_ctx, &node_sk2);
            prop_assert_eq!(keypair1, keypair2);
            // Then check the node_pks
            let node_pk1 = NodePk(secp256k1::PublicKey::from(keypair1));
            let node_pk2 = NodePk(secp256k1::PublicKey::from(keypair2));
            prop_assert_eq!(node_pk1, node_pk2);
            // Then check the serialized node_pks
            let node_pk1_str = node_pk1.to_string();
            let node_pk2_str = node_pk2.to_string();
            prop_assert_eq!(node_pk1_str, node_pk2_str);
        });
    }

    #[test]
    fn password_encryption_roundtrip() {
        use password::{MAX_PASSWORD_LENGTH, MIN_PASSWORD_LENGTH};

        let password_length_range = MIN_PASSWORD_LENGTH..MAX_PASSWORD_LENGTH;
        let any_valid_password =
            proptest::collection::vec(any::<char>(), password_length_range)
                .prop_map(String::from_iter);

        // Reduce cases since we do key stretching which is quite expensive
        let config = Config::with_cases(4);
        proptest!(config, |(
            mut rng in any::<FastRng>(),
            password in any_valid_password,
        )| {
            let root_seed1 = RootSeed::from_rng(&mut rng);
            let encrypted = root_seed1.password_encrypt(&mut rng, &password)
                .unwrap();
            let root_seed2 = RootSeed::password_decrypt(&password, encrypted)
                .unwrap();
            assert_eq!(root_seed1, root_seed2);
        })
    }

    #[test]
    fn password_decryption_compatibility() {
        let root_seed1 = RootSeed::new(Secret::new([69u8; 32]));
        let password1 = "password1234";
        // // Uncomment to regenerate
        // let mut rng = FastRng::from_u64(20231017);
        // let encrypted =
        //     root_seed1.password_encrypt(&mut rng, password1).unwrap();
        // let encrypted_hex = hex::display(&encrypted);
        // println!("Encrypted: {encrypted_hex}");

        let encrypted = hex::decode("adcfc4aef26858bacfae83dd19e735bb145203ab18183cbe932cd742b4446e7300b561678b0652666b316288bbb57552c4f40e91d8e440fd1085cba610204ca982f52fce471de27fe360e9560cee0996e55ce7ac323201908b7ff261b8ff425a87d215e83870e45062d988627c8cb7216b").unwrap();
        let root_seed1_decrypted =
            RootSeed::password_decrypt(password1, encrypted).unwrap();
        assert_eq!(root_seed1, root_seed1_decrypted);

        let root_seed2 = RootSeed::new(Secret::new([0u8; 32]));
        let password2 = "                ";
        // // Uncomment to regenerate
        // let mut rng = FastRng::from_u64(20231017);
        // let encrypted =
        //     root_seed2.password_encrypt(&mut rng, password2).unwrap();
        // let encrypted_hex = hex::display(&encrypted);
        // println!("Encrypted: {encrypted_hex}");

        let encrypted = hex::decode("adcfc4aef26858bacfae83dd19e735bb145203ab18183cbe932cd742b4446e7300b561678b0652666b316288bbb57552c4f40e91d8e440fd1085cba610204ca982062fbcb21c14cdb9d107f2f359e0f272e473d2cdb71a870d8fb19d1169c160876ee1ccde4f73a8f2b4ebc9bed68f6139").unwrap();
        let root_seed2_decrypted =
            RootSeed::password_decrypt(password2, encrypted).unwrap();
        assert_eq!(root_seed2, root_seed2_decrypted);
    }

    #[test]
    fn root_seed_mnemonic_round_trip() {
        proptest!(|(root_seed1 in any::<RootSeed>())| {
            let mnemonic = root_seed1.to_mnemonic();

            // All mnemonics should have exactly 24 words.
            prop_assert_eq!(mnemonic.word_count(), 24);

            let root_seed2 = RootSeed::try_from(mnemonic).unwrap();
            prop_assert_eq!(
                root_seed1.expose_secret(), root_seed2.expose_secret()
            );
        });
    }

    /// Check correctness of `bip39::Mnemonic`'s `FromStr` / `Display` impls
    #[test]
    fn mnemonic_fromstr_display_roundtrip() {
        proptest!(|(root_seed in any::<RootSeed>())| {
            let mnemonic1 = root_seed.to_mnemonic();
            let mnemonic2 = bip39::Mnemonic::from_str(&mnemonic1.to_string()).unwrap();
            prop_assert_eq!(mnemonic1, mnemonic2)
        })
    }

    #[cfg(not(target_env = "sgx"))]
    #[test]
    fn seedphrase_file_roundtrip() {
        let mut rng = FastRng::from_u64(20250216);
        let root_seed1 = RootSeed::from_rng(&mut rng);

        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("seedphrase.txt");

        // Write seedphrase to file
        root_seed1.write_to_seedphrase_file(&path).unwrap();

        // Read it back
        let root_seed2 =
            RootSeed::read_from_seedphrase_file(&path).unwrap().unwrap();

        assert_eq!(root_seed1, root_seed2);

        // Writing again should fail (file exists)
        let err = root_seed1.write_to_seedphrase_file(&path).unwrap_err();
        assert!(err.to_string().contains("already exists"));

        // Reading non-existent file should return None
        let missing = tempdir.path().join("missing.txt");
        assert!(
            RootSeed::read_from_seedphrase_file(&missing)
                .unwrap()
                .is_none()
        );
    }

    /// A basic compatibility test to check that a few "known good" pairings of
    /// [`RootSeed`] <-> [`Mnemonic`] <-> [`String`] still correspond. This
    /// ensures that the [`bip39`] crate cannot introduce compatibility-breaking
    /// changes without us noticing.
    #[test]
    fn mnemonic_compatibility_test() {
        // This code generated the "known good" values
        // let mut rng = FastRng::from_u64(98592174);
        // let seed1 = RootSeed::from_rng(&mut rng);
        // let seed2 = RootSeed::from_rng(&mut rng);
        // let seed3 = RootSeed::from_rng(&mut rng);
        // let seed1_str = hex::encode(seed1.as_bytes());
        // let seed2_str = hex::encode(seed2.as_bytes());
        // let seed3_str = hex::encode(seed3.as_bytes());
        // println!("{seed1_str}");
        // println!("{seed2_str}");
        // println!("{seed3_str}");
        // let mnenemenmenomic1 = seed1.to_mnemonic().to_string();
        // let mnenemenmenomic2 = seed2.to_mnemonic().to_string();
        // let mnenemenmenomic3 = seed3.to_mnemonic().to_string();
        // println!("{mnenemenmenomic1}");
        // println!("{mnenemenmenomic2}");
        // println!("{mnenemenmenomic3}");

        // "Known good" seeds
        let seed1 = RootSeed::new(Secret::new(hex::decode_const(
            b"91f24ce8326abc2e9faef6a3b866021ce9574c11210e86b0f457a31ed8ad4cba",
        )));
        let seed2 = RootSeed::new(Secret::new(hex::decode_const(
            b"5c2aa5fdd678112c8b13d745b5c1d1e1a81ace76721ec72f1424bd2eb387a8af",
        )));
        let seed3 = RootSeed::new(Secret::new(hex::decode_const(
            b"51ddba4775fc71fb1dba65dfc2ffab7526dd61bae7a9b13e9f3aa550bee19360",
        )));

        // "Known good" mnemonic strings
        let str1 = String::from(
            "music mystery deliver gospel profit blanket leaf tell \
            photo segment letter degree nice plastic duty canyon \
            mammal marble bicycle economy unique find cream dune",
        );
        let str2 = String::from(
            "found festival legal provide library north clump kit \
            east puppy inner select like grunt supply duck \
            shrimp judge ankle kid twenty sense pencil tray",
        );
        let str3 = String::from(
            "fade universe mushroom typical shove work ivory erosion \
            thank blood turn tumble horse radio twist vivid \
            raise visual solid enjoy armor ignore eternal arrange",
        );

        // Check `Mnemonic`
        let mnemonic_from_str1 = bip39::Mnemonic::from_str(&str1).unwrap();
        let mnemonic_from_str2 = bip39::Mnemonic::from_str(&str2).unwrap();
        let mnemonic_from_str3 = bip39::Mnemonic::from_str(&str3).unwrap();
        assert_eq!(seed1.to_mnemonic(), mnemonic_from_str1);
        assert_eq!(seed2.to_mnemonic(), mnemonic_from_str2);
        assert_eq!(seed3.to_mnemonic(), mnemonic_from_str3);

        // Check `RootSeed`
        let seed_from_str1 =
            RootSeed::try_from(mnemonic_from_str1.clone()).unwrap();
        let seed_from_str2 =
            RootSeed::try_from(mnemonic_from_str2.clone()).unwrap();
        let seed_from_str3 =
            RootSeed::try_from(mnemonic_from_str3.clone()).unwrap();
        assert_eq!(seed1.as_bytes(), seed_from_str1.as_bytes());
        assert_eq!(seed2.as_bytes(), seed_from_str2.as_bytes());
        assert_eq!(seed3.as_bytes(), seed_from_str3.as_bytes());

        // Check `String`
        assert_eq!(str1, seed1.to_mnemonic().to_string());
        assert_eq!(str2, seed2.to_mnemonic().to_string());
        assert_eq!(str3, seed3.to_mnemonic().to_string());
    }

    /// Snapshot tests to ensure key derivations don't change.
    /// These protect backwards compatibility for existing wallets.
    #[test]
    fn derive_snapshots() {
        let seed = RootSeed::from_u64(20240506);

        // Lexe user pubkey
        let user_pk = seed.derive_user_pk();
        assert_eq!(
            user_pk.to_string(),
            "a9edf9596ddf589918beca32d148a7d0ba59273b419ccf63a910f1b75861ff06",
        );

        // Lightning node pubkey
        let node_pk = seed.derive_node_pk(&mut FastRng::from_u64(1234));
        assert_eq!(
            node_pk.to_string(),
            "035a70d45eec7efb270319f116a9684250acb4ef282a26d21874878e7c5088f73b",
        );

        // LDK seed (used to initialize KeysManager)
        let ldk_seed = seed.derive_ldk_seed(&mut FastRng::from_u64(1234));
        assert_eq!(
            hex::encode(ldk_seed.expose_secret()),
            "551444699ae8acbebe67d5b54da844e8297b83e26e205203a65f29564eaf3787",
        );

        // BIP39 compatible 64-byte seed
        let bip39_seed = seed.derive_bip39_seed();
        assert_eq!(
            hex::encode(bip39_seed.expose_secret()),
            "30dc1cca6811e6f52a6efba751db4fe9495883b778c72b28ee248f0076cf03b9\
             dc3c3d7d662c98806ce59c0e59911a249533ca0c82dea3780cdf040f9a3dfe09",
        );

        // BIP39-compatible master xpriv (for new on-chain wallets)
        let bip39_master_xpriv =
            seed.derive_bip32_master_xprv(LxNetwork::Mainnet);
        assert_eq!(
            bip39_master_xpriv.to_string(),
            "xprv9s21ZrQH143K3BwTSDGEpsQA99b5fmckcX2s4dBbxojs287ApWXGThVTu9\
             TmogYG8A1JiUnbD6gHSfw5hXsTduny878ygutaCaCvg1KTvgM",
        );

        // BIP39-compatible master xpriv (Testnet)
        let bip39_testnet_xpriv =
            seed.derive_bip32_master_xprv(LxNetwork::Testnet3);
        assert_eq!(
            bip39_testnet_xpriv.to_string(),
            "tprv8ZgxMBicQKsPe1Az6n7jzX29TH1HuHekx4wyw3c4SnELoirFoss1ySrupK\
             dRp3vaVbY5iaQMNTG5uXUppkDQSy4ZekMHMGcd7fxM7h7WWqo"
        );

        // Legacy Lexe master xpriv (used for existing on-chain wallets)
        let master_xpriv = seed.derive_legacy_master_xprv(LxNetwork::Mainnet);
        assert_eq!(
            master_xpriv.to_string(),
            "xprv9s21ZrQH143K42JPXVa2Q7nAp6XB3FVwyYdGkQetMYRcprZXKvt52p1tqg\
             9fwyFJaL6Ki92bCdRNDPAnyddy7CzpQAEktM8nMtNGw4Xj6vt",
        );

        // Legacy Lexe master xpriv (Testnet)
        let master_xpriv_testnet =
            seed.derive_legacy_master_xprv(LxNetwork::Testnet3);
        assert_eq!(
            master_xpriv_testnet.to_string(),
            "tprv8ZgxMBicQKsPeqXvC4RXZmQA8DwPGmXxK6YPcq5LqWv6cTJcKJDpYZPLk\
             rKKxLdcwmd6iEeMMz1AgEiY6qyuvGGQvoT4YhrqGz7hNoR5R4G",
        );

        // Ephemeral issuing CA pubkey
        let ephemeral_ca = seed.derive_ephemeral_issuing_ca_key_pair();
        assert_eq!(
            ephemeral_ca.public_key().to_string(),
            "70656b5a6084c457bf004dad264cecc131879b7e6791fe0cc828c38cc0df6e92",
        );

        // Revocable issuing CA pubkey
        let revocable_ca = seed.derive_revocable_issuing_ca_key_pair();
        assert_eq!(
            revocable_ca.public_key().to_string(),
            "efe6e020ba9ca4a50467cdbaff469f9d465f21d1c6fe976868a20d97bbaa2ee3",
        );

        // VFS master key (via derivation + encryption)
        let vfs_ctxt = seed.derive_vfs_master_key().encrypt(
            &mut FastRng::from_u64(1234),
            &[],
            None,
            &|out: &mut Vec<u8>| out.extend_from_slice(b"test"),
        );
        assert_eq!(
            hex::encode(&vfs_ctxt),
            "0000a7e6a0514440b57fcf6df97b46132adde062f1a5a224aacf4fa0f286b4c56\
             fe2768b7dad22333936638c5734f0d529a74880aa",
        );
    }

    /// Verify the BIP39 mnemonic buffer size constant is large enough.
    #[test]
    fn bip39_mnemonic_buf_size() {
        let words = bip39::Language::English.word_list();
        let max_word_len = words.iter().map(|w| w.len()).max().unwrap();
        assert_eq!(max_word_len, 8);

        let root_seed = RootSeed::from_u64(20240506);
        let mnemonic = root_seed.to_mnemonic();
        let num_words = mnemonic.words().count();
        assert_eq!(num_words, 24);

        // Max size: 24 words * 8 chars + 23 spaces = 215 bytes
        assert!(
            (max_word_len + 1) * num_words <= RootSeed::BIP39_MNEMONIC_BUF_SIZE
        );
    }

    /// Verify our BIP39 seed derivation matches the rust-bip39 crate.
    #[test]
    fn derive_bip39_seed_matches_rust_bip39() {
        proptest!(|(root_seed in any::<RootSeed>())| {
            let mnemonic = root_seed.to_mnemonic();

            // Our implementation
            let our_seed = root_seed.derive_bip39_seed();

            // rust-bip39 implementation (empty passphrase)
            let their_seed = mnemonic.to_seed_normalized("");

            prop_assert_eq!(our_seed.expose_secret(), &their_seed);
        });
    }
}
