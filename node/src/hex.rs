use std::fmt::{self, Write};

use thiserror::Error;

#[derive(Clone, Copy, Error, Debug)]
pub enum DecodeError {
    #[error("hex decode error: output buffer length != half input length")]
    BadOutputLength,

    #[error("hex decode error: input contains non-hex character")]
    InvalidCharacter,

    #[error("hex decode error: input string length must be even")]
    OddInputLength,
}

#[inline]
fn decode_nibble(x: u8) -> Result<u8, DecodeError> {
    match x {
        b'0'..=b'9' => Ok(x - b'0'),
        b'a'..=b'f' => Ok(x - b'a' + 10),
        b'A'..=b'F' => Ok(x - b'A' + 10),
        _ => Err(DecodeError::InvalidCharacter),
    }
}

fn decode_to_slice_inner(
    hex_chunks: &[[u8; 2]],
    out: &mut [u8],
) -> Result<(), DecodeError> {
    if hex_chunks.len() != out.len() {
        return Err(DecodeError::BadOutputLength);
    }

    for (&[c_hi, c_lo], out_i) in hex_chunks.iter().zip(out) {
        let b_hi = decode_nibble(c_hi)?;
        let b_lo = decode_nibble(c_lo)?;
        *out_i = (b_hi << 4) | b_lo;
    }

    Ok(())
}

fn hex_str_to_chunks(hex: &str) -> Result<&[[u8; 2]], DecodeError> {
    let (hex_chunks, extra) = hex.as_bytes().as_chunks::<2>();
    if extra.is_empty() {
        Ok(hex_chunks)
    } else {
        Err(DecodeError::OddInputLength)
    }
}

pub fn decode(hex: &str) -> Result<Vec<u8>, DecodeError> {
    let hex_chunks = hex_str_to_chunks(hex)?;
    let mut out = vec![0u8; hex_chunks.len()];
    decode_to_slice_inner(hex_chunks, &mut out).map(|()| out)
}

pub fn decode_to_slice(hex: &str, out: &mut [u8]) -> Result<(), DecodeError> {
    let hex_chunks = hex_str_to_chunks(hex)?;
    decode_to_slice_inner(hex_chunks, out)
}

/// Decode a hex string into an output buffer in constant time.
/// This prevents leakage of e.g. # of digits vs # abcdef.
pub fn decode_to_slice_ct(
    hex: &str,
    out: &mut [u8],
) -> Result<(), DecodeError> {
    // TODO(phlip9): make this actually constant time
    decode_to_slice(hex, out)
}

pub fn encode(bytes: &[u8]) -> String {
    let mut res = String::with_capacity(bytes.len() * 2);
    write!(&mut res, "{}", display(bytes)).unwrap();
    res
}

pub struct HexDisplay<'a>(&'a [u8]);

impl<'a> fmt::Display for HexDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:02x}", byte)?
        }
        Ok(())
    }
}

impl<'a> fmt::Debug for HexDisplay<'a> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[inline]
pub fn display(bytes: &[u8]) -> HexDisplay<'_> {
    HexDisplay(bytes)
}

#[cfg(not(target_env = "sgx"))] // TODO Remove once this fn is used in sgx
pub fn to_compressed_pubkey(
    hex: &str,
) -> Option<bitcoin::secp256k1::PublicKey> {
    use bitcoin::secp256k1::PublicKey; // TODO Likewise
    if hex.len() != 33 * 2 {
        return None;
    }
    let data = match decode(&hex[0..33 * 2]) {
        Ok(bytes) => bytes,
        Err(_) => return None,
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
