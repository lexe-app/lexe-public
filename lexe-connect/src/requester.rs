//! The REQUESTER side: build requests, accept and verify responses.

use lexe_crypto::{hpke, rng::Crng};
use lexe_uri::Uri;

use crate::{
    mailbox::Address,
    request::{
        CredentialRequest, CredentialRequestParams, Delivery, HpkePubkey,
        OneTimeSecret, RequestError,
    },
    response::{CredentialResponse, CredentialResult, ResponseError},
    seal::Blob,
};

#[derive(Debug, thiserror::Error)]
pub enum AcceptError {
    #[error("Redirect url has no response param")]
    MissingResponse,
    #[error("Response is not valid base64url: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("Response could not be decrypted: {0}")]
    Decrypt(#[from] hpke::Error),
    #[error(transparent)]
    Response(#[from] ResponseError),
    #[error("Response one_time_secret does not match the request")]
    SecretMismatch,
    #[error("Response account does not match the request")]
    AccountMismatch,
    #[error("Granted scopes or permissions differ from the request")]
    GrantMismatch,
}

/// An outstanding request and the secrets needed to accept its response.
/// Keep it until a response is accepted or the request expires, then drop it
/// so a second response can't be accepted.
#[derive(Debug)]
pub struct PendingRequest {
    request: CredentialRequest,
    ephemeral_key_pair: Option<hpke::KeyPair>,
}

impl PendingRequest {
    /// A request whose response is encrypted. Required for redirect and
    /// mailbox delivery; recommended for post.
    pub fn new(
        rng: &mut impl Crng,
        params: CredentialRequestParams,
    ) -> Result<Self, RequestError> {
        let key_pair = hpke::KeyPair::from_rng(rng);
        Self::build(rng, params, Some(key_pair))
    }

    /// A request whose response is plaintext JSON. Only valid with
    /// [`Delivery::Post`], where TLS protects the response in transit.
    pub fn new_plaintext(
        rng: &mut impl Crng,
        params: CredentialRequestParams,
    ) -> Result<Self, RequestError> {
        Self::build(rng, params, None)
    }

    fn build(
        rng: &mut impl Crng,
        params: CredentialRequestParams,
        ephemeral_key_pair: Option<hpke::KeyPair>,
    ) -> Result<Self, RequestError> {
        let request = CredentialRequest {
            ephemeral_hpke_pubkey: ephemeral_key_pair
                .as_ref()
                .map(|key_pair| HpkePubkey::from_hpke(key_pair.public_key())),
            one_time_secret: OneTimeSecret::from_rng(rng),
            params,
        };
        request.validate()?;

        Ok(Self {
            request,
            ephemeral_key_pair,
        })
    }

    pub fn request(&self) -> &CredentialRequest {
        &self.request
    }

    /// The connection string to open, QR-encode, or have the user paste.
    pub fn connection_string(&self) -> String {
        self.request.to_string()
    }

    /// Whether the response arrives at a mailbox, and if so, which address to
    /// poll.
    pub fn mailbox(&self) -> Option<(&str, Address)> {
        match &self.request.params.delivery {
            Delivery::Mailbox(url) =>
                Some((url, self.request.mailbox_address())),
            _ => None,
        }
    }

    /// Accept the url the WALLET redirected the user to.
    pub fn accept_redirect(
        &self,
        url: &str,
    ) -> Result<CredentialResponse, AcceptError> {
        let encoded = Uri::parse(url)
            .ok()
            .and_then(|uri| {
                uri.params
                    .into_iter()
                    .find(|param| param.key == "response")
                    .map(|param| param.value.into_owned())
            })
            .ok_or(AcceptError::MissingResponse)?;
        let blob = Blob::from_base64url(&encoded)?;

        self.accept_body(&blob.0)
    }

    /// Accept a POST body or mailbox blob. Decrypts it if this request's
    /// response is encrypted, then checks that the response matches this
    /// request:
    ///
    /// - `one_time_secret` and `account` are echoed.
    /// - A grant carries exactly the requested scopes and permissions.
    //
    // Spec: Protocol fields and Response forgery.
    pub fn accept_body(
        &self,
        body: &[u8],
    ) -> Result<CredentialResponse, AcceptError> {
        let response = match &self.ephemeral_key_pair {
            Some(key_pair) =>
                CredentialResponse::open(body, key_pair, &self.request)?,
            None => CredentialResponse::from_json(body)?,
        };

        let request = &self.request;
        if response.one_time_secret != request.one_time_secret {
            return Err(AcceptError::SecretMismatch);
        }
        if response.account != request.params.account {
            return Err(AcceptError::AccountMismatch);
        }
        if let CredentialResult::Granted(grant) = &response.result
            && (grant.scopes != request.params.scopes
                || grant.permissions != request.params.permissions)
        {
            return Err(AcceptError::GrantMismatch);
        }

        Ok(response)
    }
}
