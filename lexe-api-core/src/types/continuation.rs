//! Continuations that Lexe returns to the user, which may be passed back to
//! other endpoints.
//!
//! These continuations should be cryptographically resistant to tampering.
//! See [`lexe_common::root_seed::RootSeed::derive_continuation_mac_key`].

use anyhow::anyhow;
use lexe_byte_array::ByteArray;
use lexe_crypto::hmac;
use lexe_serde::base64_or_bytes;
use lexe_std::array;
use lightning::{
    routing::router::Route,
    util::ser::{Readable, Writeable},
};
use serde::{Deserialize, Serialize};

use crate::types::payments::PaymentHash;

/// An LDK [`Route`] continuation, which may have been received from an
/// untrusted source. The inner bytes are unverified until
/// [`Self::as_route_and_validate`].
///
/// AADs: The [`PaymentHash`] of the invoice associated with the [`Route`].
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
#[derive(Serialize, Deserialize)]
pub struct LdkRouteContinuation(#[serde(with = "base64_or_bytes")] pub Vec<u8>);

impl LdkRouteContinuation {
    const DOMAIN_SEPARATOR: [u8; 32] =
        array::pad(*b"LEXE-REALM::LdkRouteContinuation");

    /// Serialize a [`Route`] and sign it, for use as an
    /// [`LdkRouteContinuation`].
    pub fn from_route_and_sign(
        route: &Route,
        mac_key: &hmac::Key,
        payment_hash: &PaymentHash,
    ) -> Self {
        let signed = mac_key.sign_and_append(
            &Self::DOMAIN_SEPARATOR,
            &[payment_hash.as_slice()],
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
        payment_hash: &PaymentHash,
    ) -> anyhow::Result<Route> {
        let bytes = mac_key
            .verify(
                &Self::DOMAIN_SEPARATOR,
                &[payment_hash.as_slice()],
                &self.0,
            )
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        let route = Route::read(&mut &bytes[..])
            .map_err(|_| anyhow!("Invalid `ldk_route`."))?;
        Ok(route)
    }
}
