use std::fmt::Write;

use bitcoin::secp256k1::PublicKey;

#[inline]
fn decode_nibble(x: u8) -> Option<u8> {
    match x {
        b'0'..=b'9' => Some(x - b'0'),
        b'a'..=b'f' => Some(x - b'a' + 10),
        b'A'..=b'F' => Some(x - b'A' + 10),
        _ => None,
    }
}

fn decode_to_slice_inner(hex_chunks: &[[u8; 2]], out: &mut [u8]) -> Option<()> {
    for (&[c_hi, c_lo], out_i) in hex_chunks.iter().zip(out) {
        let b_hi = decode_nibble(c_hi)?;
        let b_lo = decode_nibble(c_lo)?;
        *out_i = (b_hi << 4) | b_lo;
    }

    Some(())
}

// TODO(phlip9): need a constant time hex decode for deserializing secrets...

pub fn decode(hex: &str) -> Option<Vec<u8>> {
    let (hex_chunks, extra) = hex.as_bytes().as_chunks::<2>();
    if !extra.is_empty() {
        return None;
    }

    let mut out = vec![0u8; hex_chunks.len()];
    decode_to_slice_inner(hex_chunks, &mut out).map(|()| out)
}

pub fn decode_to_slice(hex: &str, out: &mut [u8]) -> Option<()> {
    let (hex_chunks, extra) = hex.as_bytes().as_chunks::<2>();
    if !extra.is_empty() {
        return None;
    }

    decode_to_slice_inner(hex_chunks, out)
}

pub fn encode(bytes: &[u8]) -> String {
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
    let data = match decode(&hex[0..33 * 2]) {
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

    #[inline]
    fn is_even(x: usize) -> bool {
        x & 1 == 0
    }

    #[test]
    fn test_encode() {
        assert_eq!("", encode(&[]));
        assert_eq!(
            "01348900abff",
            encode(&[0x01, 0x34, 0x89, 0x00, 0xab, 0xff])
        );
    }

    #[test]
    fn test_roundtrip_b2s2b() {
        let bytes = &[0x01, 0x34, 0x89, 0x00, 0xab, 0xff];
        assert_eq!(bytes.as_slice(), decode(&encode(bytes)).unwrap());

        proptest!(|(bytes in vec(any::<u8>(), 0..10))| {
            assert_eq!(bytes.as_slice(), decode(&encode(&bytes)).unwrap());
        })
    }

    #[test]
    fn test_roundtrip_s2b2s() {
        let hex = "01348900abff";
        assert_eq!(hex, encode(&decode(hex).unwrap()));

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
            assert_eq!(hex.to_ascii_lowercase(), encode(&decode(&hex).unwrap()));
        })
    }
}
