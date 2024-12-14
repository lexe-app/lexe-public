use std::{
    array::TryFromSliceError,
    fmt::{self, Debug, Display},
    hash::Hash,
};

use hex::{FromHex, HexDisplay};
pub use ref_cast::RefCast;

/// A trait for types represented in memory as a byte array. Should NOT be
/// implemented for types that require validation of the byte array contents.
pub trait ByteArray<const N: usize>:
    Copy + Debug + Eq + Hash + RefCast<From = [u8; N]> + Sized
{
    // --- Required: array --- //

    fn from_array(array: [u8; N]) -> Self;
    fn to_array(&self) -> [u8; N];
    fn as_array(&self) -> &[u8; N];

    // --- Provided: array / slice / vec --- //

    fn from_array_ref(array: &[u8; N]) -> &Self {
        Self::ref_cast(array)
    }
    fn as_slice(&self) -> &[u8] {
        self.as_array().as_slice()
    }
    fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
    fn try_from_slice(slice: &[u8]) -> Result<Self, TryFromSliceError> {
        <[u8; N]>::try_from(slice).map(Self::from_array)
    }
    fn try_from_vec(vec: Vec<u8>) -> Result<Self, TryFromSliceError> {
        Self::try_from_slice(&vec)
    }

    // --- Provided: hex --- //

    fn hex_display(&self) -> HexDisplay<'_> {
        hex::display(self.as_slice())
    }
    fn try_from_hexstr(s: &str) -> Result<Self, hex::DecodeError> {
        <[u8; N]>::from_hex(s).map(Self::from_array)
    }
    fn fmt_hexstr(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&hex::display(self.as_slice()), f)
    }
}
