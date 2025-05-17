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

/// Impls [`ByteArray`] for a transparent newtype over `[u8; N]`
///
/// ```ignore
/// byte_array::impl_byte_array!(Measurement, 32);
/// ```
#[macro_export]
macro_rules! impl_byte_array {
    ($type:ty, $n:expr) => {
        impl ByteArray<$n> for $type {
            fn from_array(array: [u8; $n]) -> Self {
                Self(array)
            }
            fn to_array(&self) -> [u8; $n] {
                self.0
            }
            fn as_array(&self) -> &[u8; $n] {
                &self.0
            }
        }
    };
}

/// Impls `FromStr` and `FromHex` for a [`ByteArray`] parsed from a hex string.
///
/// ```ignore
/// byte_array::impl_fromstr_fromhex!(Measurement);
/// ```
#[macro_export]
macro_rules! impl_fromstr_fromhex {
    ($type:ty, $n:expr) => {
        impl std::str::FromStr for $type {
            type Err = hex::DecodeError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::try_from_hexstr(s)
            }
        }
        impl hex::FromHex for $type {
            fn from_hex(s: &str) -> Result<Self, hex::DecodeError> {
                <[u8; $n]>::from_hex(s).map(Self::from_array)
            }
        }
    };
}

/// Impls Debug + Display for a [`ByteArray`] type formatted as a hex string.
///
/// ```ignore
/// byte_array::impl_debug_display_hex!(Measurement);
/// ```
#[macro_export]
macro_rules! impl_debug_display_as_hex {
    ($type:ty) => {
        impl std::fmt::Debug for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                // We don't implement this like
                // `f.debug_tuple(stringify!($type)).field(&self.0).finish()`
                // because that includes useless newlines when pretty printing.
                write!(f, "{}(\"{}\")", stringify!($type), self.hex_display())
            }
        }
        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Self::fmt_hexstr(self, f)
            }
        }
    };
}

/// Impls Debug + Display with secret values redacted.
/// Useful for preventing the accidental leakage of secrets in logs.
/// Can technically be used for non [`ByteArray`] types as well.
///
/// ```ignore
/// byte_array::impl_debug_display_redacted!(PaymentSecret);
/// ```
#[macro_export]
macro_rules! impl_debug_display_redacted {
    ($type:ty) => {
        impl std::fmt::Debug for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($type), "(..)"))
            }
        }
        impl std::fmt::Display for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("..")
            }
        }
    };
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast)]
    #[repr(transparent)]
    struct MyStruct([u8; 4]);

    impl_byte_array!(MyStruct, 4);
    impl_fromstr_fromhex!(MyStruct, 4);
    impl_debug_display_as_hex!(MyStruct);

    #[derive(Copy, Clone, Eq, PartialEq, Hash, RefCast)]
    #[repr(transparent)]
    struct MySecret([u8; 4]);

    impl_byte_array!(MySecret, 4);
    impl_fromstr_fromhex!(MySecret, 4);
    impl_debug_display_redacted!(MySecret);

    #[test]
    fn test_display_and_debug() {
        let data = [0xde, 0xad, 0xbe, 0xef];

        // Test regular display/debug
        let my_struct = MyStruct(data);
        assert_eq!(my_struct.to_string(), "deadbeef");
        assert_eq!(format!("{my_struct}"), "deadbeef");
        assert_eq!(format!("{my_struct:#}"), "deadbeef");
        assert_eq!(format!("{:?}", my_struct), r#"MyStruct("deadbeef")"#);
        assert_eq!(format!("{:#?}", my_struct), r#"MyStruct("deadbeef")"#);

        // Test redacted display/debug
        let my_secret = MySecret(data);
        assert_eq!(my_secret.to_string(), "..");
        assert_eq!(format!("My secret is {my_secret}"), "My secret is ..");
        assert_eq!(format!("My secret is {my_secret:#}"), "My secret is ..");
        assert_eq!(format!("{:?}", my_secret), r#"MySecret(..)"#);
        assert_eq!(format!("{:#?}", my_secret), r#"MySecret(..)"#);
    }

    #[test]
    fn basic_parse() {
        // Valid cases
        let my_struct = MyStruct::from_str("deadbeef").unwrap();
        assert_eq!(my_struct.0, [0xde, 0xad, 0xbe, 0xef]);

        let my_secret = MySecret::from_str("deadbeef").unwrap();
        assert_eq!(my_secret.0, [0xde, 0xad, 0xbe, 0xef]);

        // Error cases
        MyStruct::from_str("invalid").unwrap_err();
        MyStruct::from_str("deadbee").unwrap_err(); // Too short
        MyStruct::from_str("deadbeefff").unwrap_err(); // Too long
        MyStruct::from_str("wxyz").unwrap_err(); // Not hex
    }
}
