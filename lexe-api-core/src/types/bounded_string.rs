use std::{
    fmt::{self, Display},
    ops::Deref,
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::types::username::USERNAME_MAX_LENGTH;

/// A length-bounded string (max 200 chars / 512 bytes).
///
/// Used for `message` (payer-provided) and `personal_note` fields on
/// payments. Construction validates that the string is within limits;
/// deserialization rejects strings that exceed either limit.
///
/// For untrusted external input that should be silently truncated rather than
/// rejected, use [`BoundedString::truncate`].
#[derive(Clone, Eq, PartialEq, Hash, Serialize)]
pub struct BoundedString(String);

// Assert that a Lexe issuer (e.g. "username@lexe.app") fits within limits.
lexe_std::const_assert!(
    BoundedString::MAX_CHARS >= USERNAME_MAX_LENGTH + "@lexe.app".len()
);

impl BoundedString {
    /// Max characters allowed.
    pub const MAX_CHARS: usize = 200;
    /// Max bytes allowed.
    pub const MAX_BYTES: usize = 512;

    /// Validate that a string is within length limits.
    fn validate(s: &str) -> Result<(), StringTooLong> {
        if s.is_empty() {
            return Err(StringTooLong::Empty);
        }

        let byte_count = s.len();
        if byte_count > Self::MAX_BYTES {
            return Err(StringTooLong::TooManyBytes { bytes: byte_count });
        }

        let char_count = s.chars().count();
        if char_count > Self::MAX_CHARS {
            return Err(StringTooLong::TooManyChars { chars: char_count });
        }

        Ok(())
    }

    /// Constructs a bounded string, returning [`StringTooLong`] if it's empty
    /// or exceeds limits.
    pub fn new(s: String) -> Result<Self, StringTooLong> {
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Silently truncate a string to fit within limits.
    ///
    /// Truncates by character count first, then by byte count. Returns `None`
    /// if the result is empty after truncation.
    ///
    /// Use this for untrusted external input (e.g. inbound LNURL comments,
    /// BOLT12 payer notes) where rejecting would cause payment failures.
    pub fn truncate(mut s: String) -> Option<Self> {
        lexe_std::string::truncate_bytes(&mut s, Self::MAX_BYTES);
        lexe_std::string::truncate_chars(&mut s, Self::MAX_CHARS);
        if s.is_empty() { None } else { Some(Self(s)) }
    }

    /// Returns the string as a string slice.
    pub fn inner(&self) -> &str {
        &self.0
    }

    /// Consumes and returns the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for BoundedString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}

impl FromStr for BoundedString {
    type Err = StringTooLong;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl Display for BoundedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Deref for BoundedString {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Debug for BoundedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl TryFrom<String> for BoundedString {
    type Error = StringTooLong;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

/// Error returned when a string exceeds length limits.
#[derive(Debug, Eq, PartialEq)]
pub enum StringTooLong {
    /// String is empty.
    Empty,
    /// String exceeds the character limit.
    TooManyChars { chars: usize },
    /// String exceeds the byte limit.
    TooManyBytes { bytes: usize },
}

impl std::error::Error for StringTooLong {}

impl fmt::Display for StringTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let max_bytes = BoundedString::MAX_BYTES;
        let max_chars = BoundedString::MAX_CHARS;
        match self {
            Self::Empty =>
                f.write_str("String is optional but cannot be empty."),
            Self::TooManyChars { chars } => write!(
                f,
                "String is too long ({chars} chars). The max length is \
                 {max_chars} chars.",
            ),
            Self::TooManyBytes { bytes } => write!(
                f,
                "String is too long ({bytes} bytes). The max length is \
                 {max_bytes} bytes.",
            ),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use std::ops::RangeInclusive;

    use lexe_common::test_utils::arbitrary;
    use proptest::{
        arbitrary::Arbitrary,
        collection::vec,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for BoundedString {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            static ASCII: &[RangeInclusive<char>] =
                &['a'..='z', 'A'..='Z', '0'..='9', ' '..=' '];
            // CJK Unified Ideographs (3 bytes/char).
            static CJK: &[RangeInclusive<char>] = &['\u{4e00}'..='\u{9fff}'];

            prop_oneof![
                // ASCII (1 byte/char): exercises the char limit only.
                vec(proptest::char::ranges(ASCII.into()), 1..=200).prop_map(
                    |chars| {
                        BoundedString::new(String::from_iter(chars)).unwrap()
                    }
                ),
                // CJK (3 bytes/char): exercises the byte limit path.
                // 170 CJK chars = 510 bytes (under 512).
                vec(proptest::char::ranges(CJK.into()), 1..=170).prop_map(
                    |chars| {
                        BoundedString::new(String::from_iter(chars)).unwrap()
                    }
                ),
                // "weird" strings (any valid UTF-8)
                arbitrary::any_string()
                    .prop_filter_map("empty", BoundedString::truncate),
            ]
            .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;
    use proptest::{
        prop_assert_eq, proptest, strategy::Strategy, test_runner::Config,
    };

    use super::*;

    #[test]
    fn test_new_valid() {
        // Empty-ish and short
        assert!(BoundedString::new("a".into()).is_ok());
        assert!(BoundedString::new("hello world".into()).is_ok());

        // Exactly at char limit (200 ASCII chars = 200 bytes)
        let at_limit = "a".repeat(BoundedString::MAX_CHARS);
        assert!(BoundedString::new(at_limit).is_ok());

        // Over char limit: 512 ASCII chars = 512 bytes, but 512 > 200 chars
        let at_byte_limit = "a".repeat(BoundedString::MAX_BYTES);
        assert!(BoundedString::new(at_byte_limit).is_err());
    }

    #[test]
    fn test_new_rejects_oversized() {
        assert_eq!(
            BoundedString::new(String::new()).unwrap_err(),
            StringTooLong::Empty,
        );

        // Over char limit
        let too_many_chars = "a".repeat(BoundedString::MAX_CHARS + 1);
        assert_eq!(
            BoundedString::new(too_many_chars).unwrap_err(),
            StringTooLong::TooManyChars { chars: 201 },
        );

        // Under char limit but over byte limit (CJK: 3 bytes each)
        // 171 CJK chars = 171 chars (< 200) but 513 bytes (> 512)
        let over_bytes: String = "日".repeat(171);
        assert_eq!(over_bytes.chars().count(), 171);
        assert_eq!(over_bytes.len(), 513);
        assert_eq!(
            BoundedString::new(over_bytes).unwrap_err(),
            StringTooLong::TooManyBytes { bytes: 513 },
        );
    }

    #[test]
    fn test_truncate() {
        // Normal case
        let s = BoundedString::truncate("hello".into());
        assert_eq!(s.as_ref().map(BoundedString::inner), Some("hello"));

        // Empty → None
        assert!(BoundedString::truncate(String::new()).is_none());

        // Over char limit: 250 ASCII chars → truncated to 200
        let long = "a".repeat(250);
        let s = BoundedString::truncate(long).unwrap();
        assert_eq!(s.inner().len(), 200);

        // Over byte limit: 200 CJK chars = 600 bytes → truncated
        let cjk = "日".repeat(200);
        let s = BoundedString::truncate(cjk).unwrap();
        assert!(s.inner().len() <= BoundedString::MAX_BYTES);
        assert!(s.inner().chars().count() <= BoundedString::MAX_CHARS);
    }

    #[test]
    fn test_serde_roundtrip() {
        // JSON string roundtrip: valid strings survive ser/de
        let s = BoundedString::new("test string".into()).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "\"test string\"");
        let back: BoundedString = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);

        // Oversized strings are rejected on deserialization
        let oversized = format!("\"{}\"", "x".repeat(201));
        serde_json::from_str::<BoundedString>(&oversized).unwrap_err();
    }

    #[test]
    fn test_json_string_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<BoundedString>();
    }

    #[test]
    fn test_fromstr_display_roundtrip() {
        let cases = ["a", "hello world", "日本語", "a string with spaces"];
        for input in cases {
            let s = BoundedString::from_str(input).unwrap();
            assert_eq!(input, s.to_string());
        }
    }

    #[test]
    fn test_fromstr_display_roundtrip_proptest() {
        proptest!(|(b in proptest::arbitrary::any::<BoundedString>())| {
            let roundtripped = BoundedString::from_str(&b.to_string()).unwrap();
            prop_assert_eq!(roundtripped, b);
        });
    }

    #[test]
    fn test_fromstr_json_string_equiv() {
        let strategy = proptest::arbitrary::any::<BoundedString>().prop_filter(
            "json-string-safe display",
            |b| {
                b.inner()
                    .chars()
                    .all(|c| !c.is_control() && c != '"' && c != '\\')
            },
        );
        roundtrip::fromstr_json_string_equiv_custom(
            strategy,
            Config::default(),
        );
    }
}
