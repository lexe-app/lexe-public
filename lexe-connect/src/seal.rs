//! HPKE sealing of responses.

#[cfg(feature = "crypto")]
use base64::Engine;
use lexe_byte_array::ByteArray;
#[cfg(feature = "crypto")]
use lexe_crypto::{hpke, rng::Crng};
use lexe_sha256::sha256;

use crate::{
    mailbox::Address,
    request::{CredentialRequest, OneTimeSecret},
};
#[cfg(feature = "crypto")]
use crate::{requester::AcceptError, response::CredentialResponse};

#[cfg(feature = "crypto")]
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SealError {
    #[error("Request has no ephemeral_hpke_pubkey")]
    NoPubkey,
    #[error(transparent)]
    Hpke(#[from] hpke::Error),
}

/// A sealed [`CredentialResponse`]: the encapsulated key followed by the
/// ciphertext, from single-shot HPKE under the request's
/// `ephemeral_hpke_pubkey`, `hpke_info`, and `hpke_aad`.
///
/// [`CredentialResponse`]: crate::response::CredentialResponse
//
// Spec: Encryption.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Blob(pub Vec<u8>);

#[cfg(feature = "crypto")]
impl Blob {
    const BASE64URL: base64::engine::GeneralPurpose =
        base64::engine::general_purpose::URL_SAFE_NO_PAD;

    pub(crate) fn to_base64url(&self) -> String {
        Self::BASE64URL.encode(&self.0)
    }

    pub(crate) fn from_base64url(s: &str) -> Result<Self, base64::DecodeError> {
        Self::BASE64URL.decode(s).map(Self)
    }
}

#[cfg(feature = "crypto")]
impl CredentialResponse {
    /// Seal to the request's `ephemeral_hpke_pubkey`.
    pub fn seal(
        self,
        rng: &mut impl Crng,
        request: &CredentialRequest,
    ) -> Result<Blob, SealError> {
        let pk = request.ephemeral_hpke_pubkey.ok_or(SealError::NoPubkey)?;
        let pk = pk.to_hpke();
        let plaintext = self.to_json();

        let sealed =
            pk.seal(rng, &request.hpke_info(), request.hpke_aad(), &plaintext)?;

        Ok(Blob(sealed))
    }

    /// Open a sealed blob with the REQUESTER's ephemeral key pair. This
    /// proves the response was sealed to this request; see
    /// [`PendingRequest::accept_body`](crate::requester::PendingRequest::accept_body)
    /// for the checks a REQUESTER must still make.
    pub(crate) fn open(
        sealed: &[u8],
        key_pair: &hpke::KeyPair,
        request: &CredentialRequest,
    ) -> Result<Self, AcceptError> {
        let plaintext =
            key_pair.open(&request.hpke_info(), request.hpke_aad(), sealed)?;

        Ok(Self::from_json(&plaintext)?)
    }
}

impl CredentialRequest {
    /// Lexe's HPKE domain separator.
    //
    // Spec: WALLET-defined parts.
    const DOMAIN_SEPARATOR: &str = "LexeConnect-v1";

    /// The length of [`Self::hpke_info`].
    const HPKE_INFO_LEN: usize =
        Self::DOMAIN_SEPARATOR.len() + size_of::<OneTimeSecret>();

    /// The HPKE `info`: `DOMAIN_SEPARATOR || one_time_secret`.
    //
    // Spec: Encryption.
    fn hpke_info(&self) -> [u8; Self::HPKE_INFO_LEN] {
        let mut info = [0u8; Self::HPKE_INFO_LEN];
        let (separator, secret) =
            info.split_at_mut(Self::DOMAIN_SEPARATOR.len());
        separator.copy_from_slice(Self::DOMAIN_SEPARATOR.as_bytes());
        secret.copy_from_slice(self.one_time_secret.as_slice());
        info
    }

    /// The HPKE `aad`: the `account`, or empty if unset.
    //
    // Spec: Encryption.
    #[cfg(feature = "crypto")]
    fn hpke_aad(&self) -> &[u8] {
        self.params.account.as_deref().map_or(&[], str::as_bytes)
    }

    /// Where a mailbox response is deposited: `SHA256(hpke_info)`.
    //
    // Spec: `mailbox_url` delivery.
    pub fn mailbox_address(&self) -> Address {
        Address::from_array(sha256::digest(&self.hpke_info()).to_array())
    }
}

#[cfg(all(test, feature = "crypto"))]
mod test {
    use std::collections::BTreeSet;

    use lexe_common::time::TimestampMs;
    use lexe_hex::hex;
    use rand_core::{CryptoRng, RngCore};

    use super::*;
    use crate::response::{
        CredentialError, CredentialResult, ErrorCode, Grant,
    };

    /// Hands out a fixed byte string, so the sealer's ephemeral key comes
    /// from the test vector's `ikmE`.
    struct FixedRng(Vec<u8>);

    impl RngCore for FixedRng {
        fn next_u32(&mut self) -> u32 {
            rand_core::impls::next_u32_via_fill(self)
        }
        fn next_u64(&mut self) -> u64 {
            rand_core::impls::next_u64_via_fill(self)
        }
        fn fill_bytes(&mut self, dst: &mut [u8]) {
            let rest = self.0.split_off(dst.len());
            dst.copy_from_slice(&self.0);
            self.0 = rest;
        }
        fn try_fill_bytes(
            &mut self,
            dst: &mut [u8],
        ) -> Result<(), rand_core::Error> {
            self.fill_bytes(dst);
            Ok(())
        }
    }

    impl CryptoRng for FixedRng {}

    /// The spec's "Test vectors" section; keep the two in sync.
    #[test]
    fn spec_test_vectors() {
        // `ikmR` is 0x00..=0x1f, `ikmE` is 0x20..=0x3f.
        let ikm_r: [u8; 32] = std::array::from_fn(|i| i as u8);
        let ikm_e = (0x20u8..0x40).collect::<Vec<u8>>();
        let key_pair = hpke::KeyPair::from_seed(&ikm_r);
        assert_eq!(
            key_pair.public_key().to_string(),
            "b1f1b840de7a3241b02748cf9b05b74dc8c5e8451298738817bd76aa8ebe8c2b"
        );

        let request = CredentialRequest::parse(
            "https://lexe.app/connect?v=1\
             &redirect_uri=https%3A%2F%2Fbillsplit.com%2Fcb\
             &ephemeral_hpke_pubkey=\
             b1f1b840de7a3241b02748cf9b05b74dc8c5e8451298738817bd76aa8ebe8c2b\
             &one_time_secret=000102030405060708090a0b0c0d0e0f\
             &account=%40janedoe&scopes=read_info,receive",
        )
        .unwrap();
        assert_eq!(
            hex::encode(&request.hpke_info()),
            "4c657865436f6e6e6563742d7631000102030405060708090a0b0c0d0e0f"
        );
        assert_eq!(hex::encode(request.hpke_aad()), "406a616e65646f65");

        let granted = CredentialResponse {
            result: CredentialResult::Granted(Grant {
                credential: "<client credential>".to_owned(),
                scopes: ["read_info", "receive"].map(String::from).into(),
                permissions: BTreeSet::new(),
                expires_at: Some(
                    TimestampMs::from_millis(1821484800000).unwrap(),
                ),
            }),
            one_time_secret: request.one_time_secret,
            account: request.params.account.clone(),
            metadata: None,
        };
        let rejected = CredentialResponse {
            result: CredentialResult::Error(CredentialError {
                code: ErrorCode::UserRejected,
                message: None,
            }),
            one_time_secret: request.one_time_secret,
            account: request.params.account.clone(),
            metadata: None,
        };
        let vectors = [
            (
                granted,
                r#"{"credential":"<client credential>","one_time_secret":"000102030405060708090a0b0c0d0e0f","account":"@janedoe","scopes":["read_info","receive"],"permissions":[],"expires_at":1821484800000}"#,
                "693658254630f73ad8da78fb331bf976cd42f90e0e9c9e83f40c51072a6f741718c87dfe078291112ed11fbd29fea41f7e497c02a693481c499717cb9629bf33148f84ebf928eb111ed0525e2d2cc20f23ed8043bba85f5bc6eed0b463e41461d1fc4b8238e0c59aea1e89b6041ebb0718ea5f8133994b74cd6aa22056e9e1827a5daac94185756c7a53dc4ab5d3c3f68aef2293063d337e3726489013a06e27ae68a35c9f8ae7b7fa819dafd8298949878936df24a556c2d92cd08241456aa0a7fc06f1d0562c0d0757c368b873f320f167487a45d80076451cf9135074fd7a81e94dde3e7f9ece9c5676",
            ),
            (
                rejected,
                r#"{"error":"user_rejected","one_time_secret":"000102030405060708090a0b0c0d0e0f","account":"@janedoe"}"#,
                "693658254630f73ad8da78fb331bf976cd42f90e0e9c9e83f40c51072a6f741718c87bfe1089865d609a0ba26eb6d951784f7004bc820c5d17d01cc09d02a23b15d4f9b4be24f711358601117843815b70add504a9a64d5ec6e8d1b361ec1768d5ad4ed03eb3c2cee24b80e04702fb5618b954c438dd0f6cc308a9225be3f0836b5dedff05a6d01574bfc9c06ccb2be20b0139",
            ),
        ];

        for (response, plaintext, blob_hex) in vectors {
            assert_eq!(response.clone().to_json(), plaintext.as_bytes());

            let mut rng = FixedRng(ikm_e.clone());
            let blob = response.seal(&mut rng, &request).unwrap();
            assert_eq!(hex::encode(&blob.0), blob_hex);

            let opened =
                CredentialResponse::open(&blob.0, &key_pair, &request).unwrap();
            assert_eq!(opened.to_json(), plaintext.as_bytes());
        }
    }
}
