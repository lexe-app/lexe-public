use std::{
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::Context;
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescription};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{
    api::user::NodePk,
    ln::{
        amount::Amount,
        network::LxNetwork,
        payments::{LxPaymentHash, LxPaymentId, LxPaymentSecret},
    },
    time::{self, TimestampMs},
    Apply,
};

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
            Bolt11InvoiceDescription::Direct(description)
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

    #[inline]
    pub fn is_expired(&self) -> bool {
        self.0.is_expired()
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
}

impl FromStr for LxInvoice {
    type Err = lightning_invoice::ParseOrSemanticError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Bolt11Invoice::from_str(s).map(Self)
    }
}

impl Display for LxInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use std::time::Duration;

    use bitcoin::{
        hashes::{sha256, Hash},
        secp256k1::{self, Message},
    };
    use lightning::{ln::PaymentSecret, routing::router::RouteHint};
    use lightning_invoice::{Fallback, InvoiceBuilder, MAX_TIMESTAMP};
    use proptest::{
        arbitrary::{any, Arbitrary},
        option, result,
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::{
        rng::{Crng, FastRng},
        root_seed::RootSeed,
        test_utils::arbitrary,
    };

    impl Arbitrary for LxInvoice {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let bytes32 = any::<[u8; 32]>().no_shrink();

            let node_key_pair = any::<FastRng>().prop_map(|mut rng| {
                RootSeed::from_rng(&mut rng).derive_node_key_pair(&mut rng)
            });
            let network = any::<LxNetwork>();
            let description_or_hash =
                result::maybe_ok(arbitrary::any_string(), bytes32);
            let timestamp = (0..MAX_TIMESTAMP).prop_map(Duration::from_secs);

            let payment_secret = bytes32;
            let payment_hash = bytes32;
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

    use lightning::{
        ln::channelmanager::MIN_FINAL_CLTV_EXPIRY_DELTA,
        routing::router::RouteHint,
    };
    use proptest::arbitrary::any;
    use test::arbitrary_impl::gen_invoice;

    use super::*;
    use crate::{
        rng::FastRng,
        root_seed::RootSeed,
        test_utils::{arbitrary, roundtrip},
    };

    #[test]
    fn invoice_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxInvoice>();
    }

    #[test]
    fn invoice_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxInvoice>();
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
            payment_secret.into_inner(),
            payment_hash.into_inner(),
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
        let s = "lnbcrt280u1pnxywwgdqqpp52t2fd5p8kuqn370uae3f3vezj6mjlzsuynfgkd9533xqp3vyd44scqpcsp5truuwxdmk38t9zad3al685uw6a4yg0gncg8p8yzy69asy7rz3uyq9qyysgqxqrrssnp4qfjfnyxh2n3yh2d9fqt293lfahnzfllg4qj2cu9lz04e97u2njx6vrzjqdd8p4z7a3l0kfcrr8c3d2tggfg2ed809q4zd5scwjrculzs3rmnkqqqqyqqrasqq5qqqqqqqqqqhwqqfqkqddwf80knvfd5naznztzzfm9glx7v8lhchjljjxnhknre9rwd6y3qcjn92ewl9dquc60jxhh8e0d6pd9ejsskutyr6rp6xpc0ex36spnalh5l";
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
