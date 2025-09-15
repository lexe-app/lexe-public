use std::{borrow::Cow, fmt, str::FromStr};

use lexe_api_core::types::{invoice::LxInvoice, offer::LxOffer};
#[cfg(test)]
use proptest_derive::Arbitrary;

use crate::{
    helpers,
    payment_method::PaymentMethod,
    uri::{Uri, UriParam},
};

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
#[derive(Debug)]
#[cfg_attr(test, derive(Arbitrary, Eq, PartialEq))]
pub struct LightningUri {
    pub invoice: Option<LxInvoice>,
    pub offer: Option<LxOffer>,
}

impl LightningUri {
    const URI_SCHEME: &'static str = "lightning";

    /// See: [`crate::payment_uri::PaymentUri::any_usable`]
    pub fn any_usable(&self) -> bool {
        self.invoice.is_some() || self.offer.is_some()
    }

    pub(crate) fn matches_scheme(scheme: &str) -> bool {
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

    pub(crate) fn parse_uri_inner(uri: Uri) -> Self {
        debug_assert!(Self::matches_scheme(uri.scheme));

        let mut out = LightningUri {
            invoice: None,
            offer: None,
        };

        // Try parsing the body as an invoice or offer

        if let Ok(invoice) = LxInvoice::from_str(&uri.body) {
            out.invoice = Some(invoice);
        } else if let Ok(offer) = LxOffer::from_str(&uri.body) {
            // non-standard
            out.offer = Some(offer);
        }

        // Try parsing from the query params

        for param in uri.params {
            let key = param.key_parsed();

            if key.is("lightning") && out.invoice.is_none() {
                // non-standard
                out.invoice = LxInvoice::from_str(&param.value).ok();
            } else if (key.is("lno") || key.is("b12")) && out.offer.is_none() {
                // non-standard
                out.offer = LxOffer::from_str(&param.value).ok();
            }
            // ignore duplicates or other keys
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

            // If we also have an offer, put it in the "lno" param I guess.
            if let Some(offer) = &self.offer {
                out.params.push(UriParam {
                    key: Cow::Borrowed("lno"),
                    value: Cow::Owned(offer.to_string()),
                });
            }
        } else if let Some(offer) = &self.offer {
            // If we just have an offer, put it in the body
            out.body = Cow::Owned(offer.to_string());
        }

        out
    }

    pub(crate) fn flatten(self) -> Vec<PaymentMethod> {
        let mut out = Vec::with_capacity(
            self.invoice.is_some() as usize + self.offer.is_some() as usize,
        );

        if let Some(invoice) = self.invoice {
            helpers::flatten_invoice_into(invoice, &mut out);
        }

        if let Some(offer) = self.offer {
            out.push(PaymentMethod::Offer(offer));
        }

        out
    }
}

impl fmt::Display for LightningUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.to_uri(), f)
    }
}

#[cfg(test)]
mod test {
    use common::{ln::network::LxNetwork, time::TimestampMs};
    use proptest::{prop_assert_eq, proptest};

    use super::*;

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
        offer.payee_node_pk().unwrap().to_string(),
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
