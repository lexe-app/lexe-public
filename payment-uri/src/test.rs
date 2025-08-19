use common::{
    ln::network::LxNetwork,
    rng::FastRng,
    test_utils::{arbitrary, arbitrary::any_mainnet_addr_unchecked},
    time::TimestampMs,
};
use proptest::{arbitrary::any, prop_assert_eq, proptest, strategy::Strategy};

use super::*;

// Generate a list of BIP321 address to go in a [`Bip321Uri`]. To support
// roundtripping, we filter out any P2PKH or P2SH addresses that aren't in
// the first position.
pub(crate) fn arb_bip321_addrs(
) -> impl Strategy<Value = Vec<bitcoin::Address<NetworkUnchecked>>> {
    proptest::collection::vec(any_mainnet_addr_unchecked(), 0..3).prop_map(
        |addrs| {
            addrs
                .into_iter()
                .enumerate()
                .filter_map(|(idx, addr)| {
                    if idx != 0 && !addr.is_supported_in_uri_query_param() {
                        None
                    } else {
                        Some(addr)
                    }
                })
                .collect()
        },
    )
}

#[test]
fn test_payment_uri_roundtrip() {
    proptest!(|(uri: PaymentUri)| {
        let any_usable = uri.any_usable();
        let actual = PaymentUri::parse(&uri.to_string());
        prop_assert_eq!(Ok(&uri), actual.as_ref());

        let any_usable_via_flatten = !uri.flatten().is_empty();
        prop_assert_eq!(any_usable, any_usable_via_flatten);
    });
}

#[test]
fn test_payment_uri_parse_doesnt_panic() {
    proptest!(|(s: String)| {
        let _ = PaymentUri::parse(&s);
    });
}

// cargo test -p payment-uri -- payment_uri_sample --ignored --nocapture
#[ignore]
#[test]
fn payment_uri_sample() {
    let mut rng = FastRng::from_u64(891010909651);
    let strategy = any::<PaymentUri>();
    let value_iter = arbitrary::gen_value_iter(&mut rng, strategy);
    for (idx, value) in value_iter.take(50).enumerate() {
        println!("{idx:>3}: \"{value}\"");
    }
}

#[test]
fn test_bip321_uri_manual() {
    // manual test cases

    // just an address
    let address =
        bitcoin::Address::from_str("13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU")
            .unwrap();
    assert_eq!(
        Bip321Uri::parse("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
        Some(Bip321Uri {
            onchain: vec![address.clone()],
            ..Bip321Uri::default()
        }),
    );

    // (proptest regression) funky extra arg
    assert_eq!(
        Bip321Uri::parse("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?foo=%aA"),
        Some(Bip321Uri {
            onchain: vec![address.clone()],
            ..Bip321Uri::default()
        }),
    );

    // weird mixed case `bitcoin:` scheme
    assert_eq!(
        Bip321Uri::parse(
            "BItCoIn:3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV?amount=23.456"
        ),
        Some(Bip321Uri {
            onchain: vec![bitcoin::Address::from_str(
                "3Hk4jJkZkzzGe7oKHw8awFBz9YhRcQ4iAV"
            )
            .unwrap()],
            amount: Some(Amount::from_sats_u32(23_4560_0000)),
            ..Bip321Uri::default()
        }),
    );

    // all caps QR code style
    assert_eq!(
            Bip321Uri::parse(
                "BITCOIN:BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH?label=Luke%20Jr"
            ),
            Some(Bip321Uri {
                onchain: vec![
                    bitcoin::Address::from_str("bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh").unwrap(),
                ],
                label: Some("Luke Jr".to_owned()),
                ..Bip321Uri::default()
            }),
        );

    // ignore extra param & duplicate param
    assert_eq!(
            Bip321Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip321Uri {
                onchain: vec![
                    bitcoin::Address::from_str("bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw").unwrap(),
                ],
                amount: Some(Amount::from_sats_u32(1)),
                message: Some("hello world".to_owned()),
                ..Bip321Uri::default()
            }),
        );

    // ignore onchain if unrecognized req- param
    assert_eq!(
            Bip321Uri::parse(
                "bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?asdf-dfjsijdf=sodifjoisdjf&req-foo=bar&message=hello%20world&amount=0.00000001&message=ignored"
            ),
            Some(Bip321Uri::default()),
        );

    // BOLT12 offer
    let address_str =
        "bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw";
    let address = bitcoin::Address::from_str(address_str).unwrap();
    let offer_str =
        "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q";
    let offer = LxOffer::from_str(offer_str).unwrap();
    let expected = Some(Bip321Uri {
        onchain: vec![address.clone()],
        offer: Some(offer.clone()),
        ..Bip321Uri::default()
    });
    // Support both `lightning=<offer>` and `lno=<offer>` params.
    let actual1 =
        Bip321Uri::parse(&format!("bitcoin:{address_str}?lno={offer_str}"));
    let actual2 = Bip321Uri::parse(&format!(
        "bitcoin:{address_str}?lightning={offer_str}"
    ));
    assert_eq!(actual1, expected);
    assert_eq!(actual2, expected);
}

// roundtrip: Bip321Uri -> String -> Bip321Uri
#[test]
fn test_bip321_uri_prop_roundtrip() {
    proptest!(|(uri: Bip321Uri)| {
        let actual = Bip321Uri::parse(&uri.to_string());
        prop_assert_eq!(Some(uri), actual);
    });
}

// appending junk after the `<address>?` should be fine
#[test]
fn test_bip321_uri_prop_append_junk() {
    proptest!(|(address in any_mainnet_addr_unchecked(), junk: String)| {
        let uri = Bip321Uri {
            onchain: vec![address],
            ..Bip321Uri::default()
        };
        let uri_str = uri.to_string();
        let uri_str_with_junk = format!("{uri_str}?{junk}");
        let uri_parsed = Bip321Uri::parse(&uri_str_with_junk).unwrap();

        prop_assert_eq!(
            uri.onchain.first().unwrap(),
            uri_parsed.onchain.first().unwrap()
        );
    });
}

// support `lightning=<offer>` param
#[test]
fn test_bip321_uri_prop_lightning_offer_param() {
    proptest!(|(uri: Bip321Uri, offer: LxOffer)| {
        let mut uri_raw = uri.to_uri();
        let offer_str = Cow::Owned(offer.to_string());
        let param = UriParam { key: "lightning".into(), value: offer_str };
        uri_raw.params.insert(0, param);

        let actual = Bip321Uri::parse_uri(uri_raw).unwrap();
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

#[rustfmt::skip] // Stop breaking comments
    #[test]
    fn test_bip321_test_vectors() {
        // Must parse and roundtrip
        #[track_caller]
        fn parse_ok_rt(s: &str) -> PaymentUri {
            let uri = PaymentUri::parse(s).unwrap();
            // Ensure it roundtrips
            assert_eq!(s, uri.to_string());
            uri
        }

        // It'll at least parse with some usable `PaymentMethod`s
        #[track_caller]
        fn parse_ok(s: &str) -> PaymentUri {
            let uri = PaymentUri::parse(s).unwrap();
            assert!(uri.any_usable());
            uri
        }

        // Parses but no usable `PaymentMethod`
        #[track_caller]
        fn parse_ok_unusable(s: &str) {
            let uri = PaymentUri::parse(s).unwrap();
            assert!(!uri.any_usable());
        }

        // NOTE: these test vectors are edited to use valid
        // addresses/invoices/offers/etc, otherwise we don't parse them.

        // basic, well-formed URIs that we can fully roundtrip
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?label=Luke-Jr");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?label=Luke-Jr");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz");
        parse_ok_rt("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek");
        parse_ok_rt("bitcoin:?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek");
        parse_ok_rt("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q");

        assert_eq!(
            parse_ok("bitcoin:?bc=bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw&bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("bitcoin:?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
            parse_ok("bitcoin:bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("bitcoin:?tb=tb1qkkxnp5zm6wpfyjufdznh38vm03u4w8q8awuggp"),
            parse_ok("bitcoin:tb1qkkxnp5zm6wpfyjufdznh38vm03u4w8q8awuggp"),
        );
        assert_eq!(
            parse_ok("bitcoin:?bcrt=bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"),
            parse_ok("bitcoin:bcrt1qxvnuxcz5j64y7sgkcdyxag8c9y4uxagj2u02fk"),
        );

        // TODO(phlip9): why does decimal amount 20.3 - roundtrip -> 20.30
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=20.3&label=Luke-Jr"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?amount=20.30&label=Luke-Jr"),
        );

        // TODO(phlip9): "parse" silent payments
        assert_eq!(
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q&sp=sp1qsilentpayment"),
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q"),
        );
        parse_ok_unusable("bitcoin:?sp=sp1qsilentpayment");
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?sp=sp1qsilentpayment"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
        );

        // we currently normalize to lowercase
        assert_eq!(
            parse_ok("BITCOIN:BC1QM9R9X9H2C9WPTAZ0873VYFV8CKX2LCDX8F48UCTTZQFT7R0Q2YASXKT2LW?BC=BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );
        assert_eq!(
            parse_ok("BITCOIN:?BC=BC1QM9R9X9H2C9WPTAZ0873VYFV8CKX2LCDX8F48UCTTZQFT7R0Q2YASXKT2LW&BC=BC1QFJEYFL9PHSDANZ5YAYLAS3P393MU9Z99YA9MNH"),
            parse_ok("bitcoin:bc1qm9r9x9h2c9wptaz0873vyfv8ckx2lcdx8f48ucttzqft7r0q2yasxkt2lw?bc=bc1qfjeyfl9phsdanz5yaylas3p393mu9z99ya9mnh"),
        );

        // ignore unrecognized, not-required params
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?somethingyoudontunderstand=50&somethingelseyoudontget=999"),
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU"),
        );

        // unrecognized req- params => whole on-chain method is unusable
        parse_ok_unusable("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999");
        // but still parse out offers/invoices
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q&somethingelseyoudontget=999"),
            parse_ok("bitcoin:?lno=lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q"),
        );
        assert_eq!(
            parse_ok("bitcoin:13cqLpxv6cZ71X7JjgrdTbLGqhcEzBSBnU?req-somethingyoudontunderstand=50&lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek&somethingelseyoudontget=999"),
            parse_ok("bitcoin:?lightning=lnbc1gcssw9pdqqpp54dkfmzgm5cqz4hzz24mpl7xtgz55dsuh430ap4rlugvywlm4syhqsp5qqtk8n0x2wa6ajl32mp6hj8u9vs55s5lst4s2rws3he4622w08es9qyysgqcqypt3ffpp36sw424yacusmj3hy32df9g97nlwm0a3e0yxw4nd8uau2zdw85lfl5w0h3mggd5g3qswxr9lje0el8g98vul9yec59gf0zxu3eg9rhda09ducxpupsfh36ks9jez7aamsn7hpkxqpw2xyek"),
        );
    }

#[test]
fn test_parse_err_manual() {
    assert_eq!(
        PaymentUri::parse("philip@lexe.app"),
        Err(ParseError::EmailLookingUnsupported),
    );
    assert_eq!(
        PaymentUri::parse("₿philip@lexe.app"),
        Err(ParseError::Bip353Unsupported),
    );
    assert_eq!(
            PaymentUri::parse("lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
    assert_eq!(
            PaymentUri::parse("lnurl:lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
    assert_eq!(
            PaymentUri::parse("lnurlp:lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
    assert_eq!(
            PaymentUri::parse("lnurlp://lnurl1dp68gurn8ghj7um9wfmxjcm99e3k7mf0v9cxj0m385ekvcenxc6r2c35xvukxefcv5mkvv34x5ekzd3ev56nyd3hxqurzepexejxxepnxscrvwfnv9nxzcn9xq6xyefhvgcxxcmyxymnserxfq5fns"),
            Err(ParseError::LnurlUnsupported),
        );
}
