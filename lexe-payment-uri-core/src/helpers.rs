use bitcoin::address::NetworkUnchecked;
use lexe_api_core::types::invoice::Invoice;

use crate::payment_method::PaymentMethod;

/// "Flatten" an [`Invoice`] into its component [`PaymentMethod`]s.
///
/// BOLT11 invoices can embed one or more onchain fallback addresses (the
/// `f` field), so a single invoice may resolve to the invoice itself *plus*
/// a [`PaymentMethod::Onchain`] entry for each fallback.
pub fn flatten_invoice(invoice: Invoice) -> Vec<PaymentMethod> {
    let onchain_fallback_addrs = invoice.onchain_fallbacks();

    let mut methods = Vec::with_capacity(1 + onchain_fallback_addrs.len());

    // BOLT11 invoices may include onchain fallback addresses.
    if !onchain_fallback_addrs.is_empty() {
        let description = invoice.description_str().map(str::to_owned);
        let amount = invoice.amount();

        for address in onchain_fallback_addrs {
            methods.push(PaymentMethod::Onchain {
                address,
                amount,
                label: None,
                message: description.clone(),
            });
        }
    }

    methods.push(PaymentMethod::Invoice { invoice });

    methods
}

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
