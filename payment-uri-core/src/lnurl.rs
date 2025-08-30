// TODO(phlip9): support LNURL pay
pub(crate) struct Lnurl<'a> {
    _url: &'a str,
}

impl Lnurl<'_> {
    // LUD-01: base LNURL bech32 encoding
    pub(crate) fn matches_hrp_prefix(s: &str) -> bool {
        const HRP: &[u8; 6] = b"lnurl1";
        const HRP_LEN: usize = HRP.len();
        match s.as_bytes().split_first_chunk::<HRP_LEN>() {
            Some((s_hrp, _)) => s_hrp.eq_ignore_ascii_case(HRP),
            _ => false,
        }

        // TODO(phlip9): look for "http(s):" scheme with smuggled "lightning"
        // query parameter containing bech32 LNURL

        // TODO(phlip9): look for "lightning:" scheme with bech32 LNURL
    }

    // LUD-17: protocol schemes
    pub(crate) fn matches_scheme(s: &str) -> bool {
        s.eq_ignore_ascii_case("lnurl")
            // LUD-17: fine-grained protocol schemes
            || s.eq_ignore_ascii_case("lnurlc")
            || s.eq_ignore_ascii_case("lnurlw")
            || s.eq_ignore_ascii_case("lnurlp")
            || s.eq_ignore_ascii_case("keyauth")
    }
}
