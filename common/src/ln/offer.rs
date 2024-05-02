use std::{fmt, str::FromStr};

use lightning::offers::{offer::Offer, parse::Bolt12ParseError};
use serde_with::{DeserializeFromStr, SerializeDisplay};

/// A Lightning BOLT12 offer.
#[derive(Clone, Debug, SerializeDisplay, DeserializeFromStr)]
pub struct LxOffer(pub Offer);

impl fmt::Display for LxOffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for LxOffer {
    type Err = LxBolt12ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Offer::from_str(s).map(LxOffer).map_err(LxBolt12ParseError)
    }
}

// TODO(phlip9): ldk master has Eq/PartialEq impl'd. remove after we update.
impl PartialEq for LxOffer {
    fn eq(&self, other: &Self) -> bool {
        let self_bytes: &[u8] = self.0.as_ref();
        let other_bytes: &[u8] = other.0.as_ref();
        self_bytes == other_bytes
    }
}
impl Eq for LxOffer {}

// TODO(phlip9): remove when ldk upstream impls Display
#[derive(Clone, Debug, PartialEq)]
pub struct LxBolt12ParseError(Bolt12ParseError);

impl fmt::Display for LxBolt12ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to parse BOLT12 offer: {:?}", &self.0)
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arb {
    use std::{num::NonZeroU64, time::Duration};

    use lightning::{
        blinded_path::BlindedPath,
        ln::inbound_payment::ExpandedKey,
        offers::offer::{OfferBuilder, Quantity},
    };
    use proptest::{
        arbitrary::{any, Arbitrary},
        option, prop_oneof,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;
    use crate::{
        cli::Network,
        ln::amount::Amount,
        rng::{RngExt, WeakRng},
        root_seed::RootSeed,
        test_utils::arbitrary::{self, any_option_string},
    };

    impl Arbitrary for LxOffer {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let rng = any::<WeakRng>();
            let description = arbitrary::any_string();
            let chain = any::<Option<Network>>();
            let amount = any::<Option<Amount>>();
            let expiry = any::<Option<u64>>();
            let issuer = any_option_string();
            let quantity = option::of(prop_oneof![
                any::<NonZeroU64>().prop_map(Quantity::Bounded),
                Just(Quantity::Unbounded),
                Just(Quantity::One),
            ]);
            // TODO(phlip9): technically there could be more than one path...
            // NOTE: len = 1 will not set a path, since you need at least 2 hops
            let path_len = 0_usize..5;

            (
                rng,
                description,
                chain,
                amount,
                expiry,
                issuer,
                quantity,
                path_len,
            )
                .prop_map(
                    |(
                        mut rng,
                        description,
                        chain,
                        amount,
                        expiry,
                        issuer,
                        quantity,
                        path_len,
                    )| {
                        let root_seed = RootSeed::from_rng(&mut rng);
                        let node_pk = root_seed.derive_node_pk(&mut rng);
                        let expanded_key_material =
                            lightning::sign::KeyMaterial(rng.gen_bytes());
                        let expanded_key =
                            ExpandedKey::new(&expanded_key_material);
                        let secp_ctx =
                            crate::rng::get_randomized_secp256k1_ctx(&mut rng);

                        let mut offer = OfferBuilder::deriving_signing_pubkey(
                            description,
                            node_pk.inner(),
                            &expanded_key,
                            &mut rng,
                            &secp_ctx,
                        );

                        if let Some(chain) = chain {
                            offer = offer.chain(chain.to_inner());
                        }
                        if let Some(amount) = amount {
                            offer = offer.amount_msats(amount.msat());
                        }
                        if let Some(expiry) = expiry {
                            offer = offer
                                .absolute_expiry(Duration::from_secs(expiry));
                        }
                        if let Some(issuer) = issuer {
                            offer = offer.issuer(issuer);
                        }
                        if let Some(quantity) = quantity {
                            offer = offer.supported_quantity(quantity);
                        }
                        if path_len > 2 {
                            let mut node_pks = Vec::new();
                            for _ in 0..path_len {
                                node_pks.push(
                                    RootSeed::from_rng(&mut rng)
                                        .derive_node_pk(&mut rng)
                                        .inner(),
                                );
                            }
                            offer = offer.path(
                                BlindedPath::new_for_message(
                                    &node_pks, &rng, &secp_ctx,
                                )
                                .unwrap(),
                            );
                        }

                        offer
                            .build()
                            .map(LxOffer)
                            .expect("Failed to build BOLT12 offer")
                    },
                )
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::{
        arbitrary::any,
        strategy::Strategy,
        test_runner::{Config, RngAlgorithm, TestRng, TestRunner},
    };

    use super::*;
    use crate::{
        rng::{RngExt, WeakRng},
        test_utils::roundtrip,
    };

    #[test]
    fn offer_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxOffer>();
    }

    #[test]
    fn offer_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxOffer>();
    }

    #[ignore]
    #[test]
    fn offer_sample_data() {
        let mut rng = WeakRng::from_u64(949846484986610);
        let seed = rng.gen_bytes::<32>();
        let test_rng = TestRng::from_seed(RngAlgorithm::ChaCha, &seed);
        let config = Config::default();
        let mut test_runner = TestRunner::new_with_rng(config, test_rng);

        let offer_strategy = any::<LxOffer>();

        let value = {
            let mut value_tree =
                offer_strategy.new_tree(&mut test_runner).unwrap();
            for _ in 0..128 {
                if !value_tree.simplify() {
                    break;
                }
            }
            value_tree.current()
        };
        let value_str = value.to_string();

        dbg!(value);
        dbg!(value_str);
    }
}
