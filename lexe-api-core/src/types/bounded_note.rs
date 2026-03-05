use std::{
    fmt::{self, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

/// Max characters allowed in a note.
pub const MAX_NOTE_CHARS: usize = 200;
/// Max bytes allowed in a note.
pub const MAX_NOTE_BYTES: usize = 512;

/// A length-bounded note string (max 200 chars / 512 bytes).
///
/// Used for `note` (personal) and `payer_note` (payer-provided) fields on
/// payments. Construction validates that the string is within limits;
/// deserialization rejects strings that exceed either limit.
///
/// For untrusted external input that should be silently truncated rather than
/// rejected, use [`BoundedNote::truncate`].
#[derive(Clone, Eq, PartialEq, Hash, Serialize)]
pub struct BoundedNote(String);

impl BoundedNote {
    /// Validate that a string is within note length limits.
    fn validate(s: &str) -> Result<(), NoteTooLong> {
        if s.is_empty() {
            return Err(NoteTooLong::Empty);
        }

        let byte_count = s.len();
        if byte_count > MAX_NOTE_BYTES {
            return Err(NoteTooLong::TooManyBytes { bytes: byte_count });
        }

        let char_count = s.chars().count();
        if char_count > MAX_NOTE_CHARS {
            return Err(NoteTooLong::TooManyChars { chars: char_count });
        }

        Ok(())
    }

    /// Constructs a note, returning [`NoteTooLong`] if it's empty or invalid.
    pub fn new(s: String) -> Result<Self, NoteTooLong> {
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Silently truncate a string to fit within note limits.
    ///
    /// Truncates by character count first, then by byte count. Returns `None`
    /// if the result is empty after truncation.
    ///
    /// Use this for untrusted external input (e.g. inbound LNURL comments,
    /// BOLT12 payer notes) where rejecting would cause payment failures.
    pub fn truncate(mut s: String) -> Option<Self> {
        lexe_std::string::truncate_bytes(&mut s, MAX_NOTE_BYTES);
        lexe_std::string::truncate_chars(&mut s, MAX_NOTE_CHARS);
        if s.is_empty() { None } else { Some(Self(s)) }
    }

    /// Returns the note as a string slice.
    pub fn inner(&self) -> &str {
        &self.0
    }

    /// Consumes the note and returns the inner string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for BoundedNote {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}

impl FromStr for BoundedNote {
    type Err = NoteTooLong;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }
}

impl Display for BoundedNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl fmt::Debug for BoundedNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl TryFrom<String> for BoundedNote {
    type Error = NoteTooLong;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

/// Error returned when a note exceeds length limits.
#[derive(Debug, Eq, PartialEq)]
pub enum NoteTooLong {
    /// Note is empty.
    Empty,
    /// Note exceeds the character limit.
    TooManyChars { chars: usize },
    /// Note exceeds the byte limit.
    TooManyBytes { bytes: usize },
}

impl std::error::Error for NoteTooLong {}

impl fmt::Display for NoteTooLong {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Note cannot be empty."),
            Self::TooManyChars { chars } => write!(
                f,
                "Note is too long ({chars} chars). The max length is \
                 {MAX_NOTE_CHARS} chars."
            ),
            Self::TooManyBytes { bytes } => write!(
                f,
                "Note is too long ({bytes} bytes). The max length is \
                 {MAX_NOTE_BYTES} bytes."
            ),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use std::ops::RangeInclusive;

    use common::test_utils::arbitrary;
    use proptest::{
        arbitrary::Arbitrary,
        collection::vec,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for BoundedNote {
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
                        BoundedNote::new(String::from_iter(chars)).unwrap()
                    }
                ),
                // CJK (3 bytes/char): exercises the byte limit path.
                // 170 CJK chars = 510 bytes (under 512).
                vec(proptest::char::ranges(CJK.into()), 1..=170).prop_map(
                    |chars| {
                        BoundedNote::new(String::from_iter(chars)).unwrap()
                    }
                ),
                // "weird" strings (any valid UTF-8)
                arbitrary::any_string()
                    .prop_filter_map("empty", BoundedNote::truncate),
            ]
            .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;
    use proptest::{
        prop_assert_eq, proptest, strategy::Strategy, test_runner::Config,
    };

    use super::*;

    #[test]
    fn test_new_valid() {
        // Empty-ish and short
        assert!(BoundedNote::new("a".into()).is_ok());
        assert!(BoundedNote::new("hello world".into()).is_ok());

        // Exactly at char limit (200 ASCII chars = 200 bytes)
        let at_limit = "a".repeat(MAX_NOTE_CHARS);
        assert!(BoundedNote::new(at_limit).is_ok());

        // Over char limit: 512 ASCII chars = 512 bytes, but 512 > 200 chars
        let at_byte_limit = "a".repeat(MAX_NOTE_BYTES);
        assert!(BoundedNote::new(at_byte_limit).is_err());
    }

    #[test]
    fn test_new_rejects_oversized() {
        assert_eq!(
            BoundedNote::new(String::new()).unwrap_err(),
            NoteTooLong::Empty,
        );

        // Over char limit
        let too_many_chars = "a".repeat(MAX_NOTE_CHARS + 1);
        assert_eq!(
            BoundedNote::new(too_many_chars).unwrap_err(),
            NoteTooLong::TooManyChars { chars: 201 },
        );

        // Under char limit but over byte limit (CJK: 3 bytes each)
        // 171 CJK chars = 171 chars (< 200) but 513 bytes (> 512)
        let over_bytes: String = "日".repeat(171);
        assert_eq!(over_bytes.chars().count(), 171);
        assert_eq!(over_bytes.len(), 513);
        assert_eq!(
            BoundedNote::new(over_bytes).unwrap_err(),
            NoteTooLong::TooManyBytes { bytes: 513 },
        );
    }

    #[test]
    fn test_truncate() {
        // Normal case
        let note = BoundedNote::truncate("hello".into());
        assert_eq!(note.as_ref().map(BoundedNote::inner), Some("hello"));

        // Empty → None
        assert!(BoundedNote::truncate(String::new()).is_none());

        // Over char limit: 250 ASCII chars → truncated to 200
        let long = "a".repeat(250);
        let note = BoundedNote::truncate(long).unwrap();
        assert_eq!(note.inner().len(), 200);

        // Over byte limit: 200 CJK chars = 600 bytes → truncated
        let cjk = "日".repeat(200);
        let note = BoundedNote::truncate(cjk).unwrap();
        assert!(note.inner().len() <= MAX_NOTE_BYTES);
        assert!(note.inner().chars().count() <= MAX_NOTE_CHARS);
    }

    #[test]
    fn test_serde_roundtrip() {
        // JSON string roundtrip: valid notes survive ser/de
        let note = BoundedNote::new("test note".into()).unwrap();
        let json = serde_json::to_string(&note).unwrap();
        assert_eq!(json, "\"test note\"");
        let back: BoundedNote = serde_json::from_str(&json).unwrap();
        assert_eq!(note, back);

        // Oversized strings are rejected on deserialization
        let oversized = format!("\"{}\"", "x".repeat(201));
        serde_json::from_str::<BoundedNote>(&oversized).unwrap_err();
    }

    #[test]
    fn test_json_string_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<BoundedNote>();
    }

    #[test]
    fn test_fromstr_display_roundtrip() {
        let cases = ["a", "hello world", "日本語", "a note with spaces"];
        for input in cases {
            let note = BoundedNote::from_str(input).unwrap();
            assert_eq!(input, note.to_string());
        }
    }

    #[test]
    fn test_fromstr_display_roundtrip_proptest() {
        proptest!(|(b in proptest::arbitrary::any::<BoundedNote>())| {
            let roundtripped = BoundedNote::from_str(&b.to_string()).unwrap();
            prop_assert_eq!(roundtripped, b);
        });
    }

    #[test]
    fn test_fromstr_json_string_equiv() {
        let strategy = proptest::arbitrary::any::<BoundedNote>().prop_filter(
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
