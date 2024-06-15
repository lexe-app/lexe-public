//! Utilities for encoding, decoding, and displaying hex-formatted data.

use std::{
    borrow::Cow,
    fmt::{self, Write},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::SliceExt;

/// Errors which can be produced while decoding a hex string.
#[derive(Copy, Clone, Debug, Error, Serialize, Deserialize)]
pub enum DecodeError {
    #[error("hex decode error: output buffer length != half input length")]
    BadOutputLength,

    #[error("hex decode error: input contains non-hex character")]
    InvalidCharacter,

    #[error("hex decode error: input string length must be even")]
    OddInputLength,
}

// --- Public functions --- //

/// Convert a byte slice to an owned hex string. If you simply need to display a
/// byte slice as hex, use [`display`] instead, which avoids the allocation.
pub fn encode(bytes: &[u8]) -> String {
    let mut out = vec![0u8; bytes.len() * 2];

    for (src, dst) in bytes.iter().zip(out.chunks_exact_mut(2)) {
        dst[0] = encode_nibble(src >> 4);
        dst[1] = encode_nibble(src & 0x0f);
    }

    // SAFETY: hex characters ([0-9a-f]*) are always valid UTF-8.
    unsafe { String::from_utf8_unchecked(out) }
}

/// Try to decode a hex string to owned bytes (`Vec<u8>`).
pub fn decode(hex: &str) -> Result<Vec<u8>, DecodeError> {
    let hex_chunks = hex_str_to_chunks(hex)?;
    let mut out = vec![0u8; hex_chunks.len()];
    decode_to_slice_inner(hex_chunks, &mut out).map(|()| out)
}

/// A `const fn` to decode a hex string to a fixed-length array at compile time.
/// Panics if the input was not a valid hex string.
///
/// To decode to a fixed-length array without panicking on invalid inputs, use
/// the [`FromHex`] trait instead, e.g. `<[u8; 32]>::from_hex(&s)`.
pub const fn decode_const<const N: usize>(hex: &[u8]) -> [u8; N] {
    if hex.len() != N * 2 {
        panic!("hex input is the wrong length");
    }

    let mut bytes = [0u8; N];
    let mut idx = 0;

    while idx < N {
        let hi = unwrap_const(decode_nibble(hex[2 * idx]));
        let lo = unwrap_const(decode_nibble(hex[(2 * idx) + 1]));
        let c = (hi << 4) | lo;
        bytes[idx] = c;
        idx += 1;
    }

    const fn unwrap_const(res: Result<u8, DecodeError>) -> u8 {
        match res {
            Ok(x) => x,
            Err(_) => panic!("invalid hex character"),
        }
    }

    bytes
}

/// Decodes a hex string into an output buffer.
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
    // https://github.com/RustCrypto/formats/blob/master/base16ct/src/lib.rs#L97
    decode_to_slice(hex, out)
}

/// Get a [`HexDisplay`] which provides a `Debug` and `Display` impl for the
/// given byte slice. Useful for displaying a hex value without allocating.
///
/// Example:
///
/// ```
/// use common::hex;
/// let bytes = [69u8; 32];
/// println!("Bytes as hex: {}", hex::display(&bytes));
/// ```
#[inline]
pub fn display(bytes: &[u8]) -> HexDisplay<'_> {
    HexDisplay(bytes)
}

// --- FromHex trait --- //

/// A trait to deserialize something from a hex-encoded string slice.
///
/// Examples:
///
/// ```
/// # use std::borrow::Cow;
/// use common::hex::FromHex;
/// let s = String::from("e7f51d925349a26f742e6eef3670f489aaf14fbbb5b5c3f209892f2f1baae1c9");
///
/// <Vec<u8>>::from_hex(&s).unwrap();
/// <Cow<'_, [u8]>>::from_hex(&s).unwrap();
/// <[u8; 32]>::from_hex(&s).unwrap();
/// ```
pub trait FromHex: Sized {
    fn from_hex(s: &str) -> Result<Self, DecodeError>;
}

impl FromHex for Vec<u8> {
    fn from_hex(s: &str) -> Result<Self, DecodeError> {
        decode(s)
    }
}

impl FromHex for Cow<'_, [u8]> {
    fn from_hex(s: &str) -> Result<Self, DecodeError> {
        decode(s).map(Cow::Owned)
    }
}

impl<const N: usize> FromHex for [u8; N] {
    fn from_hex(s: &str) -> Result<Self, DecodeError> {
        let mut out = [0u8; N];
        decode_to_slice(s, out.as_mut_slice())?;
        Ok(out)
    }
}

// --- HexDisplay implementation --- //

/// Provides `Debug` and `Display` impls for a byte slice.
/// Useful for displaying hex value without allocating via [`encode`].
pub struct HexDisplay<'a>(&'a [u8]);

impl<'a> fmt::Display for HexDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            f.write_char(encode_nibble(byte >> 4) as char)?;
            f.write_char(encode_nibble(byte & 0x0f) as char)?;
        }
        Ok(())
    }
}

impl<'a> fmt::Debug for HexDisplay<'a> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{self}\"")
    }
}

// --- Internal helpers --- //

fn hex_str_to_chunks(hex: &str) -> Result<&[[u8; 2]], DecodeError> {
    let (hex_chunks, extra) = hex.as_bytes().as_chunks_stable::<2>();
    if extra.is_empty() {
        Ok(hex_chunks)
    } else {
        Err(DecodeError::OddInputLength)
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

/// Encode a single nibble of hex. This encode fn is also designed to be
/// constant time.
#[inline(always)]
const fn encode_nibble(nib: u8) -> u8 {
    let mut hex = nib as i16 + (b'0' as i16);
    hex += (((b'9' as i16) - hex) >> 8) & ((b'a' as i16) - 0x3a);
    hex as u8
}

#[inline]
const fn decode_nibble(x: u8) -> Result<u8, DecodeError> {
    match x {
        b'0'..=b'9' => Ok(x - b'0'),
        b'a'..=b'f' => Ok(x - b'a' + 10),
        b'A'..=b'F' => Ok(x - b'A' + 10),
        _ => Err(DecodeError::InvalidCharacter),
    }
}

#[cfg(test)]
mod test {
    use proptest::{
        arbitrary::any, char, collection::vec, prop_assert_eq, proptest,
        strategy::Strategy,
    };

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
    fn test_decode_const() {
        const FOO: [u8; 6] = decode_const(b"01348900abff");
        assert_eq!(&FOO, &[0x01, 0x34, 0x89, 0x00, 0xab, 0xff]);
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

    #[test]
    fn test_encode_display_equiv() {
        proptest!(|(bytes: Vec<u8>)| {
            let out1 = encode(&bytes);
            let out2 = display(&bytes).to_string();
            prop_assert_eq!(out1, out2);
        });
    }
}
