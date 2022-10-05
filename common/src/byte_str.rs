use std::borrow::Borrow;
use std::{fmt, ops};

use bytes::Bytes;
use thiserror::Error;

/// `ByteStr` is just a tokio [`Bytes`], but it maintains the internal
/// invariant that the inner [`Bytes`] must be a valid utf8 string.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ByteStr(Bytes);

#[derive(Debug, Error)]
#[error("not a valid utf8 string")]
pub struct Utf8Error;

impl ByteStr {
    /// Creates a new empty `ByteStr`. This does not allocate.
    #[inline]
    pub const fn new() -> Self {
        Self(Bytes::new())
    }

    #[inline]
    pub const fn from_static(s: &'static str) -> Self {
        // INVARIANT: `s` is a string, so must be valid utf8
        Self(Bytes::from_static(s.as_bytes()))
    }

    #[inline]
    fn from_utf8_unchecked(b: Bytes) -> Self {
        if cfg!(debug_assertions) {
            match std::str::from_utf8(b.as_ref()) {
                Ok(_) => (),
                Err(err) => {
                    panic!("input is not valid utf8: err: {err}, bytes: {b:?}")
                }
            }
        }

        Self(b)
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        let b = self.0.as_ref();
        // SAFETY: the internal invariant guarantees that `b` is valid utf8
        unsafe { std::str::from_utf8_unchecked(b) }
    }

    pub fn try_from_bytes(b: Bytes) -> Result<Self, Utf8Error> {
        if std::str::from_utf8(b.as_ref()).is_ok() {
            // INVARIANT: we've just verified that `b` is valid utf8
            Ok(Self::from_utf8_unchecked(b))
        } else {
            Err(Utf8Error)
        }
    }
}

impl Default for ByteStr {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ByteStr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for ByteStr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl ops::Deref for ByteStr {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for ByteStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for ByteStr {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<String> for ByteStr {
    #[inline]
    fn from(s: String) -> Self {
        // INVARIANT: `s` is a String, so must be valid utf8
        Self::from_utf8_unchecked(Bytes::from(s))
    }
}

impl<'a> From<&'a str> for ByteStr {
    #[inline]
    fn from(s: &'a str) -> Self {
        // INVARIANT: `s` is a &str, so must be valid utf8
        Self::from_utf8_unchecked(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<ByteStr> for Bytes {
    #[inline]
    fn from(bs: ByteStr) -> Self {
        bs.0
    }
}
