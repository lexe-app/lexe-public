use std::{fmt, num::NonZeroU64, str::FromStr};

use anyhow::Context;
use lexe_std::const_assert_mem_size;
use lightning::{
    blinded_path::IntroductionNode,
    offers::{
        offer::{self, CurrencyCode, Offer},
        parse::Bolt12ParseError,
    },
    routing::gossip::ReadOnlyNetworkGraph,
};
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use super::payments::LxOfferId;
use crate::{
    api::user::NodePk,
    ln::{amount::Amount, network::LxNetwork},
    time::TimestampMs,
};

/// A Lightning BOLT12 offer.
///
/// ## Examples
///
/// To start, we have just about the shortest possible offer: an unblinded
/// [`NodePk`], at just 64 bytes long.
///
/// ```not_rust
/// "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q"
///
/// Offer {
///     bytes: [ .. ],
///     contents: OfferContents {
///         chains: None,
///         metadata: None,
///         amount: None,
///         description: "",
///         features: [],
///         absolute_expiry: None,
///         issuer: None,
///         paths: None,
///         supported_quantity: One,
///         payee_node_pk: 024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8,
///     },
/// },
/// ```
///
/// Here's an offer with an amount, description, and "issuer" (payee name), at
/// 142 bytes long.
///
/// ```not_rust
/// "lno1pqzqzhhncq9pwargd9ejq6tnyp6xsefqv3jhxcmjd9c8g6t0dcfpyargd9ejq6tnyp6xsefqd9ehxat9wgtzzqjfqrp6zred4gydz79xakcsls72576n6r4qqdrtecut4yxssh92aq"
///
///
/// Offer {
///     bytes: [ .. ],
///     contents: OfferContents {
///         chains: None,
///         metadata: None,
///         amount: Some( Bitcoin { amount_msats: 23000000 },),
///         description: "this is the description",
///         features: [],
///         absolute_expiry: None,
///         issuer: Some("this is the issuer"),
///         paths: None,
///         supported_quantity: One,
///         payee_node_pk: 024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8,
///     },
/// },
/// ```
///
/// And that same offer but with a blinded path, now 500 bytes. Notice that the
/// `payee_node_pk` is different (blinded).
///
///
/// ```not_rust
/// "lno1qsgp3atwlvef5dfjngmladyyruuwvzqyq9008sq2za6xs6tnyp5hxgr5dpjjqer9wd3hy6tsw35k7mssesp8gcupm5mqgczgk58nxcjvs9yrg9390v8cc8jkyzq67j8x4gzkcrczfmv9cujazf9ws6jkfs9dld2ach6l9v32c9n6jkskgw5t2xp9zkuqyq4yvhz2yelft86qvnqppkt65623cs2dxmhm3mtqy2s6r5njdkcmrsqrxfs0vzt3z9635m89gqtzka8cfajtkdd3vknawyzq54hywm5ktllf7fl2ykvazfgfntp3qa7ljl0qgt2vkagzd8cpq0nctp5aqxtug2m8xhrmhd7l06vzy34vfflvrwvfyrngmfnqqyrrkfdzg229nuy2le0de6xfk7u6zgf8g6rfwvsxjueqw35x2grfwdeh2etjzcssxjqh6kmxxv3qxp9f8srkptd7xyzfjtfpz2usaxlq50vgxpm6u2n6"
///
/// Offer {
///     bytes: [ .. ],
///     contents: OfferContents {
///         chains: None,
///         metadata: Some("18f56efb329a35329a37feb4841f38e6"),
///         amount: Some( Bitcoin { amount_msats: 23000000 },),
///         description: "this is the description",
///         features: [],
///         absolute_expiry: None,
///         issuer: Some("this is the issuer"),
///         paths: Some([
///             BlindedPath {
///                 introduction_node_id: 0f6c05aae648af8120561e8c0f7b25163448814c62330fb548600436dd816374aaec6016fae2ab91d670b639dcc8846da8fcdfbf4ea19050f8fd262df2d0dc21,
///                 blinding_point: b8152518b5a843165aa967c12ab2f2f5c55db5df0a4c566ae84a125d725cd84eb0dbcb7ebe7404bfc4eefb70e34458b78566e28e28aeaabc4d344291b8506bcd,
///                 blinded_hops: [
///                     BlindedHop {
///                         blinded_node_id: 1c1bdb26271d1a2a02d68efb6ed314c45169aa970d014c06f459e967a2c465a4cc876a78e5774826a420981dc49f35d4105da4ecd08d1f1429ad8acb09a8cfd6,
///                         encrypted_payload: [ .. ],
///                     },
///                     BlindedHop {
///                         blinded_node_id: 66da680e92981beca7c46a2482e9f77dbb7b5c73b6427c19d06958783e10f069467f3c9c418f044d1599b6a1237f5d2b866e08427f0ba0abc3cc38730a7d94ea,
///                         encrypted_payload: [ .. ],
///                     },
///                 ],
///             },
///         ]),
///         supported_quantity: One,
///         payee_node_pk: 034817d5b6633220304a93c0760adbe3104992d2112b90e9be0a3d883077ae2a7a,
///     },
/// },
/// ```
#[derive(Clone, Debug, Eq, PartialEq, SerializeDisplay, DeserializeFromStr)]
pub struct LxOffer(pub Offer);

impl LxOffer {
    /// Return the [`LxOfferId`] of this offer.
    #[inline]
    pub fn id(&self) -> LxOfferId {
        LxOfferId::from(self.0.id())
    }

    /// Return the serialized offer.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }

    /// Return `true` if this offer is payable on the given [`LxNetwork`],
    /// e.g., mainnet, testnet, etc...
    pub fn supports_network(&self, network: LxNetwork) -> bool {
        self.0.supports_chain(network.genesis_chain_hash())
    }

    /// Returns the payee [`NodePk`]. May not be a real node id if the offer is
    /// blinded for recipient privacy.
    pub fn payee_node_pk(&self) -> Option<NodePk> {
        self.0.issuer_signing_pubkey().map(NodePk)
    }

    /// The absolute expiration time of the offer, if any.
    pub fn expires_at(&self) -> Option<TimestampMs> {
        self.0.absolute_expiry().map(|duration_since_epoch| {
            TimestampMs::try_from(duration_since_epoch)
                .unwrap_or(TimestampMs::MAX)
        })
    }

    /// Returns `true` if the offer has an expiration and is expired.
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.is_expired_at(TimestampMs::now())
    }

    /// Returns `true` if the offer has an expiration and its expiration is
    /// before the given timestamp.
    pub fn is_expired_at(&self, ts: TimestampMs) -> bool {
        match self.expires_at() {
            Some(expires_at) => expires_at < ts,
            None => false,
        }
    }

    /// Returns the Bitcoin-denominated [`Amount`], if any.
    pub fn amount(&self) -> Option<Amount> {
        match self.0.amount()? {
            offer::Amount::Bitcoin { amount_msats } =>
                Some(Amount::from_msat(amount_msats)),
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
            } => Some((iso4217_code, amount)),
        }
    }

    #[inline]
    pub fn expects_quantity(&self) -> bool {
        self.0.expects_quantity()
    }

    /// Returns the offer description, if any.
    pub fn description(&self) -> Option<&str> {
        self.0.description().map(|s| s.0).filter(|s| !s.is_empty())
    }

    /// Returns `true` if the input string starts with a valid bech32 hrp prefix
    /// for a BOLT12 offer.
    pub fn matches_hrp_prefix(s: &str) -> bool {
        const HRP: &[u8; 4] = b"lno1";
        const HRP_LEN: usize = HRP.len();
        match s.as_bytes().split_first_chunk::<HRP_LEN>() {
            Some((s_hrp, _)) => s_hrp.eq_ignore_ascii_case(HRP),
            _ => false,
        }
    }

    /// Some node we can route to during preflight.
    pub fn preflight_routable_node(
        &self,
        network_graph: &ReadOnlyNetworkGraph,
        lsp_node_pk: &NodePk,
    ) -> anyhow::Result<NodePk> {
        let paths = self.0.paths();
        if paths.is_empty() {
            // When there are no blinded paths, the offer MUST use a public
            // routable node as its issuer signing key.
            return self
                .0
                .issuer_signing_pubkey()
                .map(NodePk)
                .context("Offer is missing an issuer signing key");
        }

        // Look for a blinded path with a public routable node.
        for path in self.0.paths() {
            // Need to special case Lexe LSP to make smoketests pass where there
            // are no public channels.
            if let IntroductionNode::NodeId(node_id) = path.introduction_node()
            {
                if node_id == lsp_node_pk.as_inner() {
                    return Ok(NodePk(*node_id));
                }
            }

            // Look for a public node
            if let Some(node_pk) = path
                .public_introduction_node_id(network_graph)
                .and_then(|node_id| node_id.as_pubkey().ok())
            {
                return Ok(NodePk(node_pk));
            }
        }

        Err(anyhow::anyhow!("Offer is missing a public routable node"))
    }
}

const_assert_mem_size!(LxOffer, 568);

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
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Offer::from_str(s).map(LxOffer).map_err(ParseError)
    }
}

/// The max number of items that can be purchased in one payment with the offer.
///
/// The expected amount paid for this offer is `offer.amount * quantity`,
/// where `offer.amount` is the amount per item and `quantity` is the
/// number of items chosen by the payer. The payer's chosen `quantity` must be
/// in the range: `0 < quantity <= offer.max_quantity`.
///
/// NOTE: this is NOT related to reusable vs single-use offers.
///
/// This type is effectively LDK's [`Quantity`] type but serde serializable. We
/// also use `u64::MAX` to represent "unbounded" quantity. This makes the
/// `CreateOfferRequest` serialization simpler, since `max_quantity` should be
/// an optional field and `Option<Option<NonZeroU64>>` has a strange
/// serialization.
///
/// See: `CreateOfferRequest::max_quantity`.
///
/// [`Quantity`]: lightning::offers::offer::Quantity
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Debug))]
#[repr(transparent)]
pub struct MaxQuantity(pub NonZeroU64);

impl MaxQuantity {
    pub const ONE: Self = Self(const { NonZeroU64::new(1).unwrap() });
    pub const UNBOUNDED: Self = Self(NonZeroU64::MAX);
}

impl Default for MaxQuantity {
    fn default() -> Self {
        Self::ONE
    }
}

impl From<lightning::offers::offer::Quantity> for MaxQuantity {
    fn from(value: lightning::offers::offer::Quantity) -> Self {
        use lightning::offers::offer::Quantity;
        match value {
            Quantity::One => Self::ONE,
            Quantity::Unbounded => Self::UNBOUNDED,
            Quantity::Bounded(n) if n == Self::UNBOUNDED.0 => Self::UNBOUNDED,
            Quantity::Bounded(n) => MaxQuantity(n),
        }
    }
}

impl From<MaxQuantity> for lightning::offers::offer::Quantity {
    fn from(value: MaxQuantity) -> Self {
        match value {
            MaxQuantity::ONE => Self::One,
            MaxQuantity::UNBOUNDED => Self::Unbounded,
            MaxQuantity(n) => Self::Bounded(n),
        }
    }
}

// TODO(phlip9): remove when ldk upstream impls Display
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError(pub Bolt12ParseError);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let err = &self.0;
        write!(f, "Failed to parse Lightning offer: {err:?}")
    }
}

impl std::error::Error for ParseError {}

#[cfg(any(test, feature = "test-utils"))]
mod arb {
    use std::{num::NonZeroU64, time::Duration};

    use bitcoin::hashes::{Hash, Hmac};
    use lightning::{
        blinded_path::message::{
            BlindedMessagePath, MessageContext, MessageForwardNode,
            OffersContext,
        },
        ln::{channelmanager::PaymentId, inbound_payment::ExpandedKey},
        offers::{nonce::Nonce, offer::OfferBuilder},
        types::payment::PaymentHash,
    };
    use proptest::{
        arbitrary::{any, Arbitrary},
        option, prop_oneof,
        strategy::{BoxedStrategy, Just, Strategy},
    };

    use super::*;
    use crate::{
        rng::{Crng, FastRng, RngExt},
        root_seed::RootSeed,
        test_utils::arbitrary::{self, any_option_string},
    };

    fn any_offers_context() -> impl Strategy<Value = OffersContext> {
        fn any_nonce() -> impl Strategy<Value = Nonce> {
            any::<FastRng>()
                .prop_map(|mut rng| Nonce::from_entropy_source(&mut rng))
        }
        fn any_payment_id() -> impl Strategy<Value = PaymentId> {
            any::<[u8; 32]>().prop_map(PaymentId)
        }
        fn any_hmac_sha256(
        ) -> impl Strategy<Value = Hmac<bitcoin::hashes::sha256::Hash>>
        {
            any::<[u8; 32]>().prop_map(Hmac::from_byte_array)
        }
        fn any_payment_hash() -> impl Strategy<Value = PaymentHash> {
            any::<[u8; 32]>().prop_map(PaymentHash)
        }

        let any_maybe_hmac = option::of(any_hmac_sha256());

        prop_oneof![
            any_nonce()
                .prop_map(|nonce| OffersContext::InvoiceRequest { nonce }),
            (any_payment_id(), any_nonce(), any_maybe_hmac).prop_map(
                |(payment_id, nonce, hmac)| {
                    OffersContext::OutboundPayment {
                        payment_id,
                        nonce,
                        hmac,
                    }
                }
            ),
            (any_payment_hash(), any_nonce(), any_hmac_sha256()).prop_map(
                |(payment_hash, nonce, hmac)| {
                    OffersContext::InboundPayment {
                        payment_hash,
                        nonce,
                        hmac,
                    }
                }
            ),
        ]
    }

    fn any_message_context() -> impl Strategy<Value = MessageContext> {
        prop_oneof![
            any_offers_context().prop_map(MessageContext::Offers),
            any::<Vec<u8>>().prop_map(MessageContext::Custom),
        ]
    }

    fn any_message_forward_node() -> impl Strategy<Value = MessageForwardNode> {
        (arbitrary::any_secp256k1_pubkey(), option::of(any::<u64>())).prop_map(
            |(node_id, short_channel_id)| MessageForwardNode {
                node_id,
                short_channel_id,
            },
        )
    }

    fn any_path(
    ) -> impl Strategy<Value = (Vec<MessageForwardNode>, MessageContext)> {
        (
            proptest::collection::vec(any_message_forward_node(), 0..=4),
            any_message_context(),
        )
    }

    impl Arbitrary for LxOffer {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            let any_rng = any::<FastRng>();
            let any_network = any::<Option<LxNetwork>>();
            let any_is_blinded = any::<bool>();
            let any_description = arbitrary::any_option_string();
            let any_amount = any::<Option<Amount>>();
            let any_expiry = arbitrary::any_option_duration();
            let any_issuer = any_option_string();
            let any_max_quantity = any::<MaxQuantity>();
            let any_paths = proptest::collection::vec(any_path(), 0..3);

            (
                any_rng,
                any_network,
                any_is_blinded,
                any_description,
                any_amount,
                any_expiry,
                any_issuer,
                any_max_quantity,
                any_paths,
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
                        max_quantity,
                        paths,
                    )| {
                        gen_offer(
                            rng,
                            network,
                            is_blinded,
                            description,
                            amount,
                            expiry,
                            issuer,
                            max_quantity,
                            paths,
                        )
                    },
                )
                .boxed()
        }
    }

    /// Un-builder-ify the [`OfferBuilder`] API, since the extra type parameters
    /// get in the way when generating via proptest. Only used in testing.
    pub(super) fn gen_offer(
        mut rng: FastRng,
        network: Option<LxNetwork>,
        is_blinded: bool,
        description: Option<String>,
        amount: Option<Amount>,
        expiry: Option<Duration>,
        issuer: Option<String>,
        max_quantity: MaxQuantity,
        paths: Vec<(Vec<MessageForwardNode>, MessageContext)>,
    ) -> LxOffer {
        let root_seed = RootSeed::from_rng(&mut rng);
        let node_pk = root_seed.derive_node_pk(&mut rng);
        let expanded_key = ExpandedKey::new(rng.gen_bytes());
        let secp_ctx = rng.gen_secp256k1_ctx();

        let network = network.map(LxNetwork::to_bitcoin);
        let amount = amount.map(|x| x.msat());

        let paths = paths
            .into_iter()
            .map(|(intermediate_nodes, message_context)| {
                let recipient_node_id = node_pk.inner();
                BlindedMessagePath::new(
                    intermediate_nodes.as_slice(),
                    recipient_node_id,
                    message_context,
                    &mut rng,
                    &secp_ctx,
                )
                .unwrap()
            })
            .collect::<Vec<_>>();

        // each builder constructor returns a different type, hence the copying
        let offer = if is_blinded {
            let nonce = Nonce::from_entropy_source(&mut rng);
            let mut offer = OfferBuilder::deriving_signing_pubkey(
                node_pk.inner(),
                &expanded_key,
                nonce,
                &secp_ctx,
            )
            .supported_quantity(max_quantity.into());
            if let Some(network) = network {
                offer = offer.chain(network);
            }
            if let Some(amount) = amount {
                offer = offer.amount_msats(amount);
            }
            if let Some(expiry) = expiry {
                offer = offer.absolute_expiry(expiry);
            }
            if let Some(description) = description {
                offer = offer.description(description);
            }
            if let Some(issuer) = issuer {
                offer = offer.issuer(issuer);
            }
            for path in paths {
                offer = offer.path(path);
            }
            offer.build()
        } else {
            let mut offer = OfferBuilder::new(node_pk.inner())
                .supported_quantity(max_quantity.into());
            if let Some(network) = network {
                offer = offer.chain(network);
            }
            if let Some(amount) = amount {
                offer = offer.amount_msats(amount);
            }
            if let Some(expiry) = expiry {
                offer = offer.absolute_expiry(expiry);
            }
            if let Some(description) = description {
                offer = offer.description(description);
            }
            if let Some(issuer) = issuer {
                offer = offer.issuer(issuer);
            }
            for path in paths {
                offer = offer.path(path);
            }
            offer.build()
        };

        LxOffer(offer.expect("Failed to build BOLT12 offer"))
    }

    impl Arbitrary for MaxQuantity {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            // prefer generating `MaxQuantity::ONE` and to a lesser extent,
            // `MaxQuantity::UNBOUNDED`, since these are by far the most common.
            prop_oneof![
                10 => Just(MaxQuantity::ONE),
                2 => Just(MaxQuantity::UNBOUNDED),
                1 => any::<NonZeroU64>().prop_map(MaxQuantity),
            ]
            .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use lightning::{
        blinded_path::message::{
            MessageContext, MessageForwardNode, OffersContext,
        },
        offers::nonce::Nonce,
    };
    use proptest::{arbitrary::any, prop_assert, prop_assert_eq, proptest};
    use test::arb::gen_offer;

    use super::*;
    use crate::{
        rng::FastRng,
        test_utils::{arbitrary, roundtrip},
    };

    #[test]
    fn offer_parse_examples() {
        #[track_caller]
        fn parse_ok(s: &str) -> LxOffer {
            let offer = LxOffer::from_str(s).unwrap();
            // Also check that it roundtrips.
            assert_eq!(offer.to_string(), s);
            offer
        }

        // basically the smallest possible offer (just a node pubkey)
        let o = parse_ok(
            "lno1pgqpvggzfyqv8gg09k4q35tc5mkmzr7re2nm20gw5qp5d08r3w5s6zzu4t5q",
        );
        assert_eq!(
            o.payee_node_pk().unwrap(),
            NodePk::from_str("024900c3a10f2daa08d178a6edb10fc3caa7b53d0ea00346bce38ba90d085caae8").unwrap(),
        );
        assert!(o.supports_network(LxNetwork::Mainnet));
        assert_eq!(o.amount(), None);
        assert_eq!(o.fiat_amount(), None);
        assert_eq!(o.description(), None);

        let o = parse_ok("lno1pg257enxv4ezqcneype82um50ynhxgrwdajx293pqglnyxw6q0hzngfdusg8umzuxe8kquuz7pjl90ldj8wadwgs0xlmc");
        assert!(o.supports_network(LxNetwork::Mainnet));
        assert_eq!(o.amount(), None);
        assert_eq!(o.fiat_amount(), None);
        assert_eq!(o.description(), Some("Offer by rusty's node"));

        parse_ok("lno1qgsyxjtl6luzd9t3pr62xr7eemp6awnejusgf6gw45q75vcfqqqqqqq2p32x2um5ypmx2cm5dae8x93pqthvwfzadd7jejes8q9lhc4rvjxd022zv5l44g6qah82ru5rdpnpj");
        parse_ok("lno1pqqnyzsmx5cx6umpwssx6atvw35j6ut4v9h8g6t50ysx7enxv4epyrmjw4ehgcm0wfczucm0d5hxzag5qqtzzq3lxgva5qlw9xsjmeqs0ek9cdj0vpec9ur972l7mywa66u3q7dlhs");
        parse_ok("lno1qsgqqqqqqqqqqqqqqqqqqqqqqqqqqzsv23jhxapqwejkxar0wfe3vggzamrjghtt05kvkvpcp0a79gmy3nt6jsn98ad2xs8de6sl9qmgvcvs");
        parse_ok("lno1pqpzwyq2p32x2um5ypmx2cm5dae8x93pqthvwfzadd7jejes8q9lhc4rvjxd022zv5l44g6qah82ru5rdpnpj");
    }

    #[test]
    fn offer_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<LxOffer>();
    }

    #[test]
    fn offer_fromstr_display_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxOffer>();
    }

    #[test]
    fn offer_matches_hrp_prefix() {
        proptest!(|(offer: LxOffer)| {
            let mut offer_str = offer.to_string();
            prop_assert!(LxOffer::matches_hrp_prefix(&offer_str));

            // uppercase
            offer_str.make_ascii_uppercase();
            prop_assert!(LxOffer::matches_hrp_prefix(&offer_str));
            prop_assert_eq!(LxOffer::from_str(&offer_str).unwrap(), offer);
        });
    }

    #[test]
    fn offer_max_quantity_serde_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<MaxQuantity>();
    }

    #[test]
    fn offer_max_quantity_ldk_roundtrip() {
        proptest!(|(max_quantity1: MaxQuantity)| {
            let ldk_quantity1 = lightning::offers::offer::Quantity::from(max_quantity1);
            let max_quantity2 = MaxQuantity::from(ldk_quantity1);
            let ldk_quantity2 = lightning::offers::offer::Quantity::from(max_quantity2);
            prop_assert_eq!(ldk_quantity1, ldk_quantity2);
            prop_assert_eq!(max_quantity1, max_quantity2);
        });
    }

    // Generate example offers using the proptest strategy.
    #[ignore]
    #[test]
    fn offer_sample_data() {
        let mut rng = FastRng::from_u64(949846484986610);
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
        let mut rng = FastRng::from_u64(123);

        // false => use node_pk to sign offer (less privacy)
        // true => derive a signing keypair per offer (add ~50 B per offer).
        let is_blinded = true;
        let network = None; // None ==> BTC mainnet
        let description = Some("this is the description".to_owned());
        let amount = Some(Amount::from_sats_u32(23_000));
        // duration since Unix epoch
        let expiry = None;
        let issuer = Some("this is the issuer".to_owned());
        let max_quantity = MaxQuantity::ONE;
        let message_context =
            MessageContext::Offers(OffersContext::InvoiceRequest {
                nonce: Nonce::from_entropy_source(&mut rng),
            });
        let intermediate_nodes = vec![
            MessageForwardNode {
                node_id: arbitrary::gen_value(&mut rng, any::<NodePk>())
                    .inner(),
                short_channel_id: None,
            },
            MessageForwardNode {
                node_id: arbitrary::gen_value(&mut rng, any::<NodePk>())
                    .inner(),
                short_channel_id: None,
            },
        ];
        let paths = vec![(intermediate_nodes, message_context)];

        let offer = gen_offer(
            rng,
            network,
            is_blinded,
            description,
            amount,
            expiry,
            issuer,
            max_quantity,
            paths,
        );
        let offer_str = offer.to_string();
        let offer_len = offer_str.len();
        let offer_metadata_hex = offer.0.metadata().map(|x| hex::encode(x));
        let node_pk = NodePk(offer.0.issuer_signing_pubkey().unwrap());

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

    /// ```bash
    /// $ cargo test -p common --lib -- offer_decode --ignored --nocapture
    /// ```
    #[ignore]
    #[test]
    fn offer_decode() {
        let offer_str =
            "lno1zrxq8pjw7qjlm68mtp7e3yvxee4y5xrgjhhyf2fxhlphpckrvevh50u0qdp2nyl5lh362fu4r6ycw59tul97ptq57j9mhusk4dyqed0nytnzyqsz0qduahca4eryls267a72a4rtcnk4p6ululyvg7a7pdczg8ha8e6qqval7cremj65ut2k087xdhay6qvv0dtljppyd80zyj68f748jt569nutyznpf9qms39a06ecl0tw9w6ky9xpqd4k7hl4phttq9lkdrhjffv08tc04yxf4pfexypwt0e8zlmdeuf4qqqsdt4qevd84nlmks62nzzz9swwpu";
        let offer = LxOffer::from_str(offer_str).unwrap();
        dbg!(&offer);
        dbg!(offer.payee_node_pk());
    }
}
