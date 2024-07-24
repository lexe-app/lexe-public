//! Permissive decoding of bitcoin+lightning payment addresses+URIs.
//!
//! This module parses various BTC-related payment methods permissively. That
//! means we should parse inputs that are not strictly well-formed.
//!
//! Other wallet parsers for comparison:
//! + [MutinyWallet/bitcoin_waila](https://github.com/MutinyWallet/bitcoin-waila/blob/master/waila/src/lib.rs)
//! + [breez/breez-sdk - input_parser.rs](https://github.com/breez/breez-sdk/blob/main/libs/sdk-core/src/input_parser.rs)
//! + [ACINQ/phoenix - Parser](https://github.com/ACINQ/phoenix/blob/master/phoenix-shared/src/commonMain/kotlin/fr.acinq.phoenix/utils/Parser.kt)

// `proptest_derive::Arbitrary` issue. This will hard-error for edition 2024 so
// hopefully it gets fixed soon...
// See: <https://github.com/proptest-rs/proptest/issues/447>
#![allow(non_local_definitions)]

use std::{borrow::Cow, fmt, str::FromStr};

use anyhow::ensure;
use common::ln::{
    amount::Amount, invoice::LxInvoice, network::LxNetwork, offer::LxOffer,
};
#[cfg(test)]
use common::{ln::amount, test_utils::arbitrary};
#[cfg(test)]
use proptest::strategy::Strategy;
#[cfg(test)]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;

/// A decoded "Payment URI", usually from a scanned QR code, manually pasted
/// code, or handling a URI open (like tapping a `bitcoin:bc1qfjeyfl...` URI in
/// your mobile browser or in another app).
///
/// Many variants give multiple ways to pay, with e.g. BOLT11 invoices including
/// an onchain fallback, or BIP21 URIs including an optional BOLT11 invoice.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub enum PaymentUri {
    /// A standalone onchain Bitcoin address.
    ///
    /// ex: "bc1qfjeyfl..."
    #[cfg_attr(
        test,
        proptest(
            strategy = "arbitrary::any_mainnet_address().prop_map(Self::Address)"
        )
    )]
    Address(bitcoin::Address),

    /// A standalone BOLT11 Lightning invoice.
    ///
    /// ex: "lnbc1pvjlue..."
    Invoice(LxInvoice),

    /// A standalone BOLT12 Lightning offer.
    ///
    /// ex: "lno1pqps7sj..."
    // TODO(phlip9): BOLT12 refund
    Offer(LxOffer),

    /// A Lightning URI, containing a BOLT11 invoice or BOLT12 offer.
    ///
    /// ex: "lightning:lnbc1pvjlue..." or
    ///     "lightning:lno1pqps7..."
    LightningUri(LightningUri),

    /// An BIP21 URI, containing an onchain payment description, plus optional
    /// BOLT11 invoice and/or BOLT12 offer.
    ///
    /// ex: "bitcoin:bc1qfj..."
    Bip21Uri(Bip21Uri),
    //
    //
    // NOTE: adding support for a new URI scheme? Remember to add it in these
    // places!
    //
    // app/ios/Runner/Info.plist
    // app/macos/Runner/Info.plist
    // app/android/app/src/main/AndroidManifest.xml
}

impl PaymentUri {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();

        // Try parsing a URI-looking thing
        //
        // ex: "bitcoin:bc1qfj..." or
        //     "lightning:lnbc1pvjlue..." or
        //     "lightning:lno1pqps7..." or ...
        if let Some(uri) = Uri::parse(s) {
            // ex: "bitcoin:bc1qfj..."
            if Bip21Uri::matches_scheme(uri.scheme) {
                return Some(Self::Bip21Uri(Bip21Uri::parse_uri_inner(uri)));
            }

            // ex: "lightning:lnbc1pvjlue..." or
            //     "lightning:lno1pqps7..."
            if LightningUri::matches_scheme(uri.scheme) {
                return Some(Self::LightningUri(
                    LightningUri::parse_uri_inner(uri),
                ));
            }

            return None;
        }

        // ex: "lnbc1pvjlue..."
        if let Ok(invoice) = LxInvoice::from_str(s) {
            return Some(Self::Invoice(invoice));
        }

        // ex: "lno1pqps7sj..."
        if let Ok(offer) = LxOffer::from_str(s) {
            return Some(Self::Offer(offer));
        }

        // ex: "bc1qfjeyfl..."
        if let Ok(address) = bitcoin::Address::from_str(s) {
            return Some(Self::Address(address));
        }

        None
    }

    /// "Flatten" the [`PaymentUri`] into its component [`PaymentMethod`]s.
    pub fn flatten(self) -> Vec<PaymentMethod> {
        let mut out = Vec::new();
        match self {
            Self::Address(address) =>
                out.push(PaymentMethod::Onchain(Onchain::from(address))),
            Self::Invoice(invoice) => flatten_invoice_into(invoice, &mut out),
            Self::Offer(offer) => out.push(PaymentMethod::Offer(offer)),
            Self::LightningUri(LightningUri { invoice, offer }) => {
                if let Some(invoice) = invoice {
                    flatten_invoice_into(invoice, &mut out);
                }
                if let Some(offer) = offer {
                    out.push(PaymentMethod::Offer(offer));
                }
            }
            Self::Bip21Uri(Bip21Uri {
                onchain,
                invoice,
                offer,
            }) => {
                if let Some(onchain) = onchain {
                    out.push(PaymentMethod::Onchain(onchain));
                }
                if let Some(invoice) = invoice {
                    flatten_invoice_into(invoice, &mut out);
                }
                if let Some(offer) = offer {
                    out.push(PaymentMethod::Offer(offer));
                }
            }
        }
        out
    }

    /// Resolve the `PaymentUri` into a single, "best" [`PaymentMethod`].
    //
    // phlip9: this impl is currently pretty dumb and just unconditionally
    // returns the first (valid) BOLT11 invoice it finds, o/w onchain. It's not
    // hard to imagine a better strategy, like using our current
    // liquidity/balance to decide onchain vs LN, or returning all methods and
    // giving the user a choice. This'll also need to be async in the future, as
    // we'll need to fetch invoices from any LNURL endpoints we come across.
    pub fn resolve_best(
        self,
        network: LxNetwork,
    ) -> anyhow::Result<PaymentMethod> {
        // A single scanned/opened PaymentUri can contain multiple different
        // payment methods (e.g., a LN BOLT11 invoice + an onchain fallback
        // address).
        let mut payment_methods = self.flatten();

        // Filter out all methods that aren't valid for our current network
        // (e.g., ignore all testnet addresses when we're cfg'd for mainnet).
        payment_methods.retain(|method| method.supports_network(network));

        ensure!(
            !payment_methods.is_empty(),
            "Payment code is not valid for {network}"
        );

        // Pick the most preferable payment method.
        let best = payment_methods
            .into_iter()
            .max_by_key(|x| match x {
                PaymentMethod::Invoice(_) => 2,
                PaymentMethod::Onchain(_) => 1,
                // TODO(phlip9): increase priority when BOLT12 support
                PaymentMethod::Offer(_) => 0,
            })
            .expect("We just checked there's at least one method");

        // TODO(phlip9): remove when BOLT12 support
        ensure!(
            !best.is_offer(),
            "Lexe doesn't currently support Lightning BOLT12 Offers",
        );

        Ok(best)
    }
}

impl fmt::Display for PaymentUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::fmt::Display;
        match self {
            Self::Address(address) => Display::fmt(address, f),
            Self::Invoice(invoice) => Display::fmt(invoice, f),
            Self::Offer(offer) => Display::fmt(offer, f),
            Self::LightningUri(ln_uri) => Display::fmt(ln_uri, f),
            Self::Bip21Uri(bip21_uri) => Display::fmt(bip21_uri, f),
        }
    }
}

/// "Flatten" an [`LxInvoice`] into its "component" [`PaymentMethod`]s, pushing
/// them into an existing `Vec`.
fn flatten_invoice_into(invoice: LxInvoice, out: &mut Vec<PaymentMethod>) {
    let onchain_fallback_addrs = invoice.onchain_fallbacks();
    out.reserve(1 + onchain_fallback_addrs.len());

    // BOLT11 invoices may include onchain fallback addresses.
    if !onchain_fallback_addrs.is_empty() {
        let description = invoice.description_str().map(str::to_owned);
        let amount = invoice.amount();

        for address in onchain_fallback_addrs {
            out.push(PaymentMethod::Onchain(Onchain {
                address,
                amount,
                label: None,
                message: description.clone(),
            }));
        }
    }

    out.push(PaymentMethod::Invoice(invoice));
}

/// A single "payment method" -- each kind here should correspond with a single
/// linear payment flow for a user, where there are no other alternate methods.
///
/// For example, a Unified BTC QR code contains a single [`Bip21Uri`], which may
/// contain _multiple_ discrete payment methods (an onchain address, a BOLT11
/// invoice, a BOLT12 offer).
#[allow(clippy::large_enum_variant)]
pub enum PaymentMethod {
    Onchain(Onchain),
    Invoice(LxInvoice),
    Offer(LxOffer),
}

impl PaymentMethod {
    pub fn is_onchain(&self) -> bool {
        matches!(self, Self::Onchain(_))
    }

    pub fn is_invoice(&self) -> bool {
        matches!(self, Self::Invoice(_))
    }

    pub fn is_offer(&self) -> bool {
        matches!(self, Self::Offer(_))
    }

    pub fn supports_network(&self, network: LxNetwork) -> bool {
        match self {
            Self::Onchain(x) => x.supports_network(network),
            Self::Invoice(x) => x.supports_network(network),
            Self::Offer(x) => x.supports_network(network),
        }
    }
}

/// An onchain payment method, usually parsed from a standalone BTC address or
/// BIP21 URI.
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Onchain {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_mainnet_address()"))]
    pub address: bitcoin::Address,

    #[cfg_attr(
        test,
        proptest(strategy = "amount::arb::sats_amount().prop_map(Some)")
    )]
    pub amount: Option<Amount>,

    /// The recipient/payee name.
    pub label: Option<String>,

    /// The payment description.
    pub message: Option<String>,
}

impl Onchain {
    #[inline]
    pub fn supports_network(&self, network: LxNetwork) -> bool {
        self.address.is_valid_for_network(network.to_bitcoin())
    }
}

impl From<bitcoin::Address> for Onchain {
    fn from(address: bitcoin::Address) -> Self {
        Self {
            address,
            amount: None,
            label: None,
            message: None,
        }
    }
}

/// Parse an onchain amount in BTC, e.g. "1.0024" => 1_0024_0000 sats. This
/// parser also rounds to the nearest satoshi amount, since on-chain payments
/// are limited to satoshi precision.
fn parse_onchain_btc_amount(s: &str) -> Option<Amount> {
    Decimal::from_str(s)
        .ok()
        .and_then(|btc_decimal| Amount::try_from_btc(btc_decimal).ok())
        // On-chain min. denomination
        .map(|amount| amount.round_sat())
}

/// A [BIP21 URI](https://github.com/bitcoin/bips/blob/master/bip-0021.mediawiki).
/// Encodes an onchain address plus some extra metadata.
///
/// Wallets that use [Unified QRs](https://bitcoinqr.dev/) may also include a
/// BOLT11 invoice or BOLT12 offer as `lightning` or `b12` query params.
///
/// Examples:
///
/// ```not_rust
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?label=Luke-Jr
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=20.3&label=Luke-Jr
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999
///
/// bitcoin:175tWpb8K1S7NmH4Zx6rewF9WQrcZv245W?somethingyoudontunderstand=50&somethingelseyoudontget=999
/// ```
#[derive(Debug, Default, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct Bip21Uri {
    pub onchain: Option<Onchain>,
    pub invoice: Option<LxInvoice>,
    pub offer: Option<LxOffer>,
}

impl Bip21Uri {
    const URI_SCHEME: &'static str = "bitcoin";

    pub fn matches_scheme(scheme: &str) -> bool {
        // Use `eq_ignore_ascii_case` as it's technically in-spec for the scheme
        // to be upper, lower, or even mixed case.
        scheme.eq_ignore_ascii_case(Self::URI_SCHEME)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let uri = Uri::parse(s)?;
        Self::parse_uri(uri)
    }

    fn parse_uri(uri: Uri) -> Option<Self> {
        if !Self::matches_scheme(uri.scheme) {
            return None;
        }

        Some(Self::parse_uri_inner(uri))
    }

    fn parse_uri_inner(uri: Uri) -> Self {
        debug_assert!(Self::matches_scheme(uri.scheme));

        let mut out = Self {
            onchain: None,
            invoice: None,
            offer: None,
        };

        // Skip the `Onchain` method if we see any `req-` parameters, as per the
        // spec. However, we're going to partially ignore the spec and
        // unconditionally parse out BOLT11 and BOLT12 pieces, since they're
        // fully self-contained formats. This probably won't be an issue
        // regardless, since `req-` params aren't used much in practice.
        let mut skip_onchain = false;

        // (Unified QR) Parse BOLT11 invoice and/or BOLT12 offer
        // <https://bitcoinqr.dev/>
        for param in &uri.params {
            match param.key.as_ref() {
                "lightning" if out.invoice.is_none() || out.offer.is_none() => {
                    if out.invoice.is_none() {
                        if let Ok(invoice) = LxInvoice::from_str(&param.value) {
                            out.invoice = Some(invoice);
                            continue;
                        }
                    }
                    if out.offer.is_none() {
                        if let Ok(offer) = LxOffer::from_str(&param.value) {
                            // bitcoinqr.dev showcases an offer inside a
                            // `lightning` parameter
                            out.offer = Some(offer);
                            continue;
                        }
                    }
                }

                "b12" if out.offer.is_none() =>
                    out.offer = LxOffer::from_str(&param.value).ok(),

                // We'll respect required && unrecognized bip21 params by
                // throwing out the whole onchain method.
                _ if param.key.starts_with("req-") => skip_onchain = true,

                // ignore duplicates or other keys
                _ => {}
            }
        }

        // Parse `Onchain` payment method
        if !skip_onchain {
            if let Ok(address) = bitcoin::Address::from_str(&uri.body) {
                let mut amount = None;
                let mut label = None;
                let mut message = None;

                for param in uri.params {
                    match param.key.as_ref() {
                        "amount" if amount.is_none() =>
                            amount = parse_onchain_btc_amount(&param.value),
                        "label" if label.is_none() =>
                            label = Some(param.value.into_owned()),
                        "message" if message.is_none() =>
                            message = Some(param.value.into_owned()),

                        // ignore duplicates or other keys
                        _ => {}
                    }
                }

                out.onchain = Some(Onchain {
                    address,
                    amount,
                    label,
                    message,
                });
            }
        }

        out
    }

    fn to_uri(&self) -> Uri<'_> {
        let mut out = Uri {
            scheme: Self::URI_SCHEME,
            body: Cow::Borrowed(""),
            params: Vec::new(),
        };

        // BIP21 onchain portion
        if let Some(onchain) = &self.onchain {
            out.body = Cow::Owned(onchain.address.to_string());

            if let Some(amount) = &onchain.amount {
                out.params.push(UriParam {
                    key: Cow::Borrowed("amount"),
                    // We need to round to satoshi-precision for this to be a
                    // valid on-chain amount.
                    value: Cow::Owned(amount.round_sat().btc().to_string()),
                });
            }

            if let Some(label) = &onchain.label {
                out.params.push(UriParam {
                    key: Cow::Borrowed("label"),
                    value: Cow::Borrowed(label),
                });
            }

            if let Some(message) = &onchain.message {
                out.params.push(UriParam {
                    key: Cow::Borrowed("message"),
                    value: Cow::Borrowed(message),
                });
            }
        }

        // BOLT11 invoice param
        if let Some(invoice) = &self.invoice {
            out.params.push(UriParam {
                key: Cow::Borrowed("lightning"),
                value: Cow::Owned(invoice.to_string()),
            });
        }

        // BOLT12 offer param
        if let Some(offer) = &self.offer {
            out.params.push(UriParam {
                key: Cow::Borrowed("b12"),
                value: Cow::Owned(offer.to_string()),
            });
        }

        out
    }
}

impl fmt::Display for Bip21Uri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_uri(), f)
    }
}

/// A "lightning:" URI, containing a BOLT11 invoice or BOLT12 offer.
///
/// Examples:
///
/// ```not_rust
/// // Short bolt11 invoice
/// lightning:lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek
///
/// // Long bolt11 invoice
/// lightning:lnbc1sd2trl2dk89neerzdaa7amlu9knxnn4malh5jqpmamhle2tr5dym3gpth0h777lwalt5j099a3nr3gpthjk7mm5tp2vg7qnuy7n2gh70l3k27e3s49fmhml0gmc24l9r4xhrcfl9d5vqqqkf32yncmafyca7lm64hjkj76mu5khxk0fp5khnctfpay7x0g88ez8umjv6734rp2tudu3wajgfl0hwll92u3slcfl9d5y3xlr8arhfyjjcmf7z5f82gt7x8trvja003gpt30xuqz5hxghfg0r2us4mcelrulc2alp8u4k32r5gqm7xgmmtgqxqawlwal7wle8ysm7zcg30tn0asv9fg2p9wr6zmq9snv8wpy0dlr4ud4jzeksz0n5x76yaml72h6fdxz5h5fdye07z0etd8shxufjq83nz7ez003jzem2zfza7am7hp2ygnhluyljk60pfa9nsdj9s542ydn5ap0c2208fdtkevsqufzc2jhyzhsn75t0udk56tn6r03jjcgms5m9meg9fycdrjfttqm9cawlwalyle6a25xfplwlwalyheel2vmgumar283s2427cp67wwtpf3afqhq6sjucw78ktsd85xu8rgqyfhqqelzszh9jq8s5x5fuxe07xc6nzg2u2q2uze9yt3gptkghtcfl9d5wul4q23ly32hkrg2dyylr09wnte20wvr72vgh2gpp5q743y4905ggtpapzar8zv3xrkwgfpf7ncq2250kyjgdtxwl7gukqsp53ep2hsj7v0wqy4955x4gfd76qtmehqahledhhey3pjr5ujk4252s9q2sqqqqqysgqcqyp9pknp4qtyrfeasgkr47x72mr6lk0j4665lt4sagdtqark9f3scnymy8cj6zfp4qlwkynsvgrlmy6nkpgv406jccg8fxullw3h9tcqxv5umtx58s20yqr9yq0pf60wnpe294v93fxngec4djvzkqmwu8vpkwecjd7x3xtur8qc4t7dla5tjnkj5f9jnyc5zspavyj6zkqp6n5hpzx8qu69meek3my06q7lznjlhtz6m73ag6lhm3x5t5x0xu2atmnpqnpsefcqedcc8lfz2l02rrsrqmrssz3yqhvejp8r9qpnhea58q8mk890e062tzf9dc5p2sjp56u44zynzdevqfwxxu07kw93g4ex7p2ujd246syj5nlpv5zgv5rfepxygr3rugx2wjm6tgjzm6tu0pv842ajm8uejknh0xlytt95luge0g53ww4y7dzqzcty6fl02cpl4le2ydqdertw2c5g9k664u0revt9h6du0fv6wrszt0qq94kjsw
///
/// // Short bolt12 offer
/// lightning:lno1pgqpvggzdug5w9m7sr4qdrdynkw90mmm2g9qc93ulym3w8pfzc2xmkpdt6es
///
/// // Long bolt12 offer
/// lightning:lno1pt7srytwezar7l6w2ulj4uv932cu3w327zhmmys2fcqzvt6t9rct4d48tfy098avnujrcfx34rcm3y4pyhec8yyw9lcel8ydyhpt2g0352nf8m5vh8efp09nvf4y7agt726ft26u9megd0u37x6eh9n572464ynm7j9f9xln37uteayx37psmuyn3knj55f69a2rluyljk60r9dd3yer628n3kq2es49724g928zszhrc4mqc2zl8rv53ps3kleh6x50rdaxj3wzjf8nszr2z7734q9nv0wz4vq0p8u4knp2tuvchw50r85ls4zqqz0nkjng3u9usxuk3malhheepvu2xs40fz45hmeemy9t03nl9gyt3ynspsuau2q2u0gd7jqthytq9c94euv6h2njeuyljk6rluyljk683s497zmgtfcqu2cgduyljk6q6tl53zw6au484j5jlu90kw4dr2p9c2jnlc5q4mp222nq7wyfnt2z9qjwl0aapdw0p2vrnnceffagu2q2ac5q4mee4tvf84z8kf6x7xu6rwl34x5c5apw6x5qkempvnhmh0l3hz024s469gn0rw485cn0fq4usaw0p0uzjfvz2flnkk3m7lln5z8m22hs4x4tzukrkthmh0cqpcygqjlqvl3l000tzr7szgsrcc8my32l79fckr7nj63n4f4alqznxem3w0l2er4nd4q9mp7fkuysxv5mxwf0gkjawaz4kxdgdp0dt8uncu9xtmuwl6mrjrnh5jg0tgpdqvpmnqytlhs98r8xynazuqqf3w0f4ps7mchujfkn38nps73dcvrxxuqqxdu0w6z9uplpajgz6ujphevcrccrzav2euc7h9ydctaeg9j8es0e3an5c9js5vmqp6ukr6326ae5e0avhxjs82pepypgz5edrrpjgfhxhccmtq0nuljxdcjr5lxeyg9vqkgtr3e5qqejl7s98yzewt6avqfpscw7chgyty43cmp0vml2qm7uk6gg96tglm2ucqfwjkjsd5rd6smc45gtr3n87p9qxq5era4fcqty72z984af4cgw9pzr8f59908hntawk89k0695ycywtsqpqnfufqzscy7eg3rpgvvuwa0vgfgjl5q6u2sm72hch2eu7xu6t08j5j7cpuvyszuk9uu7hxflfpdds0ef384p7zw6dr367xxmdg8352hev2gdc2jlpwywhgav3wsmu2q2uhnx0qdhzghjkw7e2h8n52f2zwnm72qep9e0tysjutnl7wxmhw0nh6pcurwghg4rfuu5379zdu5ajja4xzfuf0cfl9d5p8p2tuyljk6r3s4yp5aqkwkghtceeqy6u2q2ugf00vh5sr00h77j25x34reehxdjc2jhlj969tcfl9d5u2q2as59a7amluujh7qxnuv24xprmuyljk69crw34rptwth3j6jftuyn3wxlpv5ak9au9f0jn6t2au5m3zgkpu49k7nphu4tnkc09r5u5t32dwpy9tnc9pkghfv4kxextte2nzvzvle6p0yttuh9duy5s2vl9tdkknema09wp8p22wkzhynv9ffucwdn7z0jh7ymhs49x4w08294s0ncmvfx7zsg9xcm9t3gptsmytege9dgytce3w4g9g3v9f0r4j73khzu7wrta0g2a7lmmu4xjk9u9tzs2u70rtv3hqmxsleedagpkfcq9mcfl9d56x50p8u4k33n4udtjk97l0aa7ztc82ef9lef4raj7x5m0fc2pfw08za9kyk0r9dn3yhwpg4r727edv7r5chq62fuptccdv5wrva8k93pqd56rmfv68psmvwzzhse6pj4xg7gu6jrzega4qvzs9f9f4wjvftxj
/// ```
#[derive(Debug, Eq, PartialEq)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct LightningUri {
    pub invoice: Option<LxInvoice>,
    pub offer: Option<LxOffer>,
}

impl LightningUri {
    const URI_SCHEME: &'static str = "lightning";

    fn matches_scheme(scheme: &str) -> bool {
        // Use `eq_ignore_ascii_case` as it's technically in-spec for the scheme
        // to be upper, lower, or even mixed case.
        scheme.eq_ignore_ascii_case(Self::URI_SCHEME)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let uri = Uri::parse(s)?;
        Self::parse_uri(uri)
    }

    fn parse_uri(uri: Uri) -> Option<Self> {
        if !Self::matches_scheme(uri.scheme) {
            return None;
        }
        Some(Self::parse_uri_inner(uri))
    }

    fn parse_uri_inner(uri: Uri) -> Self {
        debug_assert!(Self::matches_scheme(uri.scheme));

        let mut out = LightningUri {
            invoice: None,
            offer: None,
        };

        // Try parsing the body as an invoice or offer

        if let Ok(invoice) = LxInvoice::from_str(&uri.body) {
            out.invoice = Some(invoice);
        } else if let Ok(offer) = LxOffer::from_str(&uri.body) {
            out.offer = Some(offer);
        }

        // Try parsing from the query params

        for param in uri.params {
            match param.key.as_ref() {
                "lightning" if out.invoice.is_none() || out.offer.is_none() => {
                    if out.invoice.is_none() {
                        if let Ok(invoice) = LxInvoice::from_str(&param.value) {
                            out.invoice = Some(invoice);
                            continue;
                        }
                    }
                    if out.offer.is_none() {
                        if let Ok(offer) = LxOffer::from_str(&param.value) {
                            out.offer = Some(offer);
                            continue;
                        }
                    }
                }

                "b12" if out.offer.is_none() =>
                    out.offer = LxOffer::from_str(&param.value).ok(),

                // ignore duplicates or other keys
                _ => {}
            }
        }

        out
    }

    fn to_uri(&self) -> Uri<'_> {
        let mut out = Uri {
            scheme: Self::URI_SCHEME,
            body: Cow::Borrowed(""),
            params: Vec::new(),
        };

        // For now, we'll prioritize BOLT11 invoice in the body position, for
        // compatibility.

        if let Some(invoice) = &self.invoice {
            out.body = Cow::Owned(invoice.to_string());

            // If we also have an offer, put it in the "b12" param I guess.
            if let Some(offer) = &self.offer {
                out.params.push(UriParam {
                    key: Cow::Borrowed("b12"),
                    value: Cow::Owned(offer.to_string()),
                });
            }
        } else if let Some(offer) = &self.offer {
            // If we just have an offer, put it in the body
            out.body = Cow::Owned(offer.to_string());
        }

        out
    }
}

impl fmt::Display for LightningUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_uri(), f)
    }
}

/// A raw, parsed URI. The params (both key and value) are percent-encoded. See
/// [URI syntax - RFC 3986](https://datatracker.ietf.org/doc/html/rfc3986).
///
/// ex: `http://example.com?foo=bar%20baz`
/// -> Uri {
///     scheme: "http",
///     body: "//example.com",
///     params: [("foo", "bar baz")],
/// }
#[derive(Debug)]
struct Uri<'a> {
    scheme: &'a str,
    body: Cow<'a, str>,
    params: Vec<UriParam<'a>>,
}

impl<'a> Uri<'a> {
    /// These are the ASCII characters that we will percent-encode inside a URI
    /// query string key or value. We're somewhat conservative here and require
    /// all non-alphanumeric characters to be percent-encoded (with the
    /// exception of of a few control characters, designated in [RFC 3986]).
    ///
    /// Only used for encoding. We will decode all percent-encoded characters.
    ///
    /// [RFC 3986]: https://datatracker.ietf.org/doc/html/rfc3986#section-2.3
    const PERCENT_ENCODE_ASCII_SET: percent_encoding::AsciiSet =
        percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');

    // syntax: `<scheme>:<body>?<key1>=<value1>&<key2>=<value2>&...`
    fn parse(s: &'a str) -> Option<Self> {
        // parse scheme
        // ex: "bitcoin:bc1qfj..." -> `scheme = "bitcoin"`
        let (scheme, rest) = s.split_once(':')?;

        // heuristic: limit scheme to 12 characters. If an input exceeds this,
        // then it's probably not a URI.
        if scheme.len() > 12 {
            return None;
        }

        // ex: "bitcoin:bc1qfj...?message=hello" -> `body = "bc1qfj..."`
        let (body, rest) = rest.split_once('?').unwrap_or((rest, ""));

        // ex: "bitcoin:bc1qfj...?message=hello%20world&amount=0.1"
        //     -> `params = [("message", "hello world"), ("amount", "0.1")]`
        let params = rest
            .split('&')
            .filter_map(UriParam::parse)
            .collect::<Vec<_>>();

        Some(Self {
            scheme,
            body: Cow::Borrowed(body),
            params,
        })
    }
}

impl<'a> fmt::Display for Uri<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = self.scheme;
        let body = &self.body;

        write!(f, "{scheme}:{body}")?;

        let mut sep: char = '?';
        for param in &self.params {
            let key = percent_encoding::utf8_percent_encode(
                &param.key,
                &Self::PERCENT_ENCODE_ASCII_SET,
            );
            let value = percent_encoding::utf8_percent_encode(
                &param.value,
                &Self::PERCENT_ENCODE_ASCII_SET,
            );

            write!(f, "{sep}{key}={value}")?;
            sep = '&';
        }
        Ok(())
    }
}

/// A single `<key>=<value>` URI parameter. Both `key` and `value` are
/// percent-encoded.
#[derive(Debug)]
struct UriParam<'a> {
    key: Cow<'a, str>,
    value: Cow<'a, str>,
}

impl<'a> UriParam<'a> {
    fn parse(s: &'a str) -> Option<Self> {
        let (key, value) = s.split_once('=')?;
        let key = percent_encoding::percent_decode_str(key)
            .decode_utf8()
            .ok()?;
        let value = percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .ok()?;
        Some(Self { key, value })
    }
}

#[cfg(test)]
mod test {
    use common::{
        ln::network::LxNetwork, rng::WeakRng,
        test_utils::arbitrary::any_mainnet_address, time::TimestampMs,
    };
    use proptest::{arbitrary::any, prop_assert_eq, proptest, sample::Index};

    use super::*;

    #[test]
    fn test_payment_uri_roundtrip() {
        proptest!(|(uri: PaymentUri)| {
            let actual = PaymentUri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
        });
    }

    // cargo test -p payment-uri -- payment_uri_sample --ignored --nocapture
    #[ignore]
    #[test]
    fn payment_uri_sample() {
        let mut rng = WeakRng::from_u64(891010909651);
        let strategy = any::<PaymentUri>();
        let value_iter = arbitrary::gen_value_iter(&mut rng, strategy);
        for (idx, value) in value_iter.take(50).enumerate() {
            println!("{idx:>3}: \"{value}\"");
        }
    }

    #[test]
    fn test_bip21_uri_manual() {
        // manual test cases

        // just an address
        let address =
            bitcoin::Address::from_str("13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU")
                .unwrap();
        assert_eq!(
            Bip21Uri::parse("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: address.clone(),
                    amount: None,
                    label: None,
                    message: None,
                }),
                invoice: None,
                offer: None,
            }),
        );

        // (proptest regression) funky extra arg
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?foo=%aA"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: address.clone(),
                    amount: None,
                    label: None,
                    message: None,
                }),
                invoice: None,
                offer: None,
            }),
        );

        // weird mixed case `bitcoin:` scheme
        assert_eq!(
            Bip21Uri::parse(
                "BItCoIn:3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV?amount=23.456"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV"
                    )
                    .unwrap(),
                    amount: Some(Amount::from_sats_u32(23_4560_0000)),
                    label: None,
                    message: None,
                }),
                invoice: None,
                offer: None,
            }),
        );

        // all caps QR code style
        assert_eq!(
            Bip21Uri::parse(
                "BITCOIN:BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH?label=Luke%20Jr"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"
                    )
                    .unwrap(),
                    amount: None,
                    label: Some("Luke Jr".to_owned()),
                    message: None,
                }),
                invoice: None,
                offer: None,
            }),
        );

        // ignore extra param & duplicate param
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip21Uri {
                onchain: Some(Onchain {
                    address: bitcoin::Address::from_str(
                        "bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw"
                    )
                    .unwrap(),
                    amount: Some(Amount::from_sats_u32(1)),
                    label: None,
                    message: Some("hello world".to_owned()),
                }),
                invoice: None,
                offer: None,
            }),
        );

        // ignore onchain if unrecognized req- param
        assert_eq!(
            Bip21Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&req-foo=bar&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip21Uri {
                onchain: None,
                invoice: None,
                offer: None,
            }),
        );

        // BOLT12 offer
        let address_str =
            "bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw";
        let address = bitcoin::Address::from_str(address_str).unwrap();
        let offer_str =
            "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";
        let offer = LxOffer::from_str(offer_str).unwrap();
        let expected = Some(Bip21Uri {
            onchain: Some(Onchain {
                address: address.clone(),
                amount: None,
                label: None,
                message: None,
            }),
            invoice: None,
            offer: Some(offer.clone()),
        });
        // Support both `lightning=<offer>` and `b12=<offer>` params.
        let actual1 =
            Bip21Uri::parse(&format!("bitcoin:{address_str}?b12={offer_str}"));
        let actual2 = Bip21Uri::parse(&format!(
            "bitcoin:{address_str}?lightning={offer_str}"
        ));
        assert_eq!(actual1, expected);
        assert_eq!(actual2, expected);
    }

    // roundtrip: Bip21Uri -> String -> Bip21Uri
    #[test]
    fn test_bip21_uri_prop_roundtrip() {
        proptest!(|(uri: Bip21Uri)| {
            let actual = Bip21Uri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
        });
    }

    // appending junk after the `<address>?` should be fine
    #[test]
    fn test_bip21_uri_prop_append_junk() {
        proptest!(|(address in any_mainnet_address(), junk: String)| {
            let uri = Bip21Uri {
                onchain: Some(Onchain { address, amount: None, label: None, message: None }),
                invoice: None,
                offer: None,
            };
            let uri_str = uri.to_string();
            let uri_str_with_junk = format!("{uri_str}?{junk}");
            let uri_parsed = Bip21Uri::parse(&uri_str_with_junk).unwrap();

            prop_assert_eq!(
                uri.onchain.unwrap().address,
                uri_parsed.onchain.unwrap().address
            );
        });
    }

    // inserting a `req-` URI param should make us to skip the onchain method
    #[test]
    fn test_bip21_uri_prop_req_param() {
        proptest!(|(uri: Bip21Uri, key: String, value: String, param_idx: Index)| {

            let mut uri_raw = uri.to_uri();
            let param_idx = param_idx.index(uri_raw.params.len() + 1);
            let key = format!("req-{key}");
            let param = UriParam { key: key.into(), value: value.into() };
            uri_raw.params.insert(param_idx, param);

            let actual1 = Bip21Uri::parse(&uri_raw.to_string()).unwrap();
            let actual2 = Bip21Uri::parse_uri(uri_raw).unwrap();
            prop_assert_eq!(&actual1, &actual2);
            prop_assert_eq!(None, actual1.onchain);
            prop_assert_eq!(uri.invoice, actual1.invoice);
        });
    }

    // support `lightning=<offer>` param
    #[test]
    fn test_bip21_uri_prop_lightning_offer_param() {
        proptest!(|(uri: Bip21Uri, offer: LxOffer)| {
            let mut uri_raw = uri.to_uri();
            let offer_str = Cow::Owned(offer.to_string());
            let param = UriParam { key: "lightning".into(), value: offer_str };
            uri_raw.params.insert(0, param);

            let actual = Bip21Uri::parse_uri(uri_raw).unwrap();
            let mut expected = uri;
            expected.offer = Some(offer);

            prop_assert_eq!(actual, expected);
        });
    }

    #[test]
    fn test_lightning_uri_manual() {
        // small invoice
        let uri_str = "lightning:lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let invoice = &lightning_uri.invoice.unwrap();
        assert_eq!(lightning_uri.offer, None);
        assert_eq!(invoice.amount(), None);
        assert_eq!(invoice.description_str(), None);
        assert_eq!(invoice.network(), LxNetwork::Mainnet.to_bitcoin());
        assert_eq!(
            invoice.created_at().unwrap(),
            TimestampMs::try_from(9412556961000_i64).unwrap(),
        );
        assert_eq!(
            invoice.payee_node_pk().to_string(),
            "031fd9809565f84bdd53c6708a19a8a4952857fb8d68ee1e5af5e8a666d07ef3a7",
        );
        assert_eq!(invoice.0.route_hints().len(), 0);

        // long invoice
        let uri_str = "lightning:lnbc1sd2trl2dk89neerzdaa7amlu9knxnn4malh5jqpmamhle2tr5dym3gpth0h777lwalt5j099a3nr3gpthjk7mm5tp2vg7qnuy7n2gh70l3k27e3s49fmhml0gmc24l9r4xhrcfl9d5vqqqkf32yncmafyca7lm64hjkj76mu5khxk0fp5khnctfpay7x0g88ez8umjv6734rp2tudu3wajgfl0hwll92u3slcfl9d5y3xlr8arhfyjjcmf7z5f82gt7x8trvja003gpt30xuqz5hxghfg0r2us4mcelrulc2alp8u4k32r5gqm7xgmmtgqxqawlwal7wle8ysm7zcg30tn0asv9fg2p9wr6zmq9snv8wpy0dlr4ud4jzeksz0n5x76yaml72h6fdxz5h5fdye07z0etd8shxufjq83nz7ez003jzem2zfza7am7hp2ygnhluyljk60pfa9nsdj9s542ydn5ap0c2208fdtkevsqufzc2jhyzhsn75t0udk56tn6r03jjcgms5m9meg9fycdrjfttqm9cawlwalyle6a25xfplwlwalyheel2vmgumar283s2427cp67wwtpf3afqhq6sjucw78ktsd85xu8rgqyfhqqelzszh9jq8s5x5fuxe07xc6nzg2u2q2uze9yt3gptkghtcfl9d5wul4q23ly32hkrg2dyylr09wnte20wvr72vgh2gpp5q743y4905ggtpapzar8zv3xrkwgfpf7ncq2250kyjgdtxwl7gukqsp53ep2hsj7v0wqy4955x4gfd76qtmehqahledhhey3pjr5ujk4252s9q2sqqqqqysgqcqyp9pknp4qtyrfeasgkr47x72mr6lk0j4665lt4sagdtqark9f3scnymy8cj6zfp4qlwkynsvgrlmy6nkpgv406jccg8fxullw3h9tcqxv5umtx58s20yqr9yq0pf60wnpe294v93fxngec4djvzkqmwu8vpkwecjd7x3xtur8qc4t7dla5tjnkj5f9jnyc5zspavyj6zkqp6n5hpzx8qu69meek3my06q7lznjlhtz6m73ag6lhm3x5t5x0xu2atmnpqnpsefcqedcc8lfz2l02rrsrqmrssz3yqhvejp8r9qpnhea58q8mk890e062tzf9dc5p2sjp56u44zynzdevqfwxxu07kw93g4ex7p2ujd246syj5nlpv5zgv5rfepxygr3rugx2wjm6tgjzm6tu0pv842ajm8uejknh0xlytt95luge0g53ww4y7dzqzcty6fl02cpl4le2ydqdertw2c5g9k664u0revt9h6du0fv6wrszt0qq94kjsw";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let invoice = &lightning_uri.invoice.unwrap();
        assert_eq!(lightning_uri.offer, None);
        assert_eq!(invoice.amount(), None);
        assert_eq!(invoice.description_str().map(|s| s.len()), Some(444));
        assert_eq!(invoice.network(), LxNetwork::Mainnet.to_bitcoin());
        assert_eq!(
            invoice.created_at().unwrap(),
            TimestampMs::try_from(17626927082000_i64).unwrap(),
        );
        assert_eq!(
            invoice.payee_node_pk().to_string(),
            "02c834e7b045875f1bcad8f5fb3e55d6a9f5d61d43560e8ec54c618993643e25a1",
        );
        let route_hints = invoice.0.route_hints();
        assert_eq!(route_hints.len(), 1);
        let route_hint = &route_hints[0];
        assert_eq!(route_hint.0.len(), 2);

        // short offer
        let uri_str = "lightning:lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let offer = &lightning_uri.offer.unwrap();
        assert_eq!(lightning_uri.invoice, None);
        assert!(offer.supports_network(LxNetwork::Mainnet));
        assert_eq!(offer.description(), None);
        assert_eq!(offer.amount(), None);
        assert_eq!(offer.fiat_amount(), None);
        assert_eq!(
            offer.payee_node_pk().to_string(),
            "024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8",
        );

        // long offer
        let uri_str = "lightning:lno1pt7srytwezar7l6w2ulj4uv932cu3w327zhmmys2fcqzvt6t9rct4d48tfy098avnujrcfx34rcm3y4pyhec8yyw9lcel8ydyhpt2g0352nf8m5vh8efp09nvf4y7agt726ft26u9megd0u37x6eh9n572464ynm7j9f9xln37uteayx37psmuyn3knj55f69a2rluyljk60r9dd3yer628n3kq2es49724g928zszhrc4mqc2zl8rv53ps3kleh6x50rdaxj3wzjf8nszr2z7734q9nv0wz4vq0p8u4knp2tuvchw50r85ls4zqqz0nkjng3u9usxuk3malhheepvu2xs40fz45hmeemy9t03nl9gyt3ynspsuau2q2u0gd7jqthytq9c94euv6h2njeuyljk6rluyljk683s497zmgtfcqu2cgduyljk6q6tl53zw6au484j5jlu90kw4dr2p9c2jnlc5q4mp222nq7wyfnt2z9qjwl0aapdw0p2vrnnceffagu2q2ac5q4mee4tvf84z8kf6x7xu6rwl34x5c5apw6x5qkempvnhmh0l3hz024s469gn0rw485cn0fq4usaw0p0uzjfvz2flnkk3m7lln5z8m22hs4x4tzukrkthmh0cqpcygqjlqvl3l000tzr7szgsrcc8my32l79fckr7nj63n4f4alqznxem3w0l2er4nd4q9mp7fkuysxv5mxwf0gkjawaz4kxdgdp0dt8uncu9xtmuwl6mrjrnh5jg0tgpdqvpmnqytlhs98r8xynazuqqf3w0f4ps7mchujfkn38nps73dcvrxxuqqxdu0w6z9uplpajgz6ujphevcrccrzav2euc7h9ydctaeg9j8es0e3an5c9js5vmqp6ukr6326ae5e0avhxjs82pepypgz5edrrpjgfhxhccmtq0nuljxdcjr5lxeyg9vqkgtr3e5qqejl7s98yzewt6avqfpscw7chgyty43cmp0vml2qm7uk6gg96tglm2ucqfwjkjsd5rd6smc45gtr3n87p9qxq5era4fcqty72z984af4cgw9pzr8f59908hntawk89k0695ycywtsqpqnfufqzscy7eg3rpgvvuwa0vgfgjl5q6u2sm72hch2eu7xu6t08j5j7cpuvyszuk9uu7hxflfpdds0ef384p7zw6dr367xxmdg8352hev2gdc2jlpwywhgav3wsmu2q2uhnx0qdhzghjkw7e2h8n52f2zwnm72qep9e0tysjutnl7wxmhw0nh6pcurwghg4rfuu5379zdu5ajja4xzfuf0cfl9d5p8p2tuyljk6r3s4yp5aqkwkghtceeqy6u2q2ugf00vh5sr00h77j25x34reehxdjc2jhlj969tcfl9d5u2q2as59a7amluujh7qxnuv24xprmuyljk69crw34rptwth3j6jftuyn3wxlpv5ak9au9f0jn6t2au5m3zgkpu49k7nphu4tnkc09r5u5t32dwpy9tnc9pkghfv4kxextte2nzvzvle6p0yttuh9duy5s2vl9tdkknema09wp8p22wkzhynv9ffucwdn7z0jh7ymhs49x4w08294s0ncmvfx7zsg9xcm9t3gptsmytege9dgytce3w4g9g3v9f0r4j73khzu7wrta0g2a7lmmu4xjk9u9tzs2u70rtv3hqmxsleedagpkfcq9mcfl9d56x50p8u4k33n4udtjk97l0aa7ztc82ef9lef4raj7x5m0fc2pfw08za9kyk0r9dn3yhwpg4r727edv7r5chq62fuptccdv5wrva8k93pqd56rmfv68psmvwzzhse6pj4xg7gu6jrzega4qvzs9f9f4wjvftxj";
        let lightning_uri = LightningUri::parse(uri_str).unwrap();
        let offer = &lightning_uri.offer.unwrap();
        assert_eq!(lightning_uri.invoice, None);
        assert!(offer.supports_network(LxNetwork::Mainnet));
        assert_eq!(offer.description().map(|x| x.len()), Some(401));
        assert_eq!(offer.amount(), None);
        assert_eq!(offer.fiat_amount(), None);
    }

    #[test]
    fn test_lightning_uri_roundtrip() {
        proptest!(|(uri: LightningUri)| {
            let actual = LightningUri::parse(&uri.to_string());
            prop_assert_eq!(Some(uri), actual);
        });
    }
}
