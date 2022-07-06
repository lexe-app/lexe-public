// hello

mod certgen;
mod quote;

pub use certgen::CertificateParams;
pub use quote::quote_enclave;
use rcgen::{KeyPair, RcgenError, PKCS_ED25519};

pub(crate) fn gen_ed25519_key_pair() -> Result<KeyPair, RcgenError> {
    // TODO(phlip9): wish this didn't use an implicit rng...
    KeyPair::generate(&PKCS_ED25519)
}
