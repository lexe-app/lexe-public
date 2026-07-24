//! HMAC-SHA256 message authentication.
//!
//! A light wrapper around [`ring::hmac`]. Can be extended as needed.

use std::fmt;

use ring;

use crate::constant_time;

/// The length in bytes of an HMAC-SHA256 tag.
pub const TAG_LEN: usize = 32;

/// A key for computing and verifying HMAC-SHA256 tags.
pub struct Key(ring::hmac::Key);

impl Key {
    /// Create a new `ring::hmac::Key` from a random 32-byte seed.
    ///
    /// Use this when deriving a key from a KDF like `RootSeed`.
    pub fn from_seed(seed: &[u8; 32]) -> Self {
        Self(ring::hmac::Key::new(ring::hmac::HMAC_SHA256, seed))
    }

    /// Sign a given plaintext (`write_data_cb`) and `aad`, and return
    /// `plaintext || tag`.
    pub fn sign_and_append(
        &self,
        domain_separator: &[u8; 32],
        aad: &[&[u8]],
        // An optional hint indicating the size of the plaintext written
        // by `write_data_cb`; used to avoid `Vec` reallocation.
        data_size_hint: Option<usize>,
        // A closure which appends the plaintext to the provided `Vec`.
        write_data_cb: &dyn Fn(&mut Vec<u8>),
    ) -> Vec<u8> {
        let capacity = data_size_hint.unwrap_or_default() + TAG_LEN;
        let mut out = Vec::with_capacity(capacity);

        // out: `plaintext`
        write_data_cb(&mut out);

        let tag = self.sign(domain_separator, aad, &out);

        // out: `plaintext || tag`
        out.extend_from_slice(tag.as_ref());

        out
    }

    /// Verify a `plaintext || tag` blob produced by [`Self::sign_and_append`],
    /// returning `plaintext` on success. Constant-time.
    pub fn verify<'a>(
        &self,
        domain_separator: &[u8; 32],
        aad: &[&[u8]],
        signed: &'a [u8],
    ) -> Result<&'a [u8], &'static str> {
        let (plaintext, tag) =
            signed.split_last_chunk::<TAG_LEN>().ok_or("Tag mismatch")?;

        let expected_tag = self.sign(domain_separator, aad, plaintext);
        let expected_tag =
            <&[u8; TAG_LEN]>::try_from(expected_tag.as_ref()).unwrap();

        if constant_time::u8x32_eq(tag, expected_tag) {
            Ok(plaintext)
        } else {
            Err("Tag mismatch")
        }
    }

    /// `sign(domain_sep (|| aad_len || aad_text)* || text_len || plaintext)`
    fn sign(
        &self,
        domain_separator: &[u8; 32],
        aad: &[&[u8]],
        plaintext: &[u8],
    ) -> ring::hmac::Tag {
        let mut ctx = ring::hmac::Context::with_key(&self.0);
        ctx.update(domain_separator);
        for &datum in aad {
            let datum_len: u64 = u64::try_from(datum.len()).unwrap();
            ctx.update(&datum_len.to_le_bytes());
            ctx.update(datum);
        }
        let plaintext_len: u64 = u64::try_from(plaintext.len()).unwrap();
        ctx.update(&plaintext_len.to_le_bytes());
        ctx.update(plaintext);
        ctx.sign()
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("hmac::Key(..)")
    }
}

#[cfg(test)]
mod test {
    use std::{assert_eq, println};

    use proptest::{
        collection::vec,
        prelude::*,
        proptest,
        strategy::ValueTree,
        test_runner::{self, TestRng, TestRunner},
    };

    use super::*;
    use crate::rng::{FastRng, RngExt};

    fn sign(
        key: &Key,
        domain: &[u8; 32],
        aad: &[&[u8]],
        plaintext: &str,
    ) -> Vec<u8> {
        key.sign_and_append(domain, aad, Some(plaintext.len()), &|out| {
            out.extend_from_slice(plaintext.as_bytes())
        })
    }

    /// Test the [`Key::sign_and_append`] -> [`Key::verify`] roundtrip via
    /// proptest. Also ensures length is `plaintext.len() + TAG_LEN`.
    #[test]
    fn sign_verify_roundtrip() {
        proptest!(|(
            seed: [u8; 32],
            domain: [u8; 32],
            aads in vec(
                vec(any::<u8>(), 0..64),
                0..=5
            ),
            plaintexts in vec(
                vec(any::<u8>(), 0..64),
                0..=5
            )
        )| {
            let key = Key::from_seed(&seed);
            let aad = aads.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>();

            for plaintext in plaintexts {
                // Sign
                let signed = key.sign_and_append(
                    &domain,
                    &aad,
                    Some(plaintext.len()),
                    &|out| out.extend_from_slice(&plaintext),
                );

                // Verify length
                prop_assert_eq!(signed.len(), plaintext.len() + TAG_LEN);

                // Verify roundtrip
                let verified = key.verify(&domain, &aad, &signed);
                prop_assert_eq!(verified, Ok(plaintext.as_slice()));
            }
        });
    }

    // --- Unit tests --- //

    /// Ensure that our signed output remains backwards compatible.
    /// Snapshot sourced from [`print_snapshot_data`].
    #[test]
    fn snapshot_compat() {
        // --- Test case 1 --- //
        #[rustfmt::skip]
        let snapshot: (&[u8; 32], &[u8; 32], &[&[u8]], &[u8], &[u8]) = (
            /* seed      */ &[194, 128, 19, 113, 226, 250, 32, 233, 155, 123, 134, 83, 255, 2, 82, 142, 75, 252, 132, 118, 107, 203, 153, 82, 115, 146, 100, 251, 113, 66, 0, 164],
            /* domain    */ &[205, 174, 107, 153, 218, 56, 43, 171, 131, 97, 216, 115, 245, 148, 158, 129, 94, 121, 23, 95, 106, 195, 55, 241, 87, 91, 201, 34, 195, 166, 69, 6],
            /* aad       */ &[&[174, 176, 193, 58, 9, 240, 69, 146, 14, 115, 42, 112, 120, 75, 200, 60, 90, 76, 223, 99, 120, 142, 248, 60, 152, 44, 128, 156, 129, 120, 142, 140, 156, 82, 164, 216, 173, 162, 141, 135, 248, 121, 233, 183, 248, 239]],
            /* plaintext */ &[83, 53, 240, 125, 240, 64, 242, 207, 18, 70, 169, 92, 124, 37, 237, 27, 74, 139, 179, 117, 7, 130, 0, 146, 156, 249, 151, 75, 95, 75, 63, 189, 200, 167, 18, 13, 71, 25, 233, 20, 37, 164, 37, 218, 139, 161, 150, 85, 220, 238, 118, 18, 125, 54, 15, 143, 217, 202, 150],
            /* expected  */ &[83, 53, 240, 125, 240, 64, 242, 207, 18, 70, 169, 92, 124, 37, 237, 27, 74, 139, 179, 117, 7, 130, 0, 146, 156, 249, 151, 75, 95, 75, 63, 189, 200, 167, 18, 13, 71, 25, 233, 20, 37, 164, 37, 218, 139, 161, 150, 85, 220, 238, 118, 18, 125, 54, 15, 143, 217, 202, 150, 196, 167, 246, 228, 49, 114, 137, 165, 170, 79, 139, 49, 167, 196, 122, 69, 100, 33, 29, 234, 82, 57, 56, 152, 154, 75, 5, 71, 191, 149, 235, 183],
        );
        let (seed, domain, aad, plaintext, expected) = snapshot;

        let key = Key::from_seed(seed);
        let current =
            key.sign_and_append(domain, aad, Some(plaintext.len()), &|out| {
                out.extend_from_slice(plaintext)
            });
        assert_eq!(current, expected);

        // --- Test case 2 --- //
        #[rustfmt::skip]
        let snapshot: (&[u8; 32], &[u8; 32], &[&[u8]], &[u8], &[u8]) = (
            /* seed      */ &[173, 43, 12, 81, 7, 247, 91, 248, 211, 206, 87, 20, 13, 237, 65, 121, 194, 68, 248, 183, 167, 235, 201, 126, 239, 216, 89, 221, 132, 172, 69, 226],
            /* domain    */ &[46, 15, 88, 59, 109, 35, 131, 252, 22, 35, 193, 134, 136, 203, 32, 176, 160, 171, 73, 196, 15, 72, 242, 0, 45, 159, 65, 90, 45, 136, 92, 120],
            /* aad       */ &[&[81, 236, 111, 72, 77, 218, 20, 177, 176, 57, 243, 157, 41, 66, 15, 187, 164, 173, 85, 143, 4, 64, 213, 206, 147, 249, 6, 148, 65, 126, 77, 173, 183, 143, 87], &[105, 23, 87, 33, 135, 201, 32, 41, 87, 167, 87, 13, 94, 30, 49, 167, 56, 72, 36, 202, 75, 33, 241, 107, 84, 105, 255, 237, 27], &[32, 79, 153, 80, 253, 150, 181, 151, 5, 112, 247, 90, 28, 142, 64, 136, 139, 2, 46, 41, 173, 1, 174, 29, 107, 101, 139, 140, 94, 217, 180, 96, 73, 161, 157, 240, 11, 56, 121, 96, 151, 110, 188, 67, 45, 203, 13, 236, 32, 157, 220]],
            /* plaintext */ &[110, 192, 94, 70, 121, 234, 52, 58, 15, 101, 180, 91, 21, 151, 236, 105, 62, 37, 67, 205, 90, 214, 117, 146, 201, 123, 167, 61, 7, 175, 67, 81, 80, 175, 92, 167, 250, 73, 179, 2, 207, 253, 31, 145, 122, 75],
            /* signed    */ &[110, 192, 94, 70, 121, 234, 52, 58, 15, 101, 180, 91, 21, 151, 236, 105, 62, 37, 67, 205, 90, 214, 117, 146, 201, 123, 167, 61, 7, 175, 67, 81, 80, 175, 92, 167, 250, 73, 179, 2, 207, 253, 31, 145, 122, 75, 237, 12, 73, 125, 242, 196, 242, 81, 49, 29, 160, 90, 115, 80, 173, 136, 168, 219, 49, 154, 171, 71, 205, 17, 180, 187, 62, 82, 19, 35, 81, 33],
        );
        let (seed, domain, aad, plaintext, expected) = snapshot;

        let key = Key::from_seed(seed);
        let current =
            key.sign_and_append(domain, aad, Some(plaintext.len()), &|out| {
                out.extend_from_slice(plaintext)
            });
        assert_eq!(current, expected);

        // --- Test case 3 --- //
        #[rustfmt::skip]
        let snapshot: (&[u8; 32], &[u8; 32], &[&[u8]], &[u8], &[u8]) = (
            /* seed      */ &[52, 174, 187, 80, 227, 50, 89, 135, 176, 155, 248, 40, 194, 93, 212, 236, 30, 160, 67, 46, 160, 123, 82, 81, 33, 150, 189, 122, 25, 221, 254, 139],
            /* domain    */ &[100, 169, 17, 187, 44, 171, 149, 246, 126, 214, 71, 249, 146, 242, 233, 251, 1, 116, 102, 161, 118, 195, 39, 95, 171, 208, 109, 71, 119, 22, 8, 249],
            /* aad       */ &[&[239, 141, 103, 219, 18, 254, 151, 204, 227, 159, 19, 138, 196], &[251, 82, 191, 138, 33, 129, 155, 205, 76, 19, 58, 114, 156]],
            /* plaintext */ &[134, 159, 173, 109, 127, 231, 204, 230, 202, 129, 221, 5, 107, 202, 192, 165, 193, 216, 149, 124, 16, 9, 198, 65, 100, 154, 19, 150, 1, 93, 182, 253, 104, 63, 32, 150, 11, 174, 103, 185, 145, 255, 222, 242, 212, 13, 10, 67, 81, 152, 160, 50, 169, 45, 178, 241],
            /* signed    */ &[134, 159, 173, 109, 127, 231, 204, 230, 202, 129, 221, 5, 107, 202, 192, 165, 193, 216, 149, 124, 16, 9, 198, 65, 100, 154, 19, 150, 1, 93, 182, 253, 104, 63, 32, 150, 11, 174, 103, 185, 145, 255, 222, 242, 212, 13, 10, 67, 81, 152, 160, 50, 169, 45, 178, 241, 59, 126, 52, 102, 153, 18, 146, 129, 184, 46, 203, 30, 136, 181, 142, 128, 198, 66, 61, 227, 243, 15, 246, 190, 243, 138, 19, 227, 134, 141, 56, 184],
        );
        let (seed, domain, aad, plaintext, expected) = snapshot;

        let key = Key::from_seed(seed);
        let current =
            key.sign_and_append(domain, aad, Some(plaintext.len()), &|out| {
                out.extend_from_slice(plaintext)
            });
        assert_eq!(current, expected);
    }

    #[test]
    fn verify_rejects_tampering() {
        let key = Key::from_seed(&[42u8; 32]);
        let domain = [7u8; 32];
        let aad: &[&[u8]] = &[b"aad1", b"aad2"];

        let signed = sign(&key, &domain, aad, "hello world");

        for i in 0..signed.len() {
            let mut tampered = signed.clone();
            tampered[i] ^= 1;
            key.verify(&domain, aad, &tampered).unwrap_err();
        }
    }

    #[test]
    fn verify_rejects_wrong_key_domain_or_aad() {
        let key = Key::from_seed(&[1u8; 32]);
        let wrong_key = Key::from_seed(&[2u8; 32]);

        let domain = [7u8; 32];
        let wrong_domain = [8u8; 32];

        let aad: &[&[u8]] = &[b"aad1", b"aad2"];
        let wrong_aad: &[&[u8]] = &[b"aad1", b"aad3"];

        let signed = sign(&key, &domain, aad, "hello world");

        // Test wrong key
        wrong_key.verify(&domain, aad, &signed).unwrap_err();

        // Test wrong domain
        key.verify(&wrong_domain, aad, &signed).unwrap_err();

        // Test wrong and empty aad
        key.verify(&domain, wrong_aad, &signed).unwrap_err();
        key.verify(&domain, &[], &signed).unwrap_err();
    }

    /// The length prefixes must make each `aad` field unambiguous, so that
    /// shifting bytes across a field boundary changes the tag.
    #[test]
    fn aad_serialization_is_unambiguous() {
        let key = Key::from_seed(&[42u8; 32]);
        let domain = [7u8; 32];

        let signed = sign(&key, &domain, &[b"ab", b"c"], "hello world");

        // Test shifted `aad`s.
        key.verify(&domain, &[b"a", b"bc"], &signed).unwrap_err();
        key.verify(&domain, &[b"abc"], &signed).unwrap_err();
    }

    #[test]
    fn verify_rejects_too_short() {
        let key = Key::from_seed(&[42u8; 32]);
        let domain = [7u8; 32];
        let aad: &[&[u8]] = &[b"aad1", b"aad2"];

        key.verify(&domain, aad, &[0u8; TAG_LEN - 1]).unwrap_err();
        key.verify(&domain, aad, &[]).unwrap_err();
    }

    // --- Util --- //

    /// Snapshot some signed data. Output used in [`snapshot_compat`].
    /// ```bash
    /// $ cargo test -p lexe-crypto hmac::test::print_snapshot_data -- --show-output --ignored
    /// ```
    #[ignore]
    #[test]
    fn print_snapshot_data() {
        let mut rng = FastRng::from_u64(202608041547);

        let runner_rng = TestRng::from_seed(
            test_runner::RngAlgorithm::ChaCha,
            &rng.gen_bytes::<32>(),
        );
        let mut runner = TestRunner::new_with_rng(
            test_runner::Config::default(),
            runner_rng,
        );

        const NUM_CASES: usize = 3;
        let strategy = (
            any::<[u8; 32]>(),                   // seed
            any::<[u8; 32]>(),                   // domain
            vec(vec(any::<u8>(), 0..64), 0..=5), // AAD
            vec(any::<u8>(), 0..64),             // plaintexts
        );
        for i in 0..NUM_CASES {
            let (seed, domain, aad, plaintext) =
                strategy.new_tree(&mut runner).unwrap().current();
            let key = Key::from_seed(&seed);
            let aad = aad.iter().map(Vec::as_slice).collect::<Vec<&[u8]>>();
            let signed = key.sign_and_append(
                &domain,
                &aad,
                Some(plaintext.len()),
                &|out| out.extend_from_slice(&plaintext),
            );
            const PAD: usize = "plaintext".len();
            println!("--- ({i}) ---");
            println!("{:<PAD$} : {seed:?}", "seed");
            println!("{:<PAD$} : {domain:?}", "domain");
            println!("{:<PAD$} : {aad:?}", "aad");
            println!("{:<PAD$} : {plaintext:?}", "plaintext");
            println!("{:<PAD$} : {signed:?}", "signed");
        }
    }
}
