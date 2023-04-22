use std::{
    fmt::{self, Display},
    str::FromStr,
};

use lightning_invoice::Invoice;
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// Wraps [`lightning_invoice::Invoice`] to impl [`serde`] Serialize /
/// Deserialize using the LDK's [`FromStr`] / [`Display`] impls.
#[derive(Clone, Debug, Eq, PartialEq, SerializeDisplay, DeserializeFromStr)]
pub struct LxInvoice(pub Invoice);

impl FromStr for LxInvoice {
    type Err = lightning_invoice::ParseOrSemanticError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Invoice::from_str(s).map(Self)
    }
}

impl Display for LxInvoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use std::time::{Duration, UNIX_EPOCH};

    use bitcoin::{
        hashes::{sha256, Hash},
        secp256k1::{self, Secp256k1},
    };
    use lightning::ln::PaymentSecret;
    use lightning_invoice::{Currency, InvoiceBuilder, MAX_TIMESTAMP};
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::{
        cli::Network, rng::WeakRng, root_seed::RootSeed, test_utils::arbitrary,
    };

    impl Arbitrary for LxInvoice {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let currency = any::<Network>().prop_map(Currency::from);
            let description = arbitrary::any_string();
            let payment_hash = any::<[u8; 32]>()
                .prop_map(|buf| sha256::Hash::from_slice(&buf).unwrap());
            let payment_secret = any::<[u8; 32]>().prop_map(PaymentSecret);
            let timestamp = (0..MAX_TIMESTAMP)
                .prop_map(Duration::from_secs)
                .prop_map(|duration| UNIX_EPOCH + duration);
            let min_final_cltv_expiry_delta = any::<u64>();
            let secret_key = any::<WeakRng>()
                .prop_map(|mut rng| {
                    RootSeed::from_rng(&mut rng).derive_node_key_pair(&mut rng)
                })
                .prop_map(secp256k1::SecretKey::from);

            (
                currency,
                description,
                payment_hash,
                payment_secret,
                timestamp,
                min_final_cltv_expiry_delta,
                secret_key,
            )
                .prop_map(
                    |(
                        currency,
                        description,
                        payment_hash,
                        payment_secret,
                        timestamp,
                        min_final_cltv_expiry_delta,
                        secret_key,
                    )| {
                        let invoice = InvoiceBuilder::new(currency)
                            .description(description)
                            .payment_hash(payment_hash)
                            .payment_secret(payment_secret)
                            .timestamp(timestamp)
                            .min_final_cltv_expiry_delta(
                                min_final_cltv_expiry_delta,
                            )
                            .build_signed(|hash| {
                                Secp256k1::new()
                                    .sign_ecdsa_recoverable(hash, &secret_key)
                            })
                            .expect("Could not build invoice");
                        Self(invoice)
                    },
                )
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn invoice_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxInvoice>();
    }

    #[test]
    fn invoice_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxInvoice>();
    }
}
