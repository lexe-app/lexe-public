//! custom extension, which we'll embed into our remote attestation cert.
//! Get a quote for the running node enclave and return it as an x509 cert
//!
//! On non-SGX platforms, we just return a dummy extension for now.

use lexe_crypto::ed25519;

/// Small newtype for [`sgx_isa::Report::reportdata`] field.
/// For now, we only use the first 32 out of 64 bytes to commit to a cert pk.
#[derive(Debug)]
pub struct ReportData([u8; 64]);

impl ReportData {
    /// Construct from the `reportdata` of an existing [`sgx_isa::Report`].
    pub fn new(reportdata: [u8; 64]) -> Self {
        Self(reportdata)
    }

    pub fn from_cert_pk(pk: &ed25519::PublicKey) -> Self {
        let mut report_data = [0u8; 64];
        // ed25519 pks are always 32 bytes.
        // This will panic if this internal invariant is somehow not true.
        report_data[..32].copy_from_slice(pk.as_ref());
        Self(report_data)
    }

    pub fn as_inner(&self) -> &[u8; 64] {
        &self.0
    }

    pub fn into_inner(self) -> [u8; 64] {
        self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Whether the first 32 bytes contains the given [`ed25519::PublicKey`].
    pub fn contains(&self, cert_pk: &ed25519::PublicKey) -> bool {
        &self.0[..32] == cert_pk.as_slice()
    }
}
