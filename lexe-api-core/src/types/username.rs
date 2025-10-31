use core::fmt;
use std::{fmt::Display, str::FromStr};

use serde::{Deserialize, Serialize};

const USERNAME_MAX_LENGTH: usize = 24;
const USERNAME_MIN_LENGTH: usize = 1;

/// A validated username.
///
/// Wraps a [`String`] to enforce username validations.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Username(String);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsernameStruct {
    pub username: Username,
}

impl Username {
    /// Validate a username string.
    fn validate(s: &str) -> Result<(), ParseError> {
        let valid_min_length = s.len() >= USERNAME_MIN_LENGTH;
        let valid_max_length = s.len() <= USERNAME_MAX_LENGTH;
        if !valid_min_length {
            return Err(ParseError::InvalidMinLength);
        }
        if !valid_max_length {
            return Err(ParseError::InvalidMaxLength);
        }

        let valid_characters = s
            .as_bytes()
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));

        if !valid_characters {
            return Err(ParseError::InvalidCharacters);
        }

        let first_char = s.as_bytes().first().expect("Checked length above");
        if first_char == &b'-' {
            return Err(ParseError::StartsWithHyphen);
        }

        let last_char = s.as_bytes().last().expect("Checked length above");
        if last_char == &b'-' {
            return Err(ParseError::EndsWithHyphen);
        }

        let valid_sequence = s
            .as_bytes()
            .windows(2)
            .all(|w| w[0] != b'-' || w[1] != b'-');

        if !valid_sequence {
            return Err(ParseError::ConsecutiveHyphens);
        }

        Ok(())
    }

    /// Parse and validate a username string.
    pub fn parse(s: &str) -> Result<Self, ParseError> {
        Self::validate(s)?;
        Ok(Self(s.to_owned()))
    }

    /// Returns the username as a string slice.
    pub fn inner(&self) -> &str {
        &self.0
    }

    /// Returns the username as a string.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for Username {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::try_from(s).map_err(serde::de::Error::custom)
    }
}

impl FromStr for Username {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Username::parse(s)
    }
}

impl Display for Username {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl TryFrom<String> for Username {
    type Error = ParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::validate(&s)?;
        Ok(Self(s))
    }
}

/// Username validation error.
#[derive(Debug, Eq, PartialEq)]
pub enum ParseError {
    /// Username must be at least 1 character.
    InvalidMinLength,
    /// Username must be at most 24 characters.
    InvalidMaxLength,
    /// Username contains invalid characters (only lowercase alphanumeric and
    /// hyphens allowed).
    InvalidCharacters,
    /// Username cannot start with a hyphen.
    StartsWithHyphen,
    /// Username cannot end with a hyphen.
    EndsWithHyphen,
    /// Username cannot contain consecutive hyphens.
    ConsecutiveHyphens,
}

impl std::error::Error for ParseError {}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMinLength =>
                write!(f, "Username must be at least 1 character"),
            Self::InvalidMaxLength =>
                write!(f, "Username must be at most 24 characters"),
            Self::InvalidCharacters => write!(
                f,
                "Username contains invalid characters (only lowercase \
                 alphanumeric and hyphens are allowed)"
            ),
            Self::StartsWithHyphen =>
                write!(f, "Username cannot start with a hyphen"),
            Self::EndsWithHyphen =>
                write!(f, "Username cannot end with a hyphen"),
            Self::ConsecutiveHyphens =>
                write!(f, "Username cannot contain consecutive hyphens"),
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
pub mod arbitrary_impl {

    use std::ops::RangeInclusive;

    use proptest::{
        arbitrary::Arbitrary,
        collection::vec,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for Username {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        // We use this generation instead of regex to not add a dependency
        // on std on other environments.
        // TODO(maurice): Use regex if have the std flag.
        //  REGEX "[a-z0-9]([a-z0-9]|[a-z0-9]-[a-z0-9]){0,22}[a-z0-9]?"
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            fn dash_collapse(s: String) -> String {
                let mut out = String::with_capacity(s.len());
                for c in s.chars() {
                    if c != '-' || !(out.is_empty() || out.ends_with('-')) {
                        out.push(c);
                    }
                }
                out
            }

            static RANGES: &[RangeInclusive<char>] =
                &['-'..='-', '0'..='9', 'a'..='z'];
            let any_username_char = proptest::char::ranges(RANGES.into());
            vec(any_username_char, 1..=24)
                .prop_map(String::from_iter)
                .prop_map(dash_collapse)
                .prop_map(|s| {
                    if s.ends_with("-") || s.is_empty() {
                        Username::try_from("a".to_string()).unwrap()
                    } else {
                        Username::try_from(s).unwrap()
                    }
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::test_utils::roundtrip;

    use super::*;

    #[test]
    fn test_parse_valid() {
        let valid_cases = [
            "a",
            "user",
            "alice",
            "test-user",
            "user-name",
            "123",
            "user123",
            "test-123",
            "a-b-c",
            "user-test-name",
            "123456789012345678901234",
        ];

        for input in valid_cases {
            assert!(
                Username::parse(input).is_ok(),
                "Should parse valid username: {input}"
            );
        }
    }

    #[test]
    fn test_parse_invalid_length() {
        assert_eq!(
            Username::parse("").unwrap_err(),
            ParseError::InvalidMinLength
        );
        assert_eq!(
            Username::parse("1234567890123456789012345").unwrap_err(),
            ParseError::InvalidMaxLength
        );
    }

    #[test]
    fn test_parse_invalid_characters() {
        let invalid_cases = [
            "User",
            "USER",
            "user_name",
            "user.name",
            "user@name",
            "user name",
            "user+tag",
            "user!",
            "user#",
            "user$",
        ];

        for input in invalid_cases {
            assert_eq!(
                Username::parse(input).unwrap_err(),
                ParseError::InvalidCharacters,
                "Should fail with InvalidCharacters: {input}"
            );
        }
    }

    #[test]
    fn test_parse_invalid_first_character() {
        assert_eq!(
            Username::parse("-user").unwrap_err(),
            ParseError::StartsWithHyphen
        );
        assert_eq!(
            Username::parse("-123").unwrap_err(),
            ParseError::StartsWithHyphen
        );
        assert_eq!(
            Username::parse("-").unwrap_err(),
            ParseError::StartsWithHyphen
        );
    }

    #[test]
    fn test_parse_invalid_last_character() {
        assert_eq!(
            Username::parse("user-").unwrap_err(),
            ParseError::EndsWithHyphen
        );
        assert_eq!(
            Username::parse("123-").unwrap_err(),
            ParseError::EndsWithHyphen
        );
    }

    #[test]
    fn test_parse_invalid_sequence() {
        assert_eq!(
            Username::parse("user--name").unwrap_err(),
            ParseError::ConsecutiveHyphens
        );
        assert_eq!(
            Username::parse("a---b").unwrap_err(),
            ParseError::ConsecutiveHyphens
        );
        assert_eq!(
            Username::parse("test--123").unwrap_err(),
            ParseError::ConsecutiveHyphens
        );
    }

    #[test]
    fn test_fromstr_display_roundtrip() {
        let valid_cases =
            ["a", "user", "test-user", "user123", "123", "a-b-c-d-e-f"];

        for input in valid_cases {
            let username = Username::from_str(input).unwrap();
            let output = username.to_string();
            assert_eq!(input, output);
        }
    }

    #[test]
    fn test_arbitrary_roundtrip() {
        roundtrip::fromstr_json_string_equiv::<Username>();
    }
}
