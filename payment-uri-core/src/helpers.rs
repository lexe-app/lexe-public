use bitcoin::address::NetworkUnchecked;
use lexe_api_core::types::invoice::LxInvoice;

use crate::payment_method::{Onchain, PaymentMethod};

pub(crate) trait AddressExt {
    /// Returns `true` if the given string matches any HRP prefix for BTC
    /// addresses.
    fn matches_hrp_prefix(s: &str) -> bool;

    /// Returns `true` if this address type is supported in a BIP21 URI
    /// body.
    fn is_supported_in_uri_body(&self) -> bool;

    /// Returns `true` if this address type is supported in a BIP321 URI
    /// query param.
    fn is_supported_in_uri_query_param(&self) -> bool;
}

impl AddressExt for bitcoin::Address<NetworkUnchecked> {
    fn matches_hrp_prefix(s: &str) -> bool {
        const HRPS: [&[u8]; 3] = [b"bc1", b"tb1", b"bcrt1"];
        let s = s.as_bytes();
        HRPS.iter().any(|hrp| match s.split_at_checked(hrp.len()) {
            Some((prefix, _)) => hrp.eq_ignore_ascii_case(prefix),
            _ => false,
        })
    }

    fn is_supported_in_uri_body(&self) -> bool {
        use bitcoin::AddressType::*;
        let address_type = match self.assume_checked_ref().address_type() {
            Some(x) => x,
            // Non-standard
            None => return true,
        };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => true,
            // Pay to script hash.
            P2sh => true,
            // Pay to witness pubkey hash.
            P2wpkh => true,
            // Pay to witness script hash.
            P2wsh => true,
            // Pay to taproot.
            P2tr => false,
            // Unknown standard
            _ => false,
        }
    }

    fn is_supported_in_uri_query_param(&self) -> bool {
        use bitcoin::AddressType::*;
        let address_type = match self.assume_checked_ref().address_type() {
            Some(x) => x,
            // Non-standard
            None => return true,
        };
        match address_type {
            // Pay to pubkey hash.
            P2pkh => false,
            // Pay to script hash.
            P2sh => false,
            // Pay to witness pubkey hash.
            P2wpkh => true,
            // Pay to witness script hash.
            P2wsh => true,
            // Pay to taproot.
            P2tr => true,
            // Unknown standard
            _ => true,
        }
    }
}

/// "Flatten" an [`LxInvoice`] into its "component" [`PaymentMethod`]s,
/// pushing them into an existing `Vec`.
pub(crate) fn flatten_invoice_into(
    invoice: LxInvoice,
    out: &mut Vec<PaymentMethod>,
) {
    let onchain_fallback_addrs = invoice.onchain_fallbacks();
    out.reserve(1 + onchain_fallback_addrs.len());

    // BOLT11 invoices may include onchain fallback addresses.
    if !onchain_fallback_addrs.is_empty() {
        let description = invoice.description_str().map(str::to_owned);
        let amount = invoice.amount();

        for addr in onchain_fallback_addrs {
            // TODO(max): Upstream an `Address::into_unchecked` to avoid
            // clone
            let address = addr.as_unchecked().clone();
            out.push(PaymentMethod::Onchain(Onchain {
                address,
                amount,
                label: None,
                message: description.clone(),
            }));
        }
    }

    out.push(PaymentMethod::Invoice(invoice));
}
