//! Continuations that Lexe returns to the user, which may be passed back to
//! other endpoints.
//!
//! These continuations should be cryptographically resistant to tampering.
//! See [`lexe_common::root_seed::RootSeed::derive_continuation_mac_key`].

use anyhow::anyhow;
use lexe_common::{api::user::UserPk, ln::amount::Amount, ppm::Ppm};
use lexe_crypto::hmac;
use lexe_serde::base64_or_bytes;
use lexe_std::array;
use lightning::{
    routing::router::Route,
    util::ser::{Readable, Writeable},
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::types::payments::PaymentHash;

/// An LDK [`Route`] continuation, which may have been received from an
/// untrusted source. The inner bytes are unverified until
/// [`Self::as_route_and_validate`].
///
/// AADs: See [`LdkRouteAad`].
///
/// Producers: [`PayInvoicePreflightResponse`]
/// Consumers: [`PayInvoiceRequest`]
///
/// Note that compat is a negligible issue because we expect
/// `LdkRouteContinuation` to be short lived and therefore created by and
/// returned to the same node version.
///
/// [`PayInvoicePreflightResponse`]: crate::models::command::PayInvoicePreflightResponse
/// [`PayInvoiceRequest`]: crate::models::command::PayInvoiceRequest
#[derive(Clone, Serialize, Deserialize)]
pub struct LdkRouteContinuation(#[serde(with = "base64_or_bytes")] pub Vec<u8>);

/// The values carried by [`LdkRouteContinuation`].
pub struct LdkRouteValues {
    pub route: Route,
    /// The fee added to the [`Route`]'s first hop, which the payment must
    /// reproduce so that it charges exactly what preflight quoted.
    pub first_hop_fee: Amount,
}

/// The values an [`LdkRouteContinuation`] is bound to:
/// - the fallback amount for amountless invoices
/// - the invoice's payment hash
/// - the partner fee params added to the first hop in the [`Route`]
//
// Amounts are msat `u64`s, since `Amount` serialization is not injective:
// see `test::equal_amounts_can_serialize_differently`.
#[cfg_attr(test, derive(Arbitrary))]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LdkRouteAad {
    pub fallback_amount_msat: Option<u64>,
    pub payment_hash: PaymentHash,
    pub partner_pk: Option<UserPk>,
    pub partner_prop_fee: Option<Ppm>,
    pub partner_base_fee_msat: Option<u64>,
}

impl LdkRouteContinuation {
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::LdkRouteContinuation");

    /// Byte length of the millisatoshi `first_hop_fee` prefix in the
    /// plaintext.
    const FEES_LEN: usize = size_of::<u64>();

    /// Serialize a [`Route`] and its first-hop fee and sign them, for use as
    /// an [`LdkRouteContinuation`].
    pub fn from_route_and_sign(
        route: &Route,
        first_hop_fee: Amount,
        mac_key: &hmac::Key,
        aad: &LdkRouteAad,
    ) -> Self {
        let aad = aad.to_bytes();
        let write_data_cb = |out: &mut Vec<u8>| {
            let first_hop_fee_msat: u64 = first_hop_fee.msat();
            out.extend_from_slice(&first_hop_fee_msat.to_le_bytes());
            route.write(out).expect("Vec::write is infallible")
        };
        let signed = mac_key.sign_and_append(
            &Self::DOMAIN_SEPARATOR,
            &[aad.as_slice()],
            Some(Self::FEES_LEN + route.serialized_length()),
            &write_data_cb,
        );
        Self(signed)
    }

    /// Verify the authenticity of an [`LdkRouteContinuation`] and extract the
    /// values it carries.
    pub fn as_route_and_validate(
        &self,
        mac_key: &hmac::Key,
        aad: &LdkRouteAad,
    ) -> anyhow::Result<LdkRouteValues> {
        let aad = aad.to_bytes();
        let bytes = mac_key
            .verify(&Self::DOMAIN_SEPARATOR, &[aad.as_slice()], &self.0)
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        let (first_hop_fee_msat, route_bytes) = bytes
            .split_first_chunk::<{ Self::FEES_LEN }>()
            .ok_or_else(|| anyhow!("Invalid `ldk_route`."))?;
        let first_hop_fee =
            Amount::from_msat(u64::from_le_bytes(*first_hop_fee_msat));
        let route = Route::read(&mut &route_bytes[..])
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        Ok(LdkRouteValues {
            route,
            first_hop_fee,
        })
    }
}

impl LdkRouteAad {
    /// Canonically serializes the AAD with [`bcs`].
    fn to_bytes(self) -> Vec<u8> {
        bcs::to_bytes(&self).expect("Serializing the AAD should never fail")
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use bitcoin::secp256k1::PublicKey;
    use lexe_common::test_utils::roundtrip;
    use lightning::{
        routing::router::{Path, RouteHop},
        types::features::{ChannelFeatures, NodeFeatures},
    };
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn ldk_route_aad_bcs_roundtrip() {
        roundtrip::bcs_roundtrip_proptest::<LdkRouteAad>();
    }

    #[test]
    fn ldk_route_continuation_roundtrip() {
        let hop =
            |pubkey: &str, short_channel_id: u64, fee_msat: u64| RouteHop {
                pubkey: PublicKey::from_str(pubkey).unwrap(),
                node_features: NodeFeatures::empty(),
                short_channel_id,
                channel_features: ChannelFeatures::empty(),
                fee_msat,
                cltv_expiry_delta: 144,
                maybe_announced_channel: true,
            };
        let hops = vec![
            hop(
                "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                1,
                1_000,
            ),
            hop(
                "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
                2,
                50_000,
            ),
        ];
        // TODO(nicole): write a strategy for LDK's `Route`
        let route = Route {
            paths: vec![Path {
                hops,
                blinded_tail: None,
            }],
            route_params: None,
        };

        proptest!(|(
            seed in any::<[u8; 32]>(),
            aad in any::<LdkRouteAad>(),
            first_hop_fee in any::<Amount>(),
        )| {
            let mac_key = hmac::Key::from_seed(&seed);
            let continuation = LdkRouteContinuation::from_route_and_sign(
                &route,
                first_hop_fee,
                &mac_key,
                &aad,
            );
            let json = serde_json::to_string(&continuation).unwrap();
            let continuation =
                serde_json::from_str::<LdkRouteContinuation>(&json).unwrap();
            let values =
                continuation.as_route_and_validate(&mac_key, &aad).unwrap();
            prop_assert_eq!(&values.route, &route);
            prop_assert_eq!(values.first_hop_fee, first_hop_fee);
        });
    }

    /// Equal [`Amount`]s can serialize differently, which is why
    /// [`LdkRouteAad`] holds the partner base fee in msats.
    #[test]
    fn equal_amounts_can_serialize_differently() {
        let a1 = Amount::from_sats_u32(1);
        let a2 = serde_json::from_str::<Amount>("\"1.000\"").unwrap();
        assert_eq!(a1, a2);
        // This could cause false negatives during signature validation.
        assert_ne!(bcs::to_bytes(&a1).unwrap(), bcs::to_bytes(&a2).unwrap());
    }
}
