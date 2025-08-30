/// Indeterminate email-like human-readable payment address.
/// Either BIP353 without the BTC currency prefix or Lightning Address.
pub(crate) struct EmailLikeAddress<'a> {
    _local: &'a str,
    _domain: &'a str,
}

impl EmailLikeAddress<'_> {
    pub(crate) fn matches(s: &str) -> Option<(&str, &str)> {
        s.split_once('@')
    }

    // TODO(phlip9): support BIP353 / Lightning Address
    // fn parse(s: &'a str) -> Option<Self> {
    //     let (local, domain) = Self::matches(s)?;
    //     Self::parse_inner(local, domain)
    // }
    //
    // fn parse_inner(local: &'a str, domain: &'a str) -> Option<Self> {
    //     if local.is_empty() || domain.is_empty() {
    //         return None;
    //     }
    //
    //     // local:
    //     // - Lightning Address: a-z0-9-_.
    //     // - BIP353: DNS name segment
    //     //
    //     // Union:
    //     if !local.as_bytes().iter().all(|&b| {
    //         b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'_'
    //     }) {
    //         return None;
    //     }
    //
    //     // domain:
    //     // - Lightning Address: full DNS hostname
    //     // - BIP353: partial DNS name
    //     //
    //     // Union:
    //     if !is_dns_name(domain.as_bytes()) {
    //         return None;
    //     }
    //
    //     Some(Self {
    //         _local: local,
    //         _domain: domain,
    //     })
    // }
}

// TODO(phlip9): support BIP353
// TODO(phlip9): punycode decode?
pub(crate) struct Bip353Address<'a> {
    _user: &'a str,
    _domain: &'a str,
}

impl Bip353Address<'_> {
    pub(crate) fn matches(s: &str) -> Option<&str> {
        s.strip_prefix("₿")
    }

    // TODO(phlip9): support BIP353
    // fn parse(s: &'a str) -> Option<Self> {
    //     Self::parse_inner(Self::matches(s)?)
    // }
    //
    // fn parse_inner(hrn: &'a str) -> Option<Self> {
    //     let (user, domain) = hrn.split_once('@')?;
    //
    //     if user.is_empty() || domain.is_empty() {
    //         return None;
    //     }
    //
    //     // user:
    //     // - DNS name segment
    //     if !is_dns_name_segment(user.as_bytes()) {
    //         return None;
    //     }
    //
    //     // domain:
    //     // - partial DNS name
    //     if !is_dns_name(domain.as_bytes()) {
    //         return None;
    //     }
    //
    //     let dns = format!("{user}.user._bitcoin-payment.{domain}.");
    //     if !is_dns_name(&dns.as_bytes()) {
    //         return None;
    //     }
    //
    //     Some(Self {
    //         _user: user,
    //         _domain: domain,
    //     })
    // }
}

// TODO(phlip9): support BIP353
// fn is_dns_name(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 255
//         && s.split(|&b| b == b'.').all(is_dns_name_segment)
//         && !s.ends_with(&[b'.'])
// }

// fn is_dns_name_segment(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 63
//         && s.iter()
//             .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b ==
// b'_') }

// TODO(phlip9): support BIP353 / Lightning Address
// /// Returns `true` if `s` is a valid DNS hostname.
// ///
// /// - a-z0-9-. (case insensitive)
// /// - no leading/trailing hyphens
// /// - no empty labels
// /// - no empty domain
// /// - no more than 253 characters
// /// - no more than 63 characters per label
// /// - no trailing dot
// fn is_hostname(s: &[u8]) -> bool {
//     !s.is_empty()
//         && s.len() <= 253
//         && s.split(|&b| b == b'.').all(|label| {
//             !label.is_empty()
//                 && label.len() <= 63
//                 && !label.starts_with(&[b'-'])
//                 && !label.ends_with(&[b'-'])
//                 && s.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-')
//         })
//         && !s.ends_with(&[b'.'])
// }
