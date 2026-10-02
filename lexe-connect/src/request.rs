//! The credential request and its connection string.

use std::{
    collections::BTreeSet,
    fmt::{self, Display},
    str::FromStr,
};

use lexe_byte_array::ByteArray;
use lexe_common::time::TimestampMs;
#[cfg(feature = "crypto")]
use lexe_crypto::hpke;
use lexe_crypto::rng::{Crng, RngExt};
use lexe_uri::Uri;

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RequestError {
    #[error("Not a {} connection string", CredentialRequest::LEXE_BASE_URL)]
    NotConnectUrl,
    #[error("Unsupported LexeConnect version: {0}")]
    UnsupportedVersion(String),
    #[error("Missing param: {0}")]
    Missing(&'static str),
    #[error("Invalid param {0}: {1}")]
    Invalid(&'static str, String),
    #[error("Unsupported param: {0}")]
    Unsupported(String),
}

/// A credential request, as carried by a connection string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialRequest {
    /// Fresh per request; the response is HPKE-encrypted under it.
    /// Required unless `delivery` is [`Delivery::Post`].
    pub ephemeral_hpke_pubkey: Option<HpkePubkey>,
    /// Fresh per request; echoed in the response so it can be matched.
    pub one_time_secret: OneTimeSecret,
    pub params: CredentialRequestParams,
}

/// The parts of a [`CredentialRequest`] that the REQUESTER chooses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialRequestParams {
    pub delivery: Delivery,
    /// The REQUESTER account being connected, in a form the user recognizes,
    /// e.g. `@janedoe`. Shown on the approval screen and bound into the
    /// response encryption, so a forwarded request can't be re-targeted.
    pub account: Option<String>,
    /// Echoed back verbatim in the response.
    pub metadata: Option<String>,
    /// The REQUESTER's own name, shown only for a verified REQUESTER; see
    /// [`CredentialRequest::requester_branding`].
    pub requester_name: Option<String>,
    /// The REQUESTER's icon url, likewise.
    pub requester_icon: Option<String>,
    /// Granted exactly as requested, or the request is rejected. Scope and
    /// permission names are opaque here; the WALLET validates them when it
    /// creates the credential.
    pub scopes: BTreeSet<String>,
    /// Granted exactly as requested, or the request is rejected.
    pub permissions: BTreeSet<String>,
    /// Prefills the credential label; the user may change it.
    pub label: Option<String>,
    /// Prefills the credential expiration; the user may change it.
    pub expires_at: Option<TimestampMs>,
}

impl CredentialRequestParams {
    // Spec: Protocol params, `account` and `metadata`.
    const MAX_ACCOUNT_LEN: usize = 64;
    const MAX_METADATA_LEN: usize = 1024;
    /// Matches the Lexe client label limit.
    const MAX_LABEL_LEN: usize = 64;
}

/// Where the WALLET delivers the response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Delivery {
    /// Redirect the user to this uri, appending the encrypted response as a
    /// `response` query param. Should be an `https://` app link.
    Redirect(String),
    /// POST the response to this `https://` url.
    Post(String),
    /// POST the encrypted response to this `https://` mailbox, from which
    /// the REQUESTER polls it.
    Mailbox(String),
}

/// The REQUESTER's ephemeral X25519 HPKE public key, rendered as 64 lowercase
/// hex characters.
//
// Spec: Protocol params.
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct HpkePubkey([u8; 32]);

lexe_byte_array::impl_byte_array!(HpkePubkey, 32);
lexe_byte_array::impl_fromstr_fromhex!(HpkePubkey, 32);
lexe_byte_array::impl_debug_display_as_hex!(HpkePubkey);

#[cfg(feature = "crypto")]
impl HpkePubkey {
    pub(crate) fn from_hpke(pk: &hpke::PublicKey) -> Self {
        Self(pk.to_array())
    }

    pub(crate) fn to_hpke(self) -> hpke::PublicKey {
        hpke::PublicKey::from_array(self.0)
    }
}

/// A random per-request secret, rendered as 32 lowercase hex characters.
//
// Spec: Protocol params.
#[derive(Copy, Clone, Eq, Hash, PartialEq)]
pub struct OneTimeSecret([u8; 16]);

lexe_byte_array::impl_byte_array!(OneTimeSecret, 16);
lexe_byte_array::impl_fromstr_fromhex!(OneTimeSecret, 16);
lexe_byte_array::impl_debug_display_as_hex!(OneTimeSecret);
lexe_serde::impl_serde_hexstr_or_bytes!(OneTimeSecret);

impl OneTimeSecret {
    pub fn from_rng(rng: &mut impl Crng) -> Self {
        Self(rng.gen_bytes())
    }
}

impl CredentialRequest {
    /// The protocol version this crate implements.
    //
    // Spec: Protocol params, `v`.
    const VERSION: u32 = 1;
    /// Lexe's connect url; every connection string starts with it.
    //
    // Spec: WALLET-defined parts.
    pub const LEXE_BASE_URL: &str = "https://lexe.app/connect";

    /// Parse a connection string. Unrecognized params are dropped.
    pub fn parse(s: &str) -> Result<Self, RequestError> {
        let is_connect_url = s
            .strip_prefix(Self::LEXE_BASE_URL)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('?'));
        if !is_connect_url {
            return Err(RequestError::NotConnectUrl);
        }

        let uri = Uri::parse(s)
            .map_err(|e| RequestError::Invalid("uri", e.to_string()))?;
        Self::parse_uri(uri)
    }

    /// Parse the params of a connection string.
    //
    // Spec: Protocol params and Credential params.
    fn parse_uri(uri: Uri<'_>) -> Result<Self, RequestError> {
        fn parse_param<T: FromStr>(
            name: &'static str,
            s: &str,
        ) -> Result<T, RequestError>
        where
            T::Err: Display,
        {
            s.parse()
                .map_err(|e: T::Err| RequestError::Invalid(name, e.to_string()))
        }

        fn parse_list(s: &str) -> BTreeSet<String> {
            s.split(',').map(str::to_owned).collect()
        }

        let mut v = None;
        let mut redirect_uri = None;
        let mut post_url = None;
        let mut mailbox_url = None;
        let mut ephemeral_hpke_pubkey = None;
        let mut one_time_secret = None;
        let mut account = None;
        let mut metadata = None;
        let mut requester_name = None;
        let mut requester_icon = None;
        let mut scopes = BTreeSet::new();
        let mut permissions = BTreeSet::new();
        let mut label = None;
        let mut expires_at = None;

        // Later duplicates override earlier ones; unrecognized keys are
        // dropped.
        for param in uri.params {
            let value = param.value;
            match param.key.as_ref() {
                "v" => v = Some(value.into_owned()),
                "redirect_uri" => redirect_uri = Some(value.into_owned()),
                "post_url" => post_url = Some(value.into_owned()),
                "mailbox_url" => mailbox_url = Some(value.into_owned()),
                "ephemeral_hpke_pubkey" =>
                    ephemeral_hpke_pubkey =
                        Some(parse_param("ephemeral_hpke_pubkey", &value)?),
                "one_time_secret" =>
                    one_time_secret =
                        Some(parse_param("one_time_secret", &value)?),
                "account" => account = Some(value.into_owned()),
                "metadata" => metadata = Some(value.into_owned()),
                "requester_name" => requester_name = Some(value.into_owned()),
                "requester_icon" => requester_icon = Some(value.into_owned()),
                "scopes" => scopes = parse_list(&value),
                "permissions" => permissions = parse_list(&value),
                "label" => label = Some(value.into_owned()),
                "expires_at" =>
                    expires_at = Some(parse_param("expires_at", &value)?),
                // Budgets are planned. Some budget params are exact, so
                // dropping them would grant a credential the REQUESTER
                // didn't ask for.
                key if key.starts_with("budget_") =>
                    return Err(RequestError::Unsupported(key.to_owned())),
                _ => (),
            }
        }

        let version = v.ok_or(RequestError::Missing("v"))?;
        if version != Self::VERSION.to_string() {
            return Err(RequestError::UnsupportedVersion(version));
        }

        let delivery = match (redirect_uri, post_url, mailbox_url) {
            (Some(uri), None, None) => Delivery::Redirect(uri),
            (None, Some(url), None) => Delivery::Post(url),
            (None, None, Some(url)) => Delivery::Mailbox(url),
            (None, None, None) =>
                return Err(RequestError::Missing(
                    "redirect_uri, post_url, or mailbox_url",
                )),
            _ =>
                return Err(RequestError::Invalid(
                    "delivery",
                    "Set exactly one of redirect_uri, post_url, mailbox_url"
                        .into(),
                )),
        };

        let one_time_secret =
            one_time_secret.ok_or(RequestError::Missing("one_time_secret"))?;

        let request = Self {
            ephemeral_hpke_pubkey,
            one_time_secret,
            params: CredentialRequestParams {
                delivery,
                account,
                metadata,
                requester_name,
                requester_icon,
                scopes,
                permissions,
                label,
                expires_at,
            },
        };
        request.validate()?;

        Ok(request)
    }

    /// Render as a uri: [`Self::LEXE_BASE_URL`] plus the set params, in spec
    /// order. The inverse of [`Self::parse_uri`].
    fn to_uri(&self) -> Uri<'_> {
        let p = &self.params;
        let (delivery_key, delivery_target) = match &p.delivery {
            Delivery::Redirect(uri) => ("redirect_uri", uri.as_str()),
            Delivery::Post(url) => ("post_url", url.as_str()),
            Delivery::Mailbox(url) => ("mailbox_url", url.as_str()),
        };
        let join = |set: &BTreeSet<String>| {
            (!set.is_empty()).then(|| {
                set.iter().map(String::as_str).collect::<Vec<_>>().join(",")
            })
        };
        let scopes = join(&p.scopes);
        let permissions = join(&p.permissions);

        let mut uri: Uri<'_> = Uri::parse(Self::LEXE_BASE_URL)
            .expect("LEXE_BASE_URL is a valid uri");

        uri.push_param("v", Self::VERSION.to_string());
        uri.push_param(delivery_key, delivery_target);
        if let Some(pk) = self.ephemeral_hpke_pubkey {
            uri.push_param("ephemeral_hpke_pubkey", pk.to_string());
        }
        uri.push_param("one_time_secret", self.one_time_secret.to_string());
        if let Some(account) = &p.account {
            uri.push_param("account", account.as_str());
        }
        if let Some(metadata) = &p.metadata {
            uri.push_param("metadata", metadata.as_str());
        }
        if let Some(name) = &p.requester_name {
            uri.push_param("requester_name", name.as_str());
        }
        if let Some(icon) = &p.requester_icon {
            uri.push_param("requester_icon", icon.as_str());
        }
        if let Some(scopes) = scopes {
            uri.push_param("scopes", scopes);
        }
        if let Some(permissions) = permissions {
            uri.push_param("permissions", permissions);
        }
        if let Some(label) = &p.label {
            uri.push_param("label", label.as_str());
        }
        if let Some(expires_at) = p.expires_at {
            uri.push_param("expires_at", expires_at.to_u64().to_string());
        }

        uri
    }

    /// Check the constraints on a request.
    //
    // Spec: Protocol params and Credential params.
    pub(crate) fn validate(&self) -> Result<(), RequestError> {
        fn https_host(url: &str) -> Option<String> {
            let uri = Uri::parse(url).ok()?;
            let host = uri.is_https().then(|| uri.host()).flatten()?;
            Some(host.into_owned())
        }

        fn check_len(
            name: &'static str,
            value: Option<&str>,
            max_len: usize,
        ) -> Result<(), RequestError> {
            match value {
                Some(v) if v.len() > max_len => Err(RequestError::Invalid(
                    name,
                    format!("Exceeds {max_len} bytes"),
                )),
                _ => Ok(()),
            }
        }

        let p = &self.params;
        match &p.delivery {
            Delivery::Redirect(uri) => {
                if Uri::parse(uri).is_err() {
                    return Err(RequestError::Invalid(
                        "redirect_uri",
                        "Not a uri".into(),
                    ));
                }
                if uri.contains('#') {
                    return Err(RequestError::Invalid(
                        "redirect_uri",
                        "Must not contain a fragment".into(),
                    ));
                }
            }
            Delivery::Post(url) if https_host(url).is_none() =>
                return Err(RequestError::Invalid(
                    "post_url",
                    "Must be an https:// url".into(),
                )),
            Delivery::Mailbox(url) if https_host(url).is_none() =>
                return Err(RequestError::Invalid(
                    "mailbox_url",
                    "Must be an https:// url".into(),
                )),
            Delivery::Post(_) | Delivery::Mailbox(_) => (),
        }

        if self.ephemeral_hpke_pubkey.is_none()
            && !matches!(p.delivery, Delivery::Post(_))
        {
            return Err(RequestError::Missing("ephemeral_hpke_pubkey"));
        }

        check_len(
            "account",
            p.account.as_deref(),
            CredentialRequestParams::MAX_ACCOUNT_LEN,
        )?;
        check_len(
            "metadata",
            p.metadata.as_deref(),
            CredentialRequestParams::MAX_METADATA_LEN,
        )?;
        check_len(
            "label",
            p.label.as_deref(),
            CredentialRequestParams::MAX_LABEL_LEN,
        )?;

        if p.scopes.is_empty() && p.permissions.is_empty() {
            return Err(RequestError::Missing("scopes or permissions"));
        }

        Ok(())
    }
}

/// Renders the connection string; the inverse of [`CredentialRequest::parse`].
impl fmt::Display for CredentialRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_uri().fmt(f)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use lexe_common::test_utils::arbitrary::any_bounded_string;
    use proptest::{
        arbitrary::{Arbitrary, any},
        option, prop_oneof,
        strategy::{BoxedStrategy, Strategy},
        string::string_regex,
    };

    use super::*;

    impl Arbitrary for CredentialRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_https_url = || {
                string_regex("https://[a-z0-9]{1,12}\\.[a-z]{2,4}/[a-z]{0,8}")
                    .unwrap()
            };
            let any_delivery = prop_oneof![
                any_https_url().prop_map(Delivery::Redirect),
                any_https_url().prop_map(Delivery::Post),
                any_https_url().prop_map(Delivery::Mailbox),
            ];
            let any_names = || {
                proptest::collection::btree_set(
                    string_regex("[a-z_]{1,16}").unwrap(),
                    0..4,
                )
            };
            let any_grant = (any_names(), any_names()).prop_filter(
                "Must request something",
                |(scopes, permissions)| {
                    !scopes.is_empty() || !permissions.is_empty()
                },
            );
            (
                any::<Option<HpkePubkey>>(),
                any::<OneTimeSecret>(),
                any_delivery,
                option::of(any_bounded_string(
                    CredentialRequestParams::MAX_ACCOUNT_LEN,
                )),
                option::of(any_bounded_string(
                    CredentialRequestParams::MAX_METADATA_LEN,
                )),
                (
                    option::of(any_bounded_string(64)),
                    option::of(any_https_url()),
                ),
                any_grant,
                option::of(any_bounded_string(
                    CredentialRequestParams::MAX_LABEL_LEN,
                )),
                any::<Option<TimestampMs>>(),
            )
                .prop_map(
                    |(
                        mut ephemeral_hpke_pubkey,
                        one_time_secret,
                        delivery,
                        account,
                        metadata,
                        (requester_name, requester_icon),
                        (scopes, permissions),
                        label,
                        expires_at,
                    )| {
                        // Only plaintext `Post` may omit the pubkey.
                        if !matches!(delivery, Delivery::Post(_)) {
                            ephemeral_hpke_pubkey.get_or_insert_with(|| {
                                HpkePubkey::from_array([7; 32])
                            });
                        }
                        CredentialRequest {
                            ephemeral_hpke_pubkey,
                            one_time_secret,
                            params: CredentialRequestParams {
                                delivery,
                                account,
                                metadata,
                                requester_name,
                                requester_icon,
                                scopes,
                                permissions,
                                label,
                                expires_at,
                            },
                        }
                    },
                )
                .boxed()
        }
    }

    impl Arbitrary for HpkePubkey {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 32]>().prop_map(Self).boxed()
        }
    }

    impl Arbitrary for OneTimeSecret {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<[u8; 16]>().prop_map(Self).boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use lexe_crypto::rng::SysRng;
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn connection_string_roundtrip_prop() {
        proptest!(|(request in any::<CredentialRequest>())| {
            let parsed = CredentialRequest::parse(&request.to_string());
            prop_assert_eq!(parsed, Ok(request));
        });
    }

    fn names<const N: usize>(names: [&str; N]) -> BTreeSet<String> {
        names.map(String::from).into()
    }

    pub(crate) fn sample_params(delivery: Delivery) -> CredentialRequestParams {
        CredentialRequestParams {
            delivery,
            account: Some("@janedoe".into()),
            metadata: Some("{\"k\":\"v&=+\"}".into()),
            requester_name: None,
            requester_icon: None,
            scopes: names(["read_info", "receive"]),
            permissions: names(["cancel_payment"]),
            label: Some("BillSplit App".into()),
            expires_at: Some(TimestampMs::from_millis(1821484800000).unwrap()),
        }
    }

    fn sample_request(delivery: Delivery) -> CredentialRequest {
        let mut rng = SysRng::new();
        CredentialRequest {
            ephemeral_hpke_pubkey: Some(HpkePubkey::from_array(
                rng.gen_bytes(),
            )),
            one_time_secret: OneTimeSecret::from_rng(&mut rng),
            params: sample_params(delivery),
        }
    }

    #[test]
    fn connection_string_round_trip() {
        for delivery in [
            Delivery::Redirect("https://billsplit.com/cb?e=1".into()),
            Delivery::Redirect("myprotocol://cb".into()),
            Delivery::Post("https://paygate.com/lexe".into()),
            Delivery::Mailbox(crate::LEXE_MAILBOX_URL.into()),
        ] {
            let request = sample_request(delivery);
            let s = request.to_string();
            assert!(s.starts_with("https://lexe.app/connect?v=1&"), "{s}");
            assert_eq!(CredentialRequest::parse(&s).unwrap(), request);
        }

        // Plaintext post: no pubkey, minimal params.
        let request = CredentialRequest {
            ephemeral_hpke_pubkey: None,
            params: CredentialRequestParams {
                account: None,
                metadata: None,
                label: None,
                expires_at: None,
                ..sample_params(Delivery::Post("https://paygate.com".into()))
            },
            ..sample_request(Delivery::Post("https://paygate.com".into()))
        };
        let s = request.to_string();
        assert_eq!(CredentialRequest::parse(&s).unwrap(), request);
    }

    #[test]
    fn parse_spec_example_and_unknown_params() {
        let s = "https://lexe.app/connect?v=1\
            &redirect_uri=https%3A%2F%2Fbillsplit.com%2Fcb\
            &ephemeral_hpke_pubkey=4310ee97d88cc1f088a5576c77ab0cf5c3ac797f3d95139c6c84b5429c59662a\
            &one_time_secret=000102030405060708090a0b0c0d0e0f\
            &scopes=read_info,read_payments,receive\
            &future_param=x";
        let request = CredentialRequest::parse(s).unwrap();
        assert_eq!(
            request.params.scopes,
            names(["read_info", "read_payments", "receive"])
        );
        assert_eq!(request.params.account, None);
    }

    #[test]
    fn parse_errors() {
        let base =
            sample_request(Delivery::Redirect("https://a.com/cb".into()));
        let ok = base.to_string();
        let parse_err = |s: &str| CredentialRequest::parse(s).unwrap_err();

        assert_eq!(
            parse_err("https://lexe.app/other?v=1"),
            RequestError::NotConnectUrl
        );
        assert_eq!(
            parse_err(&ok.replace("v=1&", "v=2&")),
            RequestError::UnsupportedVersion("2".into())
        );
        assert_eq!(
            parse_err(&ok.replace("v=1&", "")),
            RequestError::Missing("v")
        );
        assert_eq!(
            parse_err(&format!("{ok}&post_url=https%3A%2F%2Fb.com")),
            RequestError::Invalid(
                "delivery",
                "Set exactly one of redirect_uri, post_url, mailbox_url".into()
            )
        );
        assert_eq!(
            parse_err(&ok.replace(
                "&scopes=read_info%2Creceive&permissions=cancel_payment",
                ""
            )),
            RequestError::Missing("scopes or permissions")
        );
        assert!(matches!(
            parse_err(&ok.replace("&one_time_secret=", "&one_time_secret=ab")),
            RequestError::Invalid("one_time_secret", _)
        ));
        assert!(matches!(
            parse_err(&ok.replace("&expires_at=", "&expires_at=-")),
            RequestError::Invalid("expires_at", _)
        ));
        assert_eq!(
            parse_err(&format!("{ok}&budget_limit=20")),
            RequestError::Unsupported("budget_limit".into())
        );

        let with = |delivery: Delivery, pubkey: bool| {
            let mut request = sample_request(delivery);
            if !pubkey {
                request.ephemeral_hpke_pubkey = None;
            }
            request.validate().unwrap_err()
        };
        assert_eq!(
            with(Delivery::Redirect("https://a.com/cb#frag".into()), true),
            RequestError::Invalid(
                "redirect_uri",
                "Must not contain a fragment".into()
            )
        );
        assert_eq!(
            with(Delivery::Redirect("no-scheme".into()), true),
            RequestError::Invalid("redirect_uri", "Not a uri".into())
        );
        assert_eq!(
            with(Delivery::Redirect("https://a.com/cb".into()), false),
            RequestError::Missing("ephemeral_hpke_pubkey")
        );
        assert_eq!(
            with(Delivery::Mailbox("https://m.com".into()), false),
            RequestError::Missing("ephemeral_hpke_pubkey")
        );
        assert_eq!(
            with(Delivery::Post("http://a.com".into()), true),
            RequestError::Invalid("post_url", "Must be an https:// url".into())
        );
        assert_eq!(
            with(Delivery::Mailbox("https://".into()), true),
            RequestError::Invalid(
                "mailbox_url",
                "Must be an https:// url".into()
            )
        );

        let mut request = base.clone();
        request.params.account =
            Some("x".repeat(CredentialRequestParams::MAX_ACCOUNT_LEN + 1));
        assert_eq!(
            request.validate().unwrap_err(),
            RequestError::Invalid("account", "Exceeds 64 bytes".into())
        );
        let mut request = base;
        request.params.metadata =
            Some("x".repeat(CredentialRequestParams::MAX_METADATA_LEN + 1));
        assert_eq!(
            request.validate().unwrap_err(),
            RequestError::Invalid("metadata", "Exceeds 1024 bytes".into())
        );
    }
}
