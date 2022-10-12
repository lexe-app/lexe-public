use std::fmt::{self, Display};
use std::str::FromStr;

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

// `any::<String>()` requires proptest feature std which doesn't work in SGX
#[cfg(all(test, not(target_env = "sgx")))]
mod test {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use bitcoin::hashes::{sha256, Hash};
    use bitcoin::secp256k1::{self, Secp256k1};
    use lightning::ln::PaymentSecret;
    use lightning_invoice::{Currency, InvoiceBuilder};
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;
    use crate::cli::Network;
    use crate::rng::SmallRng;
    use crate::root_seed::RootSeed;
    use crate::test_utils::roundtrip;

    impl Arbitrary for LxInvoice {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let currency = any::<Network>().prop_map(Currency::from);
            let description = any::<String>();

            let payment_hash = any::<[u8; 32]>()
                .prop_map(|buf| sha256::Hash::from_slice(&buf).unwrap());

            let payment_secret = any::<[u8; 32]>().prop_map(PaymentSecret);

            let timestamp = any::<SystemTime>().prop_map(|system_time| {
                // TODO: We convert to and from unix seconds because LDK's
                // fromstr/display impl fails the roundtrip test if the
                // SystemTime passed to InvoiceBuilder::timestamp isn't rounded
                // to the nearest second. We can drop the prop_map once
                // <https://github.com/lightningdevkit/rust-lightning/pull/1760>
                // is merged and released.
                let unix_secs = system_time
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::from_secs(0))
                    .as_secs();
                UNIX_EPOCH + Duration::from_secs(unix_secs)
            });

            let min_final_cltv_expiry = any::<u64>();

            let secret_key = any::<SmallRng>()
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
                min_final_cltv_expiry,
                secret_key,
            )
                .prop_map(
                    |(
                        currency,
                        description,
                        payment_hash,
                        payment_secret,
                        timestamp,
                        min_final_cltv_expiry,
                        secret_key,
                    )| {
                        let invoice = InvoiceBuilder::new(currency)
                            .description(description)
                            .payment_hash(payment_hash)
                            .payment_secret(payment_secret)
                            .timestamp(timestamp)
                            .min_final_cltv_expiry(min_final_cltv_expiry)
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

    #[test]
    fn invoice_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxInvoice>();
    }

    #[test]
    fn invoice_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxInvoice>();
    }
}
