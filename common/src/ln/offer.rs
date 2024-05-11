use std::{fmt, str::FromStr};

use lightning::offers::{
    offer::{self, CurrencyCode, Offer},
    parse::Bolt12ParseError,
};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::{api::NodePk, cli::Network, ln::amount::Amount};

/// A Lightning BOLT12 offer.
#[derive(Clone, Debug, SerializeDisplay, DeserializeFromStr)]
pub struct LxOffer(pub Offer);

impl LxOffer {
    /// Return the serialized offer.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    /// Return `true` if this offer is payable on the given [`Network`], e.g.,
    /// mainnet, testnet, etc...
    pub fn supports_network(&self, network: Network) -> bool {
        self.0.supports_chain(network.genesis_chain_hash())
    }

    /// Returns the payee [`NodePk`]. May not be a real node id if the offer is
    /// blinded for recipient privacy.
    pub fn payee_node_pk(&self) -> NodePk {
        NodePk(self.0.signing_pubkey())
    }

    /// Returns the Bitcoin-denominated [`Amount`], if any.
    pub fn amount(&self) -> Option<Amount> {
        match self.0.amount()? {
            offer::Amount::Bitcoin { amount_msats } =>
                Some(Amount::from_msat(*amount_msats)),
            offer::Amount::Currency { .. } => None,
        }
    }

    /// Returns the fiat-denominated amount, if any. Returns the fiat ISO4217
    /// currency code along with the ISO4217 exponent amount (e.g., USD cents).
    // TODO(phlip9): needs a new type
    pub fn fiat_amount(&self) -> Option<(CurrencyCode, u64)> {
        match self.0.amount()? {
            offer::Amount::Bitcoin { .. } => None,
            offer::Amount::Currency {
                iso4217_code,
                amount,
            } => Some((*iso4217_code, *amount)),
        }
    }

    /// Returns the offer description, if any.
    pub fn description(&self) -> Option<&str> {
        // TODO(phlip9): bolt spec master now allows no description; reflect
        // that here after ldk updates.
        let d = self.0.description().0;
        if d.is_empty() {
            None
        } else {
            Some(d)
        }
    }
}

impl From<Offer> for LxOffer {
    #[inline]
    fn from(value: Offer) -> Self {
        LxOffer(value)
    }
}

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
        let self_bytes: &[u8] = self.as_bytes();
        let other_bytes: &[u8] = other.as_bytes();
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
        sign::KeyMaterial,
    };
    use proptest::{
        arbitrary::{any, Arbitrary},
        option, prop_oneof,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;
    use crate::{
        rng::{Crng, RngExt, WeakRng},
        root_seed::RootSeed,
        test_utils::arbitrary::{self, any_option_string},
    };

    impl Arbitrary for LxOffer {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let rng = any::<WeakRng>();
            let network = any::<Option<Network>>();
            let is_blinded = any::<bool>();
            let description = arbitrary::any_option_string();
            let amount = any::<Option<Amount>>();
            let expiry = arbitrary::any_option_duration();
            let issuer = any_option_string();
            let quantity = option::of(prop_oneof![
                any::<NonZeroU64>().prop_map(Quantity::Bounded),
                Just(Quantity::Unbounded),
                Just(Quantity::One),
            ]);
            // TODO(phlip9): technically there could be more than one path...
            let path_len = 0_usize..4;

            (
                rng,
                network,
                is_blinded,
                description,
                amount,
                expiry,
                issuer,
                quantity,
                path_len,
            )
                .prop_map(
                    |(
                        rng,
                        network,
                        is_blinded,
                        description,
                        amount,
                        expiry,
                        issuer,
                        quantity,
                        path_len,
                    )| {
                        gen_offer(
                            rng,
                            network,
                            is_blinded,
                            description,
                            amount,
                            expiry,
                            issuer,
                            quantity,
                            path_len,
                        )
                    },
                )
                .boxed()
        }
    }

    /// Un-builder-ify the [`OfferBuilder`] API, since the extra type parameters
    /// get in the way when generating via proptest. Only used in testing.
    pub(super) fn gen_offer(
        mut rng: WeakRng,
        network: Option<Network>,
        is_blinded: bool,
        description: Option<String>,
        amount: Option<Amount>,
        expiry: Option<Duration>,
        issuer: Option<String>,
        quantity: Option<Quantity>,
        // NOTE: len <= 1 will not set a path
        path_len: usize,
    ) -> LxOffer {
        let root_seed = RootSeed::from_rng(&mut rng);
        let node_pk = root_seed.derive_node_pk(&mut rng);
        let expanded_key_material = KeyMaterial(rng.gen_bytes());
        let expanded_key = ExpandedKey::new(&expanded_key_material);
        let secp_ctx = rng.gen_secp256k1_ctx();

        let network = network.map(Network::to_inner);
        let amount = amount.map(|x| x.msat());
        let path = if path_len > 2 {
            let mut node_pks = Vec::new();
            for _ in 0..path_len {
                node_pks.push(
                    RootSeed::from_rng(&mut rng)
                        .derive_node_pk(&mut rng)
                        .inner(),
                );
            }
            let path = BlindedPath::new_for_message(&node_pks, &rng, &secp_ctx);
            Some(path.unwrap())
        } else {
            None
        };

        // TODO(phlip9): bolt spec master now allows no description; reflect
        // that here after ldk updates.
        let description_str = description.unwrap_or_default();

        // each builder constructor returns a different type, hence the copying
        let offer = if is_blinded {
            let mut offer = OfferBuilder::deriving_signing_pubkey(
                description_str,
                node_pk.inner(),
                &expanded_key,
                &mut rng,
                &secp_ctx,
            );
            if let Some(network) = network {
                offer = offer.chain(network);
            }
            if let Some(amount) = amount {
                offer = offer.amount_msats(amount);
            }
            if let Some(expiry) = expiry {
                offer = offer.absolute_expiry(expiry);
            }
            if let Some(issuer) = issuer {
                offer = offer.issuer(issuer);
            }
            if let Some(quantity) = quantity {
                offer = offer.supported_quantity(quantity);
            }
            if let Some(path) = path {
                offer = offer.path(path);
            }
            offer.build()
        } else {
            let mut offer = OfferBuilder::new(description_str, node_pk.inner());
            if let Some(network) = network {
                offer = offer.chain(network);
            }
            if let Some(amount) = amount {
                offer = offer.amount_msats(amount);
            }
            if let Some(expiry) = expiry {
                offer = offer.absolute_expiry(expiry);
            }
            if let Some(issuer) = issuer {
                offer = offer.issuer(issuer);
            }
            if let Some(quantity) = quantity {
                offer = offer.supported_quantity(quantity);
            }
            if let Some(path) = path {
                offer = offer.path(path);
            }
            offer.build()
        };

        LxOffer(offer.expect("Failed to build BOLT12 offer"))
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use test::arb::gen_offer;

    use super::*;
    use crate::{
        hex,
        rng::WeakRng,
        test_utils::{arbitrary, roundtrip},
    };

    #[test]
    fn offer_parse_examples() {
        // basically the smallest possible offer (just a node pubkey)
        let o = LxOffer::from_str(
            "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q",
        )
        .unwrap();
        assert_eq!(
            o.payee_node_pk(),
            NodePk::from_str("024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8").unwrap(),
        );
        assert!(o.supports_network(Network::MAINNET));
        assert_eq!(o.amount(), None);
        assert_eq!(o.fiat_amount(), None);
        assert_eq!(o.description(), None);

        let o = LxOffer::from_str("lno1pg257enxv4ezqcneype82um50ynhxgrwdajx293pqglnyxw6q0hzngfdusg8umzuxe8kquuz7pjl90ldj8wadwgs0xlmc").unwrap();
        assert!(o.supports_network(Network::MAINNET));
        assert_eq!(o.amount(), None);
        assert_eq!(o.fiat_amount(), None);
        assert_eq!(o.description(), Some("Offer by rusty's node"));

        Offer::from_str("lno1qgsyxjtl6luzd9t3pr62xr7eemp6awnejusgf6gw45q75vcfqqqqqqq2p32x2um5ypmx2cm5dae8x93pqthvwfzadd7jejes8q9lhc4rvjxd022zv5l44g6qah82ru5rdpnpj").unwrap();
        Offer::from_str("lno1pqqnyzsmx5cx6umpwssx6atvw35j6ut4v9h8g6t50ysx7enxv4epyrmjw4ehgcm0wfczucm0d5hxzag5qqtzzq3lxgva5qlw9xsjmeqs0ek9cdj0vpec9ur972l7mywa66u3q7dlhs").unwrap();
        Offer::from_str("lno1qsgqqqqqqqqqqqqqqqqqqqqqqqqqqzsv23jhxapqwejkxar0wfe3vggzamrjghtt05kvkvpcp0a79gmy3nt6jsn98ad2xs8de6sl9qmgvcvs").unwrap();
        Offer::from_str("lno1pqpzwyq2p32x2um5ypmx2cm5dae8x93pqthvwfzadd7jejes8q9lhc4rvjxd022zv5l44g6qah82ru5rdpnpj").unwrap();
    }

    #[test]
    fn offer_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxOffer>();
    }

    #[test]
    fn offer_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxOffer>();
    }

    // Generate example offers using the proptest strategy.
    #[ignore]
    #[test]
    fn offer_sample_data() {
        let mut rng = WeakRng::from_u64(949846484986610);
        let strategy = any::<LxOffer>();
        let value_iter = arbitrary::gen_value_iter(&mut rng, strategy);

        for value in value_iter.take(10) {
            let value_str = value.to_string();
            dbg!(value);
            dbg!(value_str);
        }
    }

    // Generate example offers with specific values.
    #[ignore]
    #[test]
    fn offer_dump() {
        let rng = WeakRng::from_u64(123);

        // false => use node_pk to sign offer (less privacy)
        // true => derive a signing keypair per offer (add ~50 B per offer).
        let is_blinded = false;
        let network = None; // None ==> BTC mainnet
        let description = None;
        let amount = None;
        // duration since Unix epoch
        let expiry = None;
        let issuer = None;
        let quantity = None;
        let path_len = 0;

        let offer = gen_offer(
            rng,
            network,
            is_blinded,
            description,
            amount,
            expiry,
            issuer,
            quantity,
            path_len,
        );
        let offer_str = offer.to_string();
        let offer_len = offer_str.len();
        let offer_metadata_hex = offer.0.metadata().map(|x| hex::encode(x));
        let node_pk = NodePk(offer.0.signing_pubkey());

        println!("---");
        dbg!(offer);
        println!("---");
        dbg!(offer_str);
        dbg!(offer_len);
        println!("---");
        dbg!(node_pk);
        dbg!(offer_metadata_hex);
        println!("---");
    }
}
