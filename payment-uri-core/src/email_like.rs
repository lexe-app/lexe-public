//! Email-like payment address parsing and validation.
//!
//! This module parses both BIP353 and Lightning Address URIs, which share an
//! email-like structure of `username@domain` but have slightly different
//! validation requirements.
//!
//! ## BIP 353
//!
//! - `username` *may* include the ₿ prefix; not required.
//! - `username` must be a valid DNS label.
//! - `domain` must be a valid DNS name.
//! - `username` and `domain` are case-insensitive in DNS, but any uppercase
//!   letters are normalized to lowercase during parsing.
//! - Only ASCII characters are supported (strict LDH rules: Letters, Digits,
//!   Hyphens). We do not support Punycode (IDN) or UTF-8 domain names.
//!
//! ## Lightning Address
//!
//! - Per [LUD-16], `username` is limited to lowercase alphanumeric characters,
//!   `-`, `_`, `.`, and `+` for tags. However, we parse permissively, accepting
//!   uppercase letters and normalizing them to lowercase during parsing.
//! - `domain` must be a valid DNS name.
//!
//! # Resolution:
//!
//! BIP353: Do DNS lookup for [`EmailLikeAddress::bip353_fqdn`], i.e.
//! `{username}.user._bitcoin-payment.{domain}.`
//!
//! Lightning Address: HTTPS GET [`EmailLikeAddress::lightning_address_url`],
//! i.e. `https://{domain}/.well-known/lnurlp/{username}`.
//!
//! [LUD-16]: <https://github.com/fiatjaf/lnurl-rfc/blob/luds/16.md>
//!
//! # AI prompts
//!
//! ```md
//! Please read these files:
//! - `bitcoin-payment-instructions/src/http_resolver.rs`
//! - `bips/bip-0353.mediawiki`
//! - `bips/bip-0321.mediawiki`
//! ```

use std::{borrow::Cow, fmt};

use crate::Error;

/// Maximum DNS name length in text form (excluding trailing dot), per
/// [RFC 2181 §11](https://www.rfc-editor.org/rfc/rfc2181#section-11).
const MAX_DNS_NAME_LEN: usize = 253;

/// Email-like human-readable payment address: BIP353 or Lightning Address.
/// String fields are guaranteed to be lowercase (normalized during parsing).
// TODO(max): use `Cow<'a, [ascii::Char]>` for these fields once stable.
#[derive(Debug)]
#[cfg_attr(test, derive(Eq, PartialEq))]
pub struct EmailLikeAddress<'a> {
    /// `<username>@...`
    pub username: Cow<'a, str>,
    /// `...@<domain>`
    pub domain: Cow<'a, str>,
    /// Lightning Address supports an optional +tag suffix on the username.
    pub tag: Option<Cow<'a, str>>,

    /// Whether the original input had a leading ₿ character, implying the
    /// receiver very likely intended for this to be a BIP353 URI.
    pub bip353_prefix: bool,

    /// If this is a valid BIP353 address, contains the Fully Qualified Domain
    /// Name (FQDN) where the resolver should look for the BIP353 TXT record.
    pub bip353_fqdn: Option<String>,

    /// The HTTPS URL where the Lightning Address LNURL-pay endpoint (LUD-06)
    /// should be queried.
    pub lightning_address_url: String,
}

/// Displays the username as `[₿]username[+tag]@domain`.
impl fmt::Display for EmailLikeAddress<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bip353_prefix {
            write!(f, "₿")?;
        }

        write!(f, "{username}", username = &self.username)?;

        if let Some(tag) = &self.tag {
            write!(f, "+{tag}")?;
        }

        write!(f, "@{domain}", domain = &self.domain)?;

        Ok(())
    }
}

impl<'a> EmailLikeAddress<'a> {
    pub fn into_owned(self) -> EmailLikeAddress<'static> {
        EmailLikeAddress {
            username: Cow::Owned(self.username.into_owned()),
            domain: Cow::Owned(self.domain.into_owned()),
            tag: self.tag.map(|t| Cow::Owned(t.into_owned())),
            bip353_prefix: self.bip353_prefix,
            bip353_fqdn: self.bip353_fqdn,
            lightning_address_url: self.lightning_address_url,
        }
    }

    /// Whether the payment uri "looks like" an email address and should be
    /// parsed as such. If so, returns the trimmed username and domain parts.
    pub(crate) fn matches(s: &str) -> Option<(&str, &str)> {
        s.split_once('@')
    }

    /// Parses an email-like address.
    pub fn parse(s: &'a str) -> Result<Self, Error> {
        let (unstripped_username, domain) = Self::matches(s)
            .ok_or(Cow::from("Must contain '@' character"))
            .map_err(Error::InvalidEmailLike)?;

        Self::parse_from_parts(unstripped_username, domain)
    }

    /// Parse an email-like address from its username and domain parts.
    ///
    /// The unstripped username may include:
    /// - a leading ₿ or %E2%82%BF prefix, and
    /// - a +tag suffix.
    pub(crate) fn parse_from_parts(
        unstripped_username: &'a str,
        domain: &'a str,
    ) -> Result<Self, Error> {
        // Strip out Bitcoin symbol prefix:
        // Either UTF-8 '₿' or URL-encoded '%E2%82%BF'
        let (bip353_prefix, username_with_tag) = if let Some(rem) =
            unstripped_username.strip_prefix('₿')
        {
            (true, rem)
        } else if let Some(rem) = unstripped_username.strip_prefix("%E2%82%BF")
        {
            (true, rem)
        } else {
            (false, unstripped_username)
        };

        // Strip out Lightning address tag (follows the '+' character)
        let (username, tag) = username_with_tag
            .split_once('+')
            .map(|(u, t)| (u, Some(t)))
            .unwrap_or((username_with_tag, None));

        if username.is_empty() {
            return Err(Error::InvalidEmailLike(Cow::from(
                "username cannot be empty",
            )));
        }
        if domain.is_empty() {
            return Err(Error::InvalidEmailLike(Cow::from(
                "domain part cannot be empty",
            )));
        }

        // LUD-16 apparently supports .onion domains (BIP353 does not).
        // We could maybe possibly™ support them in the future. Conceivably.
        if domain.ends_with(".onion") {
            return Err(Error::InvalidEmailLike(Cow::from(
                ".onion domains are not supported",
            )));
        }

        // First, try to parse as BIP353, which is strictly more constrained
        // than Lightning Address and doesn't support tags. Early return if
        // success.
        let bip353_error = if tag.is_none() {
            match validate::bip353(username, domain) {
                Ok((validated_username, validated_domain, bip353_fqdn)) => {
                    let lightning_address_url =
                        construct::lightning_address_url(
                            &validated_username,
                            None,
                            &validated_domain,
                        );
                    return Ok(Self {
                        username: validated_username,
                        domain: validated_domain,
                        tag: None,
                        bip353_prefix,
                        bip353_fqdn: Some(bip353_fqdn),
                        lightning_address_url,
                    });
                }
                Err(error) => Some(error),
            }
        } else {
            None
        };

        // BIP353 validation failed. Try parsing as Lightning Address.
        // Construct EmailLikeAddress with the normalized values.
        validate::lightning_address(username, tag, domain)
            .map(|(username, tag, domain)| {
                let lightning_address_url = construct::lightning_address_url(
                    &username,
                    tag.as_deref(),
                    &domain,
                );

                Self {
                    username,
                    domain,
                    tag,
                    bip353_prefix,
                    bip353_fqdn: None,
                    lightning_address_url,
                }
            })
            // If we have a BIP353 error and the input had a ₿ prefix, return
            // only the BIP353 error. Otherwise, return a combined error, as we
            // don't know which payment URI the recipient intended.
            .map_err(|ln_error| {
                if let Some(ref bip353_error) = bip353_error
                    && bip353_prefix
                {
                    return Error::InvalidEmailLike(Cow::from(format!(
                        "{bip353_error:#}"
                    )));
                }

                let combined_msg = match bip353_error {
                    Some(bip353_error) =>
                        format!("{bip353_error:#}; {ln_error:#}"),
                    None => format!("{ln_error:#}"),
                };
                Error::InvalidEmailLike(Cow::from(combined_msg))
            })
    }
}

/// Helpers to validate components of email-like addresses and DNS names.
mod validate {
    use std::borrow::Cow;

    use anyhow::{anyhow, ensure, Context};

    // Use consistent `validate::` prefix inside this module as well
    use super::{construct, validate, MAX_DNS_NAME_LEN};

    /// Validate the username and domain of a BIP353 address,
    /// returning the validated and normalized username and domain, and the
    /// BIP353 FQDN where the resolver should look for the TXT record.
    /// - `username` should be passed without the ₿ prefix.
    /// - `username` must be a valid DNS label following strict LDH rules.
    /// - `domain` must be a DNS name following strict LDH rules.
    /// - `username` and `domain` are case-insensitive.
    ///
    /// NOTE: That this doesn't check for a tag, which should be [`None`].
    pub(super) fn bip353<'a>(
        username: &'a str,
        domain: &'a str,
    ) -> anyhow::Result<(Cow<'a, str>, Cow<'a, str>, String)> {
        let username = {
            let has_uppercase = validate::dns_label(username)
                .context("invalid username in BIP353 address")?;
            if has_uppercase {
                Cow::Owned(username.to_ascii_lowercase())
            } else {
                Cow::Borrowed(username)
            }
        };

        let domain = validate::dns_name(domain)
            .context("invalid domain in BIP353 address")?;

        // Check that the BIP353 FQDN isn't too long. We don't need to
        // re-validate the labels since we've already validated username and
        // domain, and `.user._bitcoin-payment.` is a protocol constant.
        let bip353_fqdn = construct::bip353_fqdn(&username, &domain);
        validate::dns_name_length(&bip353_fqdn)
            .context("Fully qualified BIP353 DNS name is too long")?;

        Ok((username, domain, bip353_fqdn))
    }

    /// Validate the username, optional tag, and domain of a Lightning Address,
    /// returning the validated and normalized components.
    /// - `username` is limited to `a-zA-Z0-9-_.` (no `+` allowed in username).
    /// - `tag` (if present) is limited to `a-zA-Z0-9-_.` (no `+` in tag).
    /// - `domain` must be a DNS name following strict LDH rules.
    /// - `username`, `tag`, and `domain` are case-insensitive.
    pub(super) fn lightning_address<'a>(
        username: &'a str,
        tag: Option<&'a str>,
        domain: &'a str,
    ) -> anyhow::Result<(Cow<'a, str>, Option<Cow<'a, str>>, Cow<'a, str>)>
    {
        let username = validate::lightning_address_username_or_tag(username)
            .context("invalid username in Lightning Address")?;

        let tag = match tag {
            Some(t) => validate::lightning_address_username_or_tag(t)
                .map(Some)
                .context("invalid tag in Lightning Address")?,
            None => None,
        };

        let domain = validate::dns_name(domain)
            .context("invalid domain in Lightning Address")?;

        Ok((username, tag, domain))
    }

    /// Validates the username or tag component of a Lightning Address,
    /// returning the validated and normalized component.
    ///
    /// Per LUD-16, usernames and tags are limited to lowercase alphanumeric
    /// characters, `-`, `_`, and `.`. The `+` character is used as a separator
    /// between username and tag, so it's not allowed within components
    /// themselves. We parse permissively, accepting uppercase letters and
    /// normalizing to lowercase if an uppercase letter is found.
    // LUD-16:
    //
    // "The `<username>` is limited to `a-z0-9-_.`
    // (and `+` if the `SERVICE` supports tags)"
    //
    // <https://github.com/fiatjaf/lnurl-rfc/blob/luds/16.md>
    pub(super) fn lightning_address_username_or_tag(
        s: &str,
    ) -> anyhow::Result<Cow<'_, str>> {
        ensure!(!s.is_empty(), "component cannot be empty");

        // Whether the output needs to be normalized to lowercase.
        let mut has_uppercase = false;

        for c in s.chars() {
            let is_valid_char = matches!(
                c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '.' | '_'
            );

            has_uppercase |= c.is_ascii_uppercase();

            ensure!(
                is_valid_char,
                "Lightning Address username/tag {s} must only contain \
                 a-z, A-Z, 0-9, '-', '.', and '_'. \
                 Found invalid character '{c}'."
            );
        }

        if has_uppercase {
            Ok(Cow::Owned(s.to_ascii_lowercase()))
        } else {
            Ok(Cow::Borrowed(s))
        }
    }

    /// Validates a DNS name, returning the validated and normalized DNS name,
    /// with any trailing dot (FQDN indicator) stripped.
    pub(super) fn dns_name(name: &str) -> anyhow::Result<Cow<'_, str>> {
        // Validate length and strip off any trailing dot (FQDN indicator)
        let stripped = validate::dns_name_length(name)?;

        let mut has_uppercase = false;

        // Validate each label (also enforces ≤63 bytes per label)
        for label in stripped.split('.') {
            has_uppercase |= validate::dns_label(label)
                .with_context(|| format!("Invalid label '{label}'"))?;
        }

        // Normalize to lowercase if needed
        if has_uppercase {
            Ok(Cow::Owned(stripped.to_ascii_lowercase()))
        } else {
            Ok(Cow::Borrowed(stripped))
        }
    }

    /// Validates DNS name length only, returning the DNS name with any trailing
    /// dot (FQDN indicator) stripped.
    ///
    /// DNS names are limited to 253 characters in text form (excluding
    /// optional trailing dot) and 255 bytes in wire format ([RFC 2181 §11]).
    ///
    /// Wire format structure ([RFC 1035 §3.1]):
    /// - Each label is encoded as: 1 length byte + label bytes
    /// - The name is terminated with a zero byte (root label)
    /// - Example: "www.example.com" → `[3]www[7]example[3]com[0]`
    /// - Wire length = (1+3) + (1+7) + (1+3) + 1 = 16 bytes
    ///
    /// [RFC 2181 §11]: https://www.rfc-editor.org/rfc/rfc2181#section-11
    /// [RFC 1035 §3.1]: https://datatracker.ietf.org/doc/html/rfc1035#section-3.1
    pub(super) fn dns_name_length(name: &str) -> anyhow::Result<&str> {
        ensure!(!name.is_empty(), "DNS name cannot be empty");

        // Strip optional trailing dot (FQDN indicator)
        let stripped = name.strip_suffix('.').unwrap_or(name);

        // The text length check is sufficient to enforce both limits because:
        // - Text has (n-1) dots separating n labels
        // - Wire replaces (n-1) dots with (n-1) length prefixes, but both the
        //   dots and prefixes are the same length (1 byte each)
        // - Wire adds 1 length prefix for the first label + 1 terminating zero
        // - Wire length = Text length - (n-1) + (n-1) + 1 + 1 = Text length + 2
        // - Therefore: Text ≤ 253 guarantees Wire ≤ 255
        let stripped_len = stripped.len();
        ensure!(
            stripped_len <= MAX_DNS_NAME_LEN,
            "DNS name exceeds maximum length of 253 bytes, got {stripped_len}",
        );

        Ok(stripped)
    }

    /// Validates a DNS label (one segment of a DNS name).
    ///
    /// Returns whether the label contains any uppercase letters, and thus
    /// should be normalized to lowercase.
    ///
    /// Follows strict LDH rules: letters, digits, and hyphens only, with no
    /// leading or trailing hyphens.
    ///
    /// For BIP353, we do NOT support IDN/Punycode (café.com -> xn--caf-dma.com)
    /// because:
    ///
    /// - Wallets are recommended *against* supporting IDN domains in BIP 353:
    ///   "Because resolvers are not required to support resolving non-ASCII
    ///   identifiers, wallets SHOULD avoid using non-ASCII identifiers."
    /// - Allowing our users to make payments to IDN domains exposes them to
    ///   homograph attacks: "Your favorite charity is doing a matching donation
    ///   drive! Simply send donations to dragquéénsforgaza@strike.me"
    /// - If our users use IDN domains, they can't receive via Lightning Address
    ///   to the same username@domain. Many resolvers won't be able to pay them.
    pub(super) fn dns_label(s: &str) -> anyhow::Result<bool> {
        ensure!(!s.is_empty(), "DNS label cannot be empty");

        // Per RFC 2181 §11:
        // "The length of any one label is limited to between 1 and 63 bytes."
        // <https://www.rfc-editor.org/rfc/rfc2181#section-11>
        let label_len = s.len();
        ensure!(
            label_len <= 63,
            "DNS label must be at most 63 bytes, got {label_len}"
        );

        ensure!(
            !s.starts_with('-') && !s.ends_with('-'),
            "DNS label cannot start or end with a hyphen"
        );

        // Whether the output contains any uppercase letters.
        let mut has_uppercase = false;

        // Validate characters (byte-based for efficiency)
        for (i, &b) in s.as_bytes().iter().enumerate() {
            if !b.is_ascii_alphanumeric() && b != b'-' {
                // Find the violating character and include it in error message
                let c = s.chars().nth(i).unwrap();
                return Err(anyhow!("DNS label has invalid character '{c}'"));
            }

            has_uppercase |= b.is_ascii_uppercase();
        }

        Ok(has_uppercase)
    }
}

mod construct {
    /// Given a valid BIP353 username and domain, constructs the Fully Qualified
    /// Domain Name (FQDN) where the BIP353 TXT record can be found.
    ///
    /// Example: `satoshi@lexe.app` => `satoshi.user._bitcoin-payment.lexe.app.`
    //
    // BIP353:
    //
    // "Payment instructions are indexed by both a user and a domain.
    // Instructions for a given `user` and `domain` are stored at
    // `user`.user._bitcoin-payment.`domain` in a single TXT record."
    //
    // <https://github.com/bitcoin/bips/blob/master/bip-0353.mediawiki#records>
    pub(super) fn bip353_fqdn(username: &str, domain: &str) -> String {
        format!("{username}.user._bitcoin-payment.{domain}.")
    }

    /// Constructs the HTTPS URL where the Lightning Address LNURL-pay endpoint
    /// (LUD-06) should be queried.
    ///
    /// Example: `satoshi+tips@lexe.app`
    ///       => `https://lexe.app/.well-known/lnurlp/satoshi+tips`
    // LUD-16:
    //
    // "Upon seeing such an address, `WALLET` makes a GET request to
    // `https://<domain>/.well-known/lnurlp/<username>` endpoint if `domain` is
    // clearnet or `http://<domain>/.well-known/lnurlp/<username>` if `domain`
    // is onion."
    //
    // <https://github.com/fiatjaf/lnurl-rfc/blob/luds/16.md>
    pub(super) fn lightning_address_url(
        username: &str,
        tag: Option<&str>,
        domain: &str,
    ) -> String {
        // Note: Onion domains would use http:// instead of https://,
        // but we already reject .onion domains in parse().
        let mut url = format!("https://{domain}/.well-known/lnurlp/{username}");

        if let Some(tag) = tag {
            url.push('+');
            url.push_str(tag);
        }

        url
    }
}

#[cfg(test)]
mod test_impls {
    use std::borrow::Cow;

    use proptest::{
        arbitrary::Arbitrary,
        prop_oneof,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for EmailLikeAddress<'static> {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            prop_oneof![any_bip353_address(), any_lightning_address()].boxed()
        }
    }

    /// Any BIP353-compatible address (no tags, strict DNS labels)
    fn any_bip353_address() -> impl Strategy<Value = EmailLikeAddress<'static>>
    {
        let any_prefix = proptest::bool::ANY;
        (any_prefix, any_dns_label(), any_bip353_dns_name()).prop_map(
            |(bip353_prefix, username, domain)| {
                let bip353_fqdn = construct::bip353_fqdn(&username, &domain);
                let tag = None;
                let lightning_address_url = construct::lightning_address_url(
                    &username,
                    tag.as_deref(),
                    &domain,
                );

                EmailLikeAddress {
                    username: Cow::Owned(username),
                    domain: Cow::Owned(domain),
                    tag,
                    bip353_prefix,
                    bip353_fqdn: Some(bip353_fqdn),
                    lightning_address_url,
                }
            },
        )
    }

    /// Any Lightning Address (dots/underscores, optional tags)
    fn any_lightning_address(
    ) -> impl Strategy<Value = EmailLikeAddress<'static>> {
        let any_prefix = proptest::bool::ANY;
        let optional_tag = proptest::option::of(any_ln_username_or_tag());
        (
            any_prefix,
            any_ln_username_or_tag(),
            optional_tag,
            any_dns_name(),
        )
            .prop_map(|(bip353_prefix, username, tag, domain)| {
                // For a Lightning Address to also be valid as BIP353:
                // 1. No tag (tags aren't valid in BIP353)
                // 2. Username must be a valid DNS label (strict LDH)
                // 3. Constructed FQDN must not exceed length limit
                let maybe_bip353_fqdn = || {
                    if tag.is_some() {
                        return None;
                    }

                    validate::dns_label(&username).ok()?;

                    let fqdn = construct::bip353_fqdn(&username, &domain);
                    validate::dns_name_length(&fqdn).ok()?;

                    Some(fqdn)
                };
                let bip353_fqdn = maybe_bip353_fqdn();

                let lightning_address_url = construct::lightning_address_url(
                    &username,
                    tag.as_deref(),
                    &domain,
                );

                EmailLikeAddress {
                    username: Cow::Owned(username),
                    domain: Cow::Owned(domain),
                    tag: tag.map(Cow::Owned),
                    bip353_prefix,
                    bip353_fqdn,
                    lightning_address_url,
                }
            })
    }

    /// DNS domain names (1-4 labels separated by dots).
    /// Guaranteed to stay under MAX_DNS_NAME_LEN chars total.
    fn any_dns_name() -> impl Strategy<Value = String> {
        proptest::collection::vec(any_dns_label(), 1..=4).prop_map(|labels| {
            let mut labels = labels.into_iter();
            let mut dns_name = String::with_capacity(MAX_DNS_NAME_LEN);
            dns_name.push_str(&labels.next().unwrap());

            for label in labels {
                let potential_len = dns_name.len() + 1 + label.len();
                if potential_len > MAX_DNS_NAME_LEN {
                    return dns_name;
                }
                dns_name.push('.');
                dns_name.push_str(&label);
            }

            dns_name
        })
    }

    /// DNS domain names for BIP353 addresses (1-4 labels).
    /// Guaranteed to stay under MAX_DNS_NAME_LEN chars total.
    /// Reserves space for BIP353 overhead: max username (63) + FQDN parts (25).
    fn any_bip353_dns_name() -> impl Strategy<Value = String> {
        proptest::collection::vec(any_dns_label(), 1..=4).prop_map(|labels| {
            // Reserve space for BIP353 overhead:
            // - max username: 63 chars
            // - BIP353 FQDN parts: ".user._bitcoin-payment." (25 chars)
            // - total reserved: 88 chars
            const MAX_DOMAIN_LEN: usize = MAX_DNS_NAME_LEN - 88;

            let mut labels = labels.into_iter();
            let mut dns_name = String::with_capacity(MAX_DOMAIN_LEN);
            dns_name.push_str(&labels.next().unwrap());

            for label in labels {
                let potential_len = dns_name.len() + 1 + label.len();
                if potential_len > MAX_DOMAIN_LEN {
                    return dns_name;
                }
                dns_name.push('.');
                dns_name.push_str(&label);
            }

            dns_name
        })
    }

    /// Valid DNS labels (strict LDH rules for BIP353)
    /// Since parsing normalizes to lowercase we only generate lowercase values.
    fn any_dns_label() -> impl Strategy<Value = String> {
        "[a-z0-9]([-a-z0-9]{0,61}[a-z0-9])?"
    }

    /// Lightning Address usernames or tags (allows dots and underscores).
    /// Since parsing normalizes to lowercase we only generate lowercase values.
    fn any_ln_username_or_tag() -> impl Strategy<Value = String> {
        "[a-z0-9]([-._a-z0-9]{0,30}[a-z0-9])?"
    }
}

#[cfg(test)]
mod test {
    use proptest::prelude::{prop_assert_eq, proptest};

    use super::*;

    /// Asserts that a parse error contains a specific message.
    fn assert_error_msg(
        result: Result<EmailLikeAddress, Error>,
        expected_msg: &str,
    ) {
        match result {
            Err(Error::InvalidEmailLike(msg)) => {
                assert!(
                    msg.contains(expected_msg),
                    "Expected error to contain '{expected_msg}', but got: {msg}"
                );
            }
            _ => panic!(
                "Expected Error::InvalidEmailLike containing '{expected_msg}'"
            ),
        }
    }

    #[test]
    fn test_parse_valid() {
        // (input, has_bip353_prefix, is_valid_bip353, expected_tag)
        let valid_cases = [
            // Valid as both BIP353 and Lightning Address
            ("user@example.com", false, true, None),
            ("₿user@example.com", true, true, None),
            ("%E2%82%BFuser@example.com", true, true, None),
            ("₿alice@bitcoin.org", true, true, None),
            ("test-user@example.com", false, true, None),
            ("123@example.com", false, true, None),
            ("a@b.c", false, true, None),
            ("User@example.com", false, true, None),
            ("USER@EXAMPLE.COM", false, true, None),
            // Valid only as Lightning Address (not BIP353)
            ("test.user@example.com", false, false, None),
            ("test_user@example.com", false, false, None),
            ("user+tag@example.com", false, false, Some("tag")),
            ("₿test.user@example.com", true, false, None),
            ("%E2%82%BFtest.user@example.com", true, false, None),
            ("₿-user@example.com", true, false, None),
            ("₿user_name@example.com", true, false, None),
            ("%E2%82%BFuser_name@example.com", true, false, None),
            ("₿user.name@example.com", true, false, None),
            // With tags (Lightning Address only)
            ("alice+tips@example.com", false, false, Some("tips")),
            ("₿alice+tips@example.com", true, false, Some("tips")),
            ("user+tag-123@example.com", false, false, Some("tag-123")),
            ("user+my.tag_1@example.com", false, false, Some("my.tag_1")),
        ];

        for (input, expected_prefix, expected_bip353, expected_tag) in
            valid_cases
        {
            let addr = EmailLikeAddress::parse(input).unwrap();
            assert_eq!(
                addr.bip353_prefix, expected_prefix,
                "Wrong prefix flag for {input}"
            );
            assert_eq!(
                addr.bip353_fqdn.is_some(),
                expected_bip353,
                "Wrong BIP353 validity for {input}"
            );
            assert_eq!(
                addr.tag.as_deref(),
                expected_tag,
                "Wrong tag for {input}"
            );
        }
    }

    #[test]
    fn test_tag_extraction() {
        // Without tag
        let addr = EmailLikeAddress::parse("user@example.com").unwrap();
        assert_eq!(addr.username.as_ref(), "user");
        assert_eq!(addr.domain.as_ref(), "example.com");
        assert_eq!(addr.tag, None);

        // With tag
        let addr = EmailLikeAddress::parse("user+mytag@example.com").unwrap();
        assert_eq!(addr.username.as_ref(), "user");
        assert_eq!(addr.domain.as_ref(), "example.com");
        assert_eq!(addr.tag.as_deref(), Some("mytag"));
    }

    #[test]
    fn test_parse_invalid() {
        let invalid_cases = [
            // Missing components
            "",
            "@example.com",
            "user@",
            "userexample.com",
            "user@@example.com",
            // Invalid domain
            "user@_example.com",
            "user@-example.com",
            // Invalid username characters
            "user space@example.com",
            // Invalid Bitcoin prefix usage
            "₿",
            "₿@example.com",
            "%E2%82%BF@example.com",
            "₿user@",
            "%E2%82%BFuser@",
            "₿user",
            "%E2%82%BFuser",
            // Invalid tag usage
            "user+tag space@example.com",
            "user+tag+more@example.com",
            "user+@example.com",
            "+tag@example.com",
        ];

        for input in invalid_cases {
            assert!(
                EmailLikeAddress::parse(input).is_err(),
                "Should fail to parse: {input}"
            );
        }

        // Specific error message tests
        assert_error_msg(
            EmailLikeAddress::parse(
                "user@byfbslyavenz6uz4i4pdwu4t5ixrnmtwmxysip2h54bnzjv3cfwc6qyd.onion"
            ),
            ".onion domains are not supported",
        );

        assert_error_msg(
            EmailLikeAddress::parse("user name@example.com"),
            "Found invalid character",
        );
    }

    #[test]
    fn test_validate_dns_name() {
        // Valid hostnames
        assert!(validate::dns_name("example.com").is_ok());
        assert!(validate::dns_name("sub.example.com").is_ok());
        assert!(validate::dns_name("a.b.c.d.e.f").is_ok());
        assert!(validate::dns_name("123.456.789.0").is_ok());
        assert!(validate::dns_name("test-123.example-456.com").is_ok());
        assert!(validate::dns_name("example.com.").is_ok());
        assert!(validate::dns_name("localhost").is_ok());
        assert!(validate::dns_name("example").is_ok());

        // Onion domains
        let onion_v3 =
            "byfbslyavenz6uz4i4pdwu4t5ixrnmtwmxysip2h54bnzjv3cfwc6qyd.onion";
        assert!(validate::dns_name(onion_v3).is_ok());

        // Invalid hostnames
        assert!(validate::dns_name("").is_err());
        assert!(validate::dns_name(".example.com").is_err());
        assert!(validate::dns_name("example..com").is_err());
        assert!(validate::dns_name("example.com..").is_err());
        assert!(validate::dns_name("-example.com").is_err());
        assert!(validate::dns_name("example.com-").is_err());
        assert!(validate::dns_name("_service.example.com").is_err());
        assert!(validate::dns_name("example@com").is_err());

        // Length violations
        let long_label = "x".repeat(64);
        assert!(validate::dns_name(&format!("{long_label}.com")).is_err());

        let x250 = "x".repeat(250);
        let long_name = format!("{x250}.com");
        assert!(validate::dns_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_dns_label() {
        // Valid labels
        assert!(validate::dns_label("example").is_ok());
        assert!(validate::dns_label("test123").is_ok());
        assert!(validate::dns_label("123test").is_ok());
        assert!(validate::dns_label("a").is_ok());
        assert!(validate::dns_label("a-b").is_ok());
        assert!(validate::dns_label("test-123-abc").is_ok());
        assert!(validate::dns_label("x".repeat(63).as_str()).is_ok());

        // Invalid labels
        assert!(validate::dns_label("").is_err());
        assert!(validate::dns_label("-test").is_err());
        assert!(validate::dns_label("test-").is_err());
        assert!(validate::dns_label("test_123").is_err());
        assert!(validate::dns_label("test.com").is_err());
        assert!(validate::dns_label("test@example").is_err());
        assert!(validate::dns_label("x".repeat(64).as_str()).is_err());
        assert!(validate::dns_label("_service").is_err());
        assert!(validate::dns_label("_bitcoin-payment").is_err());

        // Invalid: Non-ASCII characters
        let err = validate::dns_label("café").unwrap_err();
        assert!(err.to_string().contains("é"));
        let err = validate::dns_label("test日本").unwrap_err();
        assert!(err.to_string().contains("日"));
    }

    #[test]
    fn test_dns_wire_format_limits() {
        // Test wire format length edge cases.
        // Wire format: each label gets a 1-byte length prefix, plus a
        // terminating zero byte for the entire name.

        // Maximum valid: 253 chars in text form = 255 bytes in wire format
        // Use 4 labels: 63, 63, 63, 61
        // Text: 63 + 1 + 63 + 1 + 63 + 1 + 61 = 253 chars
        // Wire: [63]<63>[63]<63>[63]<63>[61]<61>[0] = (1+63)*3 + (1+61) + 1 =
        // 255 bytes
        let label_63 = "x".repeat(63);
        let label_61 = "x".repeat(61);
        let name_253 = format!("{label_63}.{label_63}.{label_63}.{label_61}");
        assert_eq!(name_253.len(), 253);
        assert!(validate::dns_name(&name_253).is_ok());

        // Another valid case: 4 labels of varying sizes
        // Text: 63 + 1 + 63 + 1 + 63 + 1 + 58 = 250 chars
        // Wire: [63]<63>[63]<63>[63]<63>[58]<58>[0] = (1+63)*3 + (1+58) + 1 =
        // 252 bytes
        let label_58 = "x".repeat(58);
        let name_250 = format!("{label_63}.{label_63}.{label_63}.{label_58}");
        assert_eq!(name_250.len(), 250);
        assert!(validate::dns_name(&name_250).is_ok());

        // Exceeds text form limit (254 > 253)
        let name_254 = "x".repeat(254);
        assert!(validate::dns_name(&name_254).is_err());
    }

    #[test]
    fn prop_roundtrip() {
        proptest!(|(addr1: EmailLikeAddress)| {
            let string = addr1.to_string();
            let addr2 = EmailLikeAddress::parse(&string);
            prop_assert_eq!(Ok(&addr1), addr2.as_ref());
        });
    }

    #[test]
    fn test_url_construction_and_normalization() {
        // Test cases with various casing: already normalized, uppercase, mixed
        // case Each test case: (input, expected_bip353_fqdn,
        // expected_ln_url)
        let test_cases = [
            // Already normalized (lowercase)
            (
                "alice@example.com",
                Some("alice.user._bitcoin-payment.example.com."),
                "https://example.com/.well-known/lnurlp/alice",
            ),
            // Fully uppercase
            (
                "ALICE@EXAMPLE.COM",
                Some("alice.user._bitcoin-payment.example.com."),
                "https://example.com/.well-known/lnurlp/alice",
            ),
            // Mixed case
            (
                "Alice@Example.Com",
                Some("alice.user._bitcoin-payment.example.com."),
                "https://example.com/.well-known/lnurlp/alice",
            ),
            // With ₿ prefix, mixed case
            (
                "₿BoB@SuB.ExAmPlE.cOm",
                Some("bob.user._bitcoin-payment.sub.example.com."),
                "https://sub.example.com/.well-known/lnurlp/bob",
            ),
            // Numbers and hyphens (valid for BIP353)
            (
                "TeSt-123@Bitcoin.Org",
                Some("test-123.user._bitcoin-payment.bitcoin.org."),
                "https://bitcoin.org/.well-known/lnurlp/test-123",
            ),
            // Short domain
            (
                "₿A@B.C",
                Some("a.user._bitcoin-payment.b.c."),
                "https://b.c/.well-known/lnurlp/a",
            ),
            // Lightning-only: dots (not valid for BIP353)
            (
                "Test.User@Example.COM",
                None,
                "https://example.com/.well-known/lnurlp/test.user",
            ),
            // Lightning-only: underscores
            (
                "TEST_USER@DOMAIN.COM",
                None,
                "https://domain.com/.well-known/lnurlp/test_user",
            ),
            // Lightning-only: plus sign for tags (tag preserved separately)
            (
                "User+Tag@Domain.Com",
                None,
                "https://domain.com/.well-known/lnurlp/user+tag",
            ),
            // Lightning-only: complex username with tag and mixed case
            (
                "User.Name_123+Tag@Sub.Domain.COM",
                None,
                "https://sub.domain.com/.well-known/lnurlp/user.name_123+tag",
            ),
            // Lightning-only: tag with various valid characters
            (
                "alice+my-tag.test_123@example.com",
                None,
                "https://example.com/.well-known/lnurlp/alice+my-tag.test_123",
            ),
        ];

        for (input, expected_fqdn, expected_url) in test_cases {
            let addr = EmailLikeAddress::parse(input).unwrap();

            // Test BIP353 FQDN
            assert_eq!(
                addr.bip353_fqdn,
                expected_fqdn.map(|s| s.to_string()),
                "Failed BIP353 FQDN for input: {input}"
            );

            // Test Lightning Address URL
            assert_eq!(
                addr.lightning_address_url, expected_url,
                "Failed Lightning URL for input: {input}"
            );
        }
    }
}
