//! A convenience module for hasing things with SHA-256.

/// SHA-256 digest a single input.
pub fn digest(input: &[u8]) -> ring::digest::Digest {
    digest_many(&[input])
}

/// SHA-256 digest several input slices concatenated together, without
/// allocating.
pub fn digest_many(inputs: &[&[u8]]) -> ring::digest::Digest {
    let mut ctx = context();
    for input in inputs {
        ctx.update(input);
    }
    ctx.finish()
}

/// Create a SHA-256 digest context for manually hashing e.g. large input files.
pub fn context() -> ring::digest::Context {
    ring::digest::Context::new(&ring::digest::SHA256)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hex;

    // sanity check
    #[test]
    fn test_sha256() {
        let actual = hex::encode(digest(b"").as_ref());
        let expected =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert_eq!(&actual, expected);
    }
}
