//! Continuations that Lexe returns to the user, which may be passed back to
//! other endpoints.
//!
//! These continuations should be cryptographically resistant to tampering.
//! See [`lexe_common::root_seed::RootSeed::derive_continuation_mac_key`].

use anyhow::anyhow;
use lexe_common::{api::user::UserPk, ppm::Ppm};
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

    /// Serialize a [`Route`] and sign it, for use as an
    /// [`LdkRouteContinuation`].
    pub fn from_route_and_sign(
        route: &Route,
        mac_key: &hmac::Key,
        aad: &LdkRouteAad,
    ) -> Self {
        let aad = aad.to_bytes();
        let signed = mac_key.sign_and_append(
            &Self::DOMAIN_SEPARATOR,
            &[aad.as_slice()],
            Some(route.serialized_length()),
            &|out| route.write(out).expect("Vec::write is infallible"),
        );
        Self(signed)
    }

    /// Verify the authenticity of an [`LdkRouteContinuation`] and extract its
    /// [`Route`].
    pub fn as_route_and_validate(
        &self,
        mac_key: &hmac::Key,
        aad: &LdkRouteAad,
    ) -> anyhow::Result<Route> {
        let aad = aad.to_bytes();
        let bytes = mac_key
            .verify(&Self::DOMAIN_SEPARATOR, &[aad.as_slice()], &self.0)
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        let route = Route::read(&mut &bytes[..])
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        Ok(route)
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
    use lexe_common::{ln::amount::Amount, test_utils::roundtrip};

    use super::*;

    #[test]
    fn ldk_route_aad_bcs_roundtrip() {
        roundtrip::bcs_roundtrip_proptest::<LdkRouteAad>();
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
