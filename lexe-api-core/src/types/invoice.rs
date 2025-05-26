use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::Context;
use common::{
    api::user::NodePk,
    ln::{amount::Amount, network::LxNetwork},
    time::{self, TimestampMs},
};
use lexe_std::Apply;
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::types::payments::{LxPaymentHash, LxPaymentId, LxPaymentSecret};

/// Wraps [`lightning_invoice::Bolt11Invoice`] to impl [`serde`] Serialize /
/// Deserialize using the LDK's [`FromStr`] / [`Display`] impls.
#[derive(Clone, Debug, Eq, PartialEq, SerializeDisplay, DeserializeFromStr)]
pub struct LxInvoice(pub Bolt11Invoice);

impl LxInvoice {
    /// The invoice payment hash. The payer will receive the preimage to this
    /// hash upon successful payment, as proof-of-payment.
    #[inline]
    pub fn payment_hash(&self) -> LxPaymentHash {
        LxPaymentHash::from(*self.0.payment_hash())
    }

    /// The invoice payment secret, used to authenticate the payer to the payee
    /// and tie MPP HTLCs together.
    #[inline]
    pub fn payment_secret(&self) -> LxPaymentSecret {
        LxPaymentSecret::from(*self.0.payment_secret())
    }

    /// Lexe's main identifier for this payment, which for BOLT11 invoice
    /// payments is just the [`LxInvoice::payment_hash`].
    #[inline]
    pub fn payment_id(&self) -> LxPaymentId {
        LxPaymentId::Lightning(self.payment_hash())
    }

    #[inline]
    pub fn network(&self) -> bitcoin::Network {
        self.0.network()
    }

    #[inline]
    pub fn supports_network(&self, network: LxNetwork) -> bool {
        self.network() == network.to_bitcoin()
    }

    /// If the invoice contains a non-empty, inline description, then return
    /// that as a string. Otherwise return None.
    pub fn description_str(&self) -> Option<&str> {
        match self.0.description() {
            Bolt11InvoiceDescriptionRef::Direct(description)
                if !description.as_inner().0.is_empty() =>
                Some(description.as_inner().0.as_str()),
            // Hash description is not useful to us yet
            _ => None,
        }
    }

    /// Return the invoice's requested amount, if present. An invoice may leave
    /// the final amount up to the payer, in which case this field will be None.
    pub fn amount(&self) -> Option<Amount> {
        self.0.amount_milli_satoshis().map(Amount::from_msat)
    }

    /// The invoice amount in satoshis, if included.
    #[inline]
    pub fn amount_sats(&self) -> Option<u64> {
        self.amount().map(|x| x.sats_u64())
    }

    /// Get the invoice creation timestamp. Returns an error if the timestamp
    /// is several hundred million years in the future.
    pub fn created_at(&self) -> Result<TimestampMs, time::Error> {
        TimestampMs::try_from(self.0.timestamp())
    }

    /// Get the invoice creation timestamp unconditionally.
    #[inline]
    pub fn saturating_created_at(&self) -> TimestampMs {
        self.created_at().unwrap_or(TimestampMs::MAX)
    }

    /// `Returns` true if the invoice has expired.
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(TimestampMs::now())
    }

    /// Returns `true` if the invoice expires before the given timestamp.
    #[inline]
    pub fn is_expired_at(&self, ts: TimestampMs) -> bool {
        self.saturating_expires_at() < ts
    }

    /// Get the invoice expiration timestamp. Returns an error if the timestamp
    /// is several hundred million years in the future.
    pub fn expires_at(&self) -> Result<TimestampMs, time::Error> {
        let duration_since_epoch =
            self.0.expires_at().ok_or(time::Error::TooLarge)?;
        TimestampMs::try_from(duration_since_epoch)
    }

    /// Get the invoice expiration timestamp unconditionally.
    #[inline]
    pub fn saturating_expires_at(&self) -> TimestampMs {
        self.expires_at().unwrap_or(TimestampMs::MAX)
    }

    /// Get the invoice payee's [`NodePk`].
    ///
    /// If the pubkey is not included directly in the invoice, we have to
    /// `ecrecover` the pubkey, which is somewhat more expensive (~20-40 us).
    pub fn payee_node_pk(&self) -> NodePk {
        self.0
            .payee_pub_key()
            .copied()
            // If the payee didn't include the pubkey directly in the
            // invoice, we have to `ecrecover` from the msg+signature, which
            // is somewhat more expensive.
            .unwrap_or_else(|| self.0.recover_payee_pub_key())
            .apply(NodePk)
    }

    /// Returns the invoice's `min_final_cltv_expiry_delta` time, if present,
    /// otherwise [`lightning_invoice::DEFAULT_MIN_FINAL_CLTV_EXPIRY_DELTA`].
    pub fn min_final_cltv_expiry_delta_u32(&self) -> anyhow::Result<u32> {
        u32::try_from(self.0.min_final_cltv_expiry_delta())
            .ok()
            .context(
                "Invoice min final CLTV expiry delta too large to fit in a u32",
            )
    }

    /// BOLT11 Invoices can attach optional onchain addresses for a payee to
    /// use if the lightning payment is not feasible. This fn returns those
    /// addresses.
    #[inline]
    pub fn onchain_fallbacks(&self) -> Vec<bitcoin::Address> {
        self.0.fallback_addresses()
    }

    /// Returns `true` if the input string starts with a valid bech32 hrp prefix
    /// for a BOLT11 invoice.
    pub fn matches_hrp_prefix(s: &str) -> bool {
        const HRPS: [&[u8]; 5] = [
            b"lnbc",   // mainnet
            b"lntb",   // testnet
            b"lnsb",   // simnet
            b"lntbs",  // signet
            b"lnbcrt", // regtest
        ];
        let s = s.as_bytes();
        HRPS.iter().any(|hrp| match s.split_at_checked(hrp.len()) {
            Some((prefix, _)) => prefix.eq_ignore_ascii_case(hrp),
            None => false,
        })
    }
}

impl FromStr for LxInvoice {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Bolt11Invoice::from_str(s).map(Self).map_err(ParseError)
    }
}

impl Display for LxInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError(pub lightning_invoice::ParseOrSemanticError);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let err = &self.0;
        write!(f, "Failed to parse Lightning invoice: {err}")
    }
}

impl std::error::Error for ParseError {}

#[cfg(any(test, feature = "test-utils"))]
pub mod arbitrary_impl {
    use std::time::Duration;

    use bitcoin::{
        hashes::{sha256, Hash},
        secp256k1::{self, Message},
    };
    use byte_array::ByteArray;
    use common::{
        rng::{Crng, FastRng},
        root_seed::RootSeed,
        test_utils::arbitrary,
    };
    use lightning::{
        routing::router::RouteHint, types::payment::PaymentSecret,
    };
    use lightning_invoice::{Fallback, InvoiceBuilder, MAX_TIMESTAMP};
    use proptest::{
        arbitrary::{any, Arbitrary},
        option, result,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::types::payments::LxPaymentPreimage;

    #[derive(Default)]
    pub struct LxInvoiceParams {
        pub payment_preimage: Option<LxPaymentPreimage>,
    }

    impl Arbitrary for LxInvoice {
        type Parameters = LxInvoiceParams;
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            let bytes32 = any::<[u8; 32]>().no_shrink();

            let node_key_pair = any::<FastRng>().prop_map(|mut rng| {
                RootSeed::from_rng(&mut rng).derive_node_key_pair(&mut rng)
            });
            let network = any::<LxNetwork>();
            let description_or_hash =
                result::maybe_ok(arbitrary::any_simple_string(), bytes32);
            let timestamp = (0..MAX_TIMESTAMP).prop_map(Duration::from_secs);

            let payment_secret = bytes32;
            // Allow the caller to override the payment preimage. If the caller
            // overrides the payment preimage, generate the actual payment hash
            // from that preimage.
            let payment_hash = bytes32.prop_map(move |bytes| {
                args.payment_preimage
                    .map(|preimage| preimage.compute_hash().to_array())
                    .unwrap_or(bytes)
            });

            let min_final_cltv_expiry_delta = any::<u16>();
            let amount = any::<Option<Amount>>();
            let expiry_duration = arbitrary::any_option_duration();
            let metadata = any::<Option<Vec<u8>>>();
            let add_pubkey = any::<bool>();
            let fallback = option::of(arbitrary::any_onchain_fallback());
            let route_hint = arbitrary::any_invoice_route_hint();

            // need to group some generators into their own sub-tuples since
            // proptest only impls `Strategy` for tuples with <= 12
            // elements...

            let ext = (fallback, route_hint);

            (
                node_key_pair,
                network,
                description_or_hash,
                timestamp,
                payment_secret,
                payment_hash,
                min_final_cltv_expiry_delta,
                amount,
                expiry_duration,
                metadata,
                add_pubkey,
                ext,
            )
                .prop_map(
                    |(
                        node_key_pair,
                        network,
                        description_or_hash,
                        timestamp,
                        payment_secret,
                        payment_hash,
                        min_final_cltv_expiry_delta,
                        amount,
                        expiry_duration,
                        metadata,
                        add_pubkey,
                        (fallback, route_hint),
                    )| {
                        gen_invoice(
                            node_key_pair,
                            network,
                            description_or_hash,
                            timestamp,
                            payment_secret,
                            payment_hash,
                            min_final_cltv_expiry_delta,
                            amount,
                            expiry_duration,
                            metadata,
                            add_pubkey,
                            fallback,
                            route_hint,
                        )
                    },
                )
                .boxed()
        }
    }

    /// Un-builder-ify the [`InvoiceBuilder`] API, since the extra type params
    /// get in the way when generating via proptest. Only used during testing.
    pub(super) fn gen_invoice(
        node_key_pair: secp256k1::Keypair,
        network: LxNetwork,
        description_or_hash: Result<String, [u8; 32]>,
        timestamp: Duration,
        payment_secret: [u8; 32],
        payment_hash: [u8; 32],
        min_final_cltv_expiry_delta: u16,
        amount: Option<Amount>,
        expiry_duration: Option<Duration>,
        metadata: Option<Vec<u8>>,
        add_pubkey: bool,
        fallback: Option<Fallback>,
        route_hint: RouteHint,
    ) -> LxInvoice {
        // This rng doesn't affect the output.
        let secp_ctx = FastRng::from_u64(981999).gen_secp256k1_ctx();

        // Build invoice

        let invoice = InvoiceBuilder::new(network.into());

        let invoice = match description_or_hash {
            Ok(string) => invoice.description(string),
            Err(hash) =>
                invoice.description_hash(sha256::Hash::from_byte_array(hash)),
        };

        let mut invoice = invoice
            .duration_since_epoch(timestamp)
            .payment_hash(sha256::Hash::from_byte_array(payment_hash))
            .payment_secret(PaymentSecret(payment_secret))
            .basic_mpp()
            .min_final_cltv_expiry_delta(min_final_cltv_expiry_delta.into());

        if let Some(amount) = amount {
            let msat = amount
                .invoice_safe_msat()
                .unwrap_or(Amount::INVOICE_MAX_AMOUNT_MSATS_U64);
            invoice = invoice.amount_milli_satoshis(msat);
        }
        if let Some(expiry_duration) = expiry_duration {
            let expiry_time = timestamp
                .saturating_add(expiry_duration)
                .min(Duration::from_secs(MAX_TIMESTAMP));
            invoice = invoice.expiry_time(expiry_time);
        }
        if add_pubkey {
            invoice = invoice.payee_pub_key(node_key_pair.public_key());
        }
        if let Some(fallback) = fallback {
            invoice = invoice.fallback(fallback);
        }
        if !route_hint.0.is_empty() {
            invoice = invoice.private_route(route_hint);
        }

        // Sign invoice

        let do_sign = |msg: &Message| {
            secp_ctx.sign_ecdsa_recoverable(msg, &node_key_pair.secret_key())
        };

        let invoice = match metadata {
            Some(metadata) =>
                invoice.payment_metadata(metadata).build_signed(do_sign),
            None => invoice.build_signed(do_sign),
        };

        LxInvoice(invoice.expect("Failed to build and sign invoice"))
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use byte_array::ByteArray;
    use common::{
        rng::FastRng,
        root_seed::RootSeed,
        test_utils::{arbitrary, roundtrip, snapshot},
    };
    use lightning::{
        ln::channelmanager::MIN_FINAL_CLTV_EXPIRY_DELTA,
        routing::router::RouteHint,
    };
    use proptest::{arbitrary::any, prop_assert, prop_assert_eq, proptest};
    use test::arbitrary_impl::gen_invoice;

    use super::*;

    #[test]
    fn invoice_deser_compat() {
        let inputs = r#"
--- amount-less invoice, no description, Lexe.app
lnbc1pnap4p0dqqpp5e6wxwnkvtsf9eehvqg3q04wm0rv2vlmaswky4naakc083kxa439qcqpcsp5k8vexrcrthdcdyazfv3c7r8za7mw3sl9c6hefwu0ll3ef8uwzu0q9qyysgqxqyz5vqnp4q0w73a6xytxxrhuuvqnqjckemyhv6avveuftl64zzm5878vq3zr4jrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz0nfsqq0mgqqvqqqqqqqqqqhwqqfqawdgj4wyt2gcp2k6yqwzdpymmlgearh5hu8vz24r5my73vdv3zuyyra8yxa9zkf26fvjf8ru3rfjamq9agkw6ta9wect076du6p3mvcp9w4lu3
--- 50 sat invoice w/ description, Lexe.app
lnbc500n1pnapns2dq68skjqnr90pjjqstwv3ex76tyyqpp54yl0p0ezxl2qasdc0ect5tmxj9rdcxry7paszzdpfa5ka79t4jgscqpcsp5fc7u3hs62lr9d77xkwnjaa4fs2fch99lh96gh40kzgnufq2rvmks9qyysgqxqyz5vqnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5advrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz2g6uqqt5cqqcqqqqqqqqqqhwqqfqfhue440klc35tlmacewtk6sm3jxvkf8ddcvpggfqf4xj6mny6s7zvjwjqrjy4map9av4t82vtxrqlcqnedlwp67l6zw2x3ctf8a6amgp9v6j74
--- mainnet
lnbc16617464075412908110p1du587g2hp5j25fhdvhz66tctmq6xga76mdddz62xm7fk6n7alekxd4d8y2rqyspp5axv2wz5w2upckqf0exq3wwg09kkha9zushwm6tzmqsenkgafeyeqsp5usuarqswxgkpk6skvydnyaw56xpv400xy0auh8zjg3r4t9mqp8fs9qyysgqcqypvxfx04nrfq3khmpat4e9k5a2d52eup290rkts344t7z942zzhmk70h9ywtwu6kqc8hpx3af6tjakute4xq29h3rhdq50vcecjrxredxh0gqlm8nvn
--- regtest
lnbcrt280u1pnxywwgdqqpp52t2fd5p8kuqn370uae3f3vezj6mjlzsuynfgkd9533xqp3vyd44scqpcsp5truuwxdmk38t9zad3al685uw6a4yg0gncg8p8yzy69asy7rz3uyq9qyysgqxqrrssnp4qfjfnyxh2n3yh2d9fqt293lfahnzfllg4qj2cu9lz04e97u2njx6vrzjqdd8p4z7a3l0kfcrr8c3d2tggfg2ed809q4zd5scwjrculzs3rmnkqqqqyqqrasqq5qqqqqqqqqqhwqqfqkqddwf80knvfd5naznztzzfm9glx7v8lhchjljjxnhknre9rwd6y3qcjn92ewl9dquc60jxhh8e0d6pd9ejsskutyr6rp6xpc0ex36spnalh5l
--- long invoice 1
lnbc10000000000000000010p1qqqqqqqdtuxpqkzq8sjzqgps4pvyczqq8sjzqgpuysszq0pyyqsrp2zs0sjzqgps4pxrcfpqyqc2slpyyqsqsv9gwz59s5zqpqyps5rc9qsrs2pqxz5ysyzcfqgysyzs0sjzqgqq8sjzqgps4pxqqzps4pqpssqgzpxps5ruysszqrps4pg8p2zgpsc2snpuysszqzqsgqvys0pyyqsrcfpqyqvycv9gfqqrcfpqyq7zggpq8q5zqyruysszqwpgyqxpsjqsgq7zggpqps7zggpq8sjzqgqgqq7zggpqpq7zggpq8q5zqqpuysszq0pyyqsqs0pyyqspsnqgzpqpqlpyyqsqszpuysszqyzvzpvysrqq8sjzqgqvrp7zggpqpqxpsspp5mf45hs3cgphh0074r5qmr74y82r26ac4pzdg4nd9mdmsvz6ffqpssp5vr4yra4pcv74h9hk3d0233nqu4gktpuykjamrafrdpuedqugzh3q9q2sqqqqqysgqcqrpqqxq8pqqqqqqnp4qgvcxpme2q5lng36j9gruwlrtk2f86s3c5xmk87yhvyuwdeh025q5r9yqwnqegv9hj9nzkhyxaeyq92wcrnqp36pyrc2qzrvswj5g96ey2dn6qqqqqqqqqqqqqqqqqqqqqqqqqqqqqp9a5vs0t4z56p64xyma8s84yvdx7uhqj0gvrr424fea2wpztq2fwqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqmy9qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpcnsxc32du9n7amlypuhclzqrt6lkegq0v3r7nczjv9tv30z7phq80r3dm7pvgykl7gwuenmem93h5xwdwac6ngsmzqc34khrg3qjgsq6qk6lc
--- long invoice 2
lnbc8735500635020489010p1av5kfs8deupvyk4u5ynj03hmalhhhml0fxc2jlrv9z4lg6s4hnhkz69malhhet3x9yqpsxru4a3kwar2qtu2q2ughx367q600s5x7c7tln4k0fu78skxqevaqm8sayhuur377zgf3uf94n57xzhdw99u42hwc089djn5xj723w7zageflsnzdmyte89tecf2ac7xhg4y3u9f4xpuv2hwxjlsarp0e24fu8tme6rgv0tqj08z9f4u30rw59k8emhtvs7wye0xfw6x5q5tju2p208rvtkunzwtwghtp22tlnh62gxwhfkxp4cnz7ts3rxvlzszhv9y00h77lpdvcjyhjtmalh5dn5e8n5w8cqle0vunzduu4nza9y0734qhxday9hzywl0aa0vhzy0qmphc64d4hduj08dv2krpgqtc2v83gptk34reelxyc7wsgnze890c6nrv6p0cmepatc269eayzjjkqk30n52rfl5dg7wztl96f7wc2tzx34q909xuajnyt4u4lnk87lwal7z0etdz5tmece0v3u796jfp68nccn05ty54ncfelts3v8g0sn6v6hsu87zat4r03368ersu87252dd0nswymxzc2pyxl8yy844hspuyj47w0px4u4leefq568sk0rr9th4ql9f9ykawrczkz5hp22nstg3lrlsa6u2q2ull3kzce2sh0h77sjv0zszhzy4hfh6u0pwux5l3gpthsn72mfu47sw9zw3hzk7srznp27z0etdp0725me00sn72mgkf0fteehruk0lg6swh34z52puaekzmjlmalhhe6m8ug7z3c8g8zhjjspp5zj0sm85g5ufng9w7s6p4ucdk80tyvz64sg54v0cy4vgnr37f78sqsp5l6azu2hv6we30er90jrslqpvdtrnrphhesca2wg5q83k52rsu2cq9q2sqqqqqysgqcqr8h2np4qw0ha2k282hm8jh5rcfq0hsp2zhddtlc5vs23uphyv0lv3k8sqsfgfp4qyrk86tx5xg2aa7et4cdzhnvl5s4nd33ugytt7gamk9tugn9yransr9yq08gpwsn8t2tq4ducjfhrcz707av0ss20urjh8vldrpmehqxa0stkesvuq82txyqzfhej7qccswy7k5wvcppk63c6zpjytfdaccadacjtn52lpe6s85rjfqlxzp6frq33xshaz2nr9xjkhd3jj8qg39nmfzvpgmayakqmy9rseakwgcudug7hs45wh430ywh7qhj3khczh8gle4cn93ymgfwa7rrvcw9lywyyz58k4p40a3nu9svthaf0qeg8f2ay4tw9p48p70qmayu3ejl2q8pj9e2l22h7775tl44hs6ke4sdfgcr6aj8wra4r2v9sj6xa5chd5ctpfg8chtrer3kkp0e6af88lkrfxcklf2hyslv2hr0xl5lwrm5y5uttxn4ndfz8789znf78nspa3xy68
--- long invoice 3
lnbcrt17124979001314909880p1y6lkcwgd76tfnxksfk2atyy4tzw4nyg6jrx3282s2ygvcxyj64gevhxsjk2ymhzv3e0p5h5u3kfey92jt9ge44gsfnwycxynm2g3unw3ntt9qh25texe98jcfhxvcxuezxw9tngwrndpy9s4p4x9eyze2tfe9rxm68tp5yj5jfduen2nny8prhsm6edegn2stww4n4gwp4vfjkvdthd43524n9fa8h262vweskg66nw3vnyafn29zhsvfeg9mxummtfp35uumzfqmhy3jwgdh55mt5xpvhgmjn25uku5e5g939wmmnvdfygnrdgdh56uzcx4a92vfhgdcky3z9gfnrsvp4f4f55j68vak9yufhvdm8x5zrgc6955jvf429zumv89nh2a35wae5yntgv985jumpxehyv7t92pjrwufs89yh23f5ddy5s568wgchve3cg9ek5nzewgcrzjz0dftxg3nvf4hngje52ac4zmesxpvk6sfef4hkuetvd4vk6n29wftrw5rvg4yy2vjjwyexc5mnvfd8xknndpqkkenx0q642j35298hwve3dyc525jrd3295sm9v9jrqup3wpykg7zd239ns7jgtqu95jz0deaxksjh2fu56n6n2f5x6mm8wa89qjfef385sam2x9mxcs20gfpnq460d3axzknnf3e4sw2kvf25wjjxddpyg52dw4vx7nn2w9cyu5t8vfnyxjtpg33kssjp24ch536pd938snmtx345x6r4x93kvv2tff855um3tfekxjted4kxys2kve5hvu6g89z4ynmjgfhnw7tv892rymejgvey77rcfqe9xjr92d85636fvajxyajndfa92k2nxycx5jtjx4zxsm2y2dyn2up50f5ku3nrfdk4g5npxehkzjjv8y69gveev4z56denddaxy7tfwe8xx42zgf6kzmnxxpk826ze2s6xk6jrwearw6ejvd8rsvj2fpg525jtd5pp5j2tlt28m4kakjr84w6ce4fd8e7awy6ncyswcyut760rdnem30ptssp5p5u3xgxxtr6aev8y2w9m30wcw3kyn7fgm8wmf8qw8wzrqt34zcvq9q2sqqqqqysgqcqypmw9xq8lllllllnp4qt36twam2ca08m3s7vnhre3c0j89589wyw4vdk7fln0lryxzkdcrur28qwqq3hnyt84vsasuldd2786eysdf4dyuggwsmvw2atftf7spkmpa9dd3efq5tenpqm2v7vcz2a4s0s7jnqpjn0srysnstnw5y5z9taxn0ue37aqgufxcdsj6f8a2m4pm9udppdzc4shsdqzzx0u0rm4xljs0dqz3c5zqyvglda7nsqvqfztmlyup7vyuadzav4zyuqwx90ev6nmk53nkhkt0sev9e745wxqtdvrqzgqkakazen7e2qmsdauk665g3llg5qtl79t3xulrhjnducehdn72gpmkjvtth7kh6ejpl9dv0qcsxv2jvzzvg0hzdmk3yjsmydqksdk3h78kc63qnr265h8vyeslqexszppfm7y287t3gxvhw0ulg2wp0rsw3tevz03z50kpy77zdz9snxmkkwxd76xvj4qvj2f89rrnuvdvzw947ay0kydc077pkec2jet9qwp2tud98s24u65uz07eaxk5jk3e4nggn2caaek2p5pkrc6mm6mxjm2ezpdu8p5jstg6tgvnttgac3ygt5ys04t4udujzlshpl7e4f3ff03xe6v24cp6aq4wa
--- long invoice 4
lntb5826417333454665580p1c5rwh5edlhf33hvkj5vav5z3t02a5hxvj3vfv5kuny2f3yzj6zwf9hx3nn2fk9gepc2a3ywvj6dax5v3jy2d5nxmp3gaxhycjkv38hx4z4d4vyznrp2p24xa6t2pg4w4rrxfens6tcxdhxvvfhxa8xvvpkgat8xnpe2p44juz9g43hyur00989gvfhwd2kj72wfum4g4mgx5m5cs2rg9d9vnn6xe89ydnnvfpyy52s2dxx2er4x4xxwstdd5cxwdrjw3nkxnnv2uexxnrxw4t56sjswfn52s2xv4t8xmjtwpn8xm6sfeh4q526dyu8x3r9gceyw6fhd934qjttvdk57az5w368zdrhwfjxxu35xcmrsmmpd4g8wwtev4tkzutdd32k56mxveuy6c6v2emyv7zkfp39zjpjgd8hx7n4xph5kceswf6xxmnyfcuxca20fp24z7ncvfhyu5jf2exhw36nwf68s7rh2a6yzjf4dgukcenfxpchqsjn2pt5x334tf98wsm6dvcrvvfcwapxvk2cdvmk2npcfe68zue3w4f9xc6s2fvrw6nrg3fkskte2ftxyc20ffckcd692964sdzjwdp4yvrfdfm9q72pxp3kwat5f4j9xee5da8rss60w92857tgwych55f5w3n8zmzexpy4jwredejrqm6txf3nxm64ffh8x460dp9yjazhw4yx6dm5xerysnn5wa455k3h2d89ss2fd9axwjp3f4r9qdmfd4fx6stx2eg9sezrv369w7nvvfvhj4nnwaz5z3ny8qcxcdnvwd64jc2nx9uy2e2gxdrnx6r3w9ykxatxxg6kk6rv2ekr2emwx5ehy362d3x82dzvddfxs5rcg4vn27npf564qdtg2anycc6523jnwe3e0p65unrpvccrs5m2fuexgmnj23ay5e34v4xk5jnrwpg4xemfwqe5vjjjw9qk76zsd9yrzu6xdpv5v5ntdejxg6jtv3kx65t6gdhrgvj3fe34sj2vv3h5kegpp57hjf5kv6clw97y2e063yuz0psrz9a6l49v836dflum00rh8qtn8qsp5gd29qycuze08xls8l32zjaaf2uqv78v97lg9ss0c699huw980h2q9q2sqqqqqysgqcqr8ulnp4q26hcfwr7qxz7lwwlr2kjcrws7m2u5j36mm0kxa45uxy6zvsqt2zzfppjdkrm2rlgadt9dq3d6jkv4r2cugmf2kamr28qwuleyzzyyly8a6tu70eldahx7hzxx5x9gms7vjjr577ps8n4qyds5nern39j0v7czkch2letnt46895jupxgehf208xgxz8d6j8gu3h2qqtsk9nr9nuquhkqjxw40h2ucpldrawmktxzxdgtkt9a3p95g98nywved8s8laj2a0c98rq5zzdnzddz6ndw0lvr6u0av9m7859844cgz9vpeq05gw79zqae2s7jzeq66wydyueqtp56qc67g7krv6lj5aahxtmq4y208q5qyz38cnwl9ma6m5f4nhzqaj0tjxpfrk4nr5arv9d20lvxvddvffhzygmyuvwd959uhdcgcgjejchqt2qncuwpqqk5vws7dflw8x6esrfwhz7h3jwmhevf445k76nme926sr8drsdveqg7l7t7lnjvhaludqnwk4l2pmevkjf9pla924p77v76r7x8jzyy7h59hmk0lgzfsk6c8dpj37hssj7jt4q7jzvy8hq25l3pag37axxanjqnq56c47gpgy6frsyc0str9w2aahz4h6t7axaka4cwvhwg49r6qgj8kwz2mt6vcje25l9ekvmgq5spqtn
--- long invoice 5
lnbc1mmj7z2hd427xtea2gtw8et4p5ta7lm6xe02nemhxvg7zse98734qudr2pucwaz3ua647tl9tv8nsnv3whzszhv89lnhwum9u4vk284sfr6c2jnee9yhcn08qd65ghznuac4w5l9fahknyt4sugg0p22tqqdsxlrf465fwzvupw7z7tm9p97wyerp8j5676zafx8s9rlj967x4cld6dty3w9q9w7zvm8wl3kkacj4l3hkzcauutj630rx5pnp4l805v50g63ayd5w7lr8yrsvqxq3ljh6mf0uuxsjlq5fqtfsdsqrgd8unzuxl35zvgemamhu9kreu9kc7jduuq5ksdr2r4pd74ct39ytpgku6u6x5y5h8zszhrnuuqs6qz2uex7z3m9v7r5lc2lval8nhml0wrja0lp89yj4rv8gcm7xammz7rnuj7l0aap929c7rva7lmmmypk5jlrw4m5eqrmuap46szva32a7lm6fl0hwljauvrjzjj7r00hwljfuvn46vdr2x34pvjz37r3unj5ne8yshreuuxhzhphu4l46hlrvak4q3q4u40jkn9euveh2cxqhpy9me23r9neza870p67xug4qk34refmtufu2wg5wju7wlm4tjurvxlrvu8n8cfdd538tcm8z9nezar5tcm7z0etd834kkmgwny7zgt4y7ghgpp5f5a605yw4snw7vw44vdzyk9stc04t0993vqeau6f4c4qnvkmdc7ssp5hae8vd4p95e3shsu44ewsc7g0yy723htt3zgdd82dnn6agvj0ups9qyysgqcqypaqaxq8lllllllfppqsmsyt0w88nycxpxa4v8rd0yevu6e436vrzjqwjpdn2djrehkzaaydyzrk6kg5nhtsjrmdqalv4qvuvfhjg0uev9rund8d9uqxt5kadklnrf28wd4crjscqkhvztylj52y75en2z7jmsjs6pz72kujhcn46v0wvh6wcy7xsgzqkrr5m7gf35eps9pkuln0upsw6jkunmnv8ma3tqph5r8yyaj9vkgp0ymxnf
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let invoice = LxInvoice::from_str(input).unwrap();
            let invoice_str = invoice.to_string();
            assert_eq!(input, invoice_str);
        }
    }

    #[test]
    fn invoice_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxInvoice>();
    }

    #[test]
    fn invoice_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxInvoice>();
    }

    #[test]
    fn invoice_matches_hrp_prefix() {
        proptest!(|(invoice: LxInvoice)| {
            let mut invoice_str = invoice.to_string();
            prop_assert!(LxInvoice::matches_hrp_prefix(&invoice_str));

            // uppercase
            invoice_str.make_ascii_uppercase();
            prop_assert!(LxInvoice::matches_hrp_prefix(&invoice_str));
            prop_assert_eq!(LxInvoice::from_str(&invoice_str).unwrap(), invoice);
        });
    }

    // Generate example invoices using the proptest strategy.
    #[ignore]
    #[test]
    fn invoice_sample_data() {
        let mut rng = FastRng::from_u64(366519812156561);
        let strategy = any::<LxInvoice>();
        let value_iter = arbitrary::gen_value_iter(&mut rng, strategy);

        for value in value_iter.take(10) {
            let value_str = value.to_string();
            dbg!(value);
            dbg!(value_str);
        }
    }

    // Generate example invoices with specific values.
    // ```bash
    // $ cargo test -p common -- --ignored invoice_dump --nocapture
    // ```
    #[ignore]
    #[test]
    fn invoice_dump() {
        let node_key_pair = RootSeed::from_u64(12345)
            .derive_node_key_pair(&mut FastRng::from_u64(123));

        let network = LxNetwork::Regtest;
        let amount = None;
        let created_at = Duration::from_millis(1741232485);
        let expires_at = Some(Duration::from_millis(1741233485));
        let description_or_hash = Ok("Snacks".to_owned());
        let payment_secret = sha256::digest(b"sldfsjldfjsodifj");
        let payment_hash = sha256::digest(b"sldfj8881s4)");
        let min_final_cltv_expiry_delta = MIN_FINAL_CLTV_EXPIRY_DELTA;
        let metadata = None;
        let add_pubkey = false;
        let fallback = None;
        let route_hint = RouteHint(vec![]);

        dbg!(network);
        dbg!(amount);
        dbg!(created_at.as_millis());
        dbg!(expires_at.map(|x| x.as_millis()));
        dbg!(&description_or_hash);
        dbg!(payment_secret);
        dbg!(payment_hash);
        dbg!(min_final_cltv_expiry_delta);
        dbg!(&metadata);
        dbg!(node_key_pair.public_key());
        dbg!(add_pubkey);
        dbg!(&fallback);
        dbg!(&route_hint);

        let invoice = gen_invoice(
            node_key_pair,
            network,
            description_or_hash,
            created_at,
            payment_secret.to_array(),
            payment_hash.to_array(),
            min_final_cltv_expiry_delta,
            amount,
            expires_at.map(|x| x.saturating_sub(created_at)),
            metadata,
            add_pubkey,
            fallback,
            route_hint,
        );

        let invoice_str = invoice.to_string();
        dbg!(&invoice_str);
    }

    // Decode and print an invoice
    // ```bash
    // $ cargo test -p common -- --ignored invoice_print --nocapture
    // ```
    #[ignore]
    #[test]
    fn invoice_print() {
        let s = "lnbc1pn79l2rdqqpp5y3u8cttsjvusa34xnx9ceh8watmrvy99qw7pwpsvxjq3zl2mm8wscqpcsp5p4wrl7xfrgxj3w05ksjv2qtccyt0feg2c0suwcjc5pyrawxvlt0q9qyysgqxqyz5vqnp4q0vzagw8x7r9eyalw35t0u6syql8rtqf9tejep0z6xrwkqrua5advrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wrf0egqqy2sqqcqqqqqqqqqqhwqqfqrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wzmk4uqqj5sqqyqqqqqqqqqqhwqqfqrzjqv22wafr68wtchd4vzq7mj7zf2uzpv67xsaxcemfzak7wp7p0r29wz2g6uqqt5cqqcqqqqqqqqqqhwqqfqd5xs0luhzmmdmevhqtcyuwrcr43pq3xpmtdvdenvcsslg8vuhmfyqtcs3y54yxpsw8wlt5epz0y0y64ul7fc37zt5cklumx0u6at2dcphm9mhh";
        let invoice = LxInvoice::from_str(s).unwrap();

        dbg!(&invoice);

        println!("\nroute hints:");
        for route in invoice.0.route_hints() {
            println!("  route: ({} hops)", route.0.len());
            for hop in route.0 {
                let node_pk = NodePk(hop.src_node_id);
                println!("  hop: src_node_pk: {node_pk}");
            }
        }
    }
}
