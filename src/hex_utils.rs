use std::fmt::Write;

use bitcoin::secp256k1::PublicKey;

#[inline]
fn is_even(x: usize) -> bool {
    x & 1 == 0
}

pub fn to_vec(hex: &str) -> Option<Vec<u8>> {
    if !is_even(hex.len()) {
        return None;
    }

    let mut out = Vec::with_capacity(hex.len() / 2);

    let mut b = 0;
    for (idx, c) in hex.as_bytes().iter().enumerate() {
        b <<= 4;
        match *c {
            b'A'..=b'F' => b |= c - b'A' + 10,
            b'a'..=b'f' => b |= c - b'a' + 10,
            b'0'..=b'9' => b |= c - b'0',
            _ => return None,
        }
        if (idx & 1) == 1 {
            out.push(b);
            b = 0;
        }
    }

    Some(out)
}

pub fn hex_str(bytes: &[u8]) -> String {
    let mut res = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut res, "{:02x}", byte).unwrap();
    }
    res
}

pub fn to_compressed_pubkey(hex: &str) -> Option<PublicKey> {
    if hex.len() != 33 * 2 {
        return None;
    }
    let data = match to_vec(&hex[0..33 * 2]) {
        Some(bytes) => bytes,
        None => return None,
    };
    match PublicKey::from_slice(&data) {
        Ok(pk) => Some(pk),
        Err(_) => None,
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::collection::vec;
    use proptest::strategy::Strategy;
    use proptest::{char, proptest};

    use super::*;

    #[test]
    fn test_hex() {
        assert_eq!("", hex_str(&[]));
        assert_eq!(
            "01348900abff",
            hex_str(&[0x01, 0x34, 0x89, 0x00, 0xab, 0xff])
        );
    }

    #[test]
    fn test_roundtrip_b2s2b() {
        let bytes = &[0x01, 0x34, 0x89, 0x00, 0xab, 0xff];
        assert_eq!(bytes.as_slice(), to_vec(&hex_str(bytes)).unwrap());

        proptest!(|(bytes in vec(any::<u8>(), 0..10))| {
            assert_eq!(bytes.as_slice(), to_vec(&hex_str(&bytes)).unwrap());
        })
    }

    #[test]
    fn test_roundtrip_s2b2s() {
        let hex = "01348900abff";
        assert_eq!(hex, hex_str(&to_vec(hex).unwrap()));

        let hex_char =
            char::ranges(['0'..='9', 'a'..='f', 'A'..='F'].as_slice().into());
        let hex_chars = vec(hex_char, 0..10);
        let hex_strs =
            hex_chars.prop_filter_map("no odd length hex strings", |chars| {
                if is_even(chars.len()) {
                    Some(String::from_iter(chars))
                } else {
                    None
                }
            });

        proptest!(|(hex in hex_strs)| {
            assert_eq!(hex.to_ascii_lowercase(), hex_str(&to_vec(&hex).unwrap()));
        })
    }
}
