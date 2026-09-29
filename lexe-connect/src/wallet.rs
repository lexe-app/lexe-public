//! The WALLET side: approval screen model and response delivery.

use lexe_common::time::TimestampMs;
#[cfg(feature = "crypto")]
use lexe_crypto::rng::Crng;
use lexe_uri::Uri;

use crate::request::{CredentialRequest, Delivery};
#[cfg(feature = "crypto")]
use crate::{
    request::RequestError,
    response::{
        CredentialError, CredentialResponse, CredentialResult, ErrorCode, Grant,
    },
    seal::SealError,
};

#[cfg(feature = "crypto")]
#[derive(Debug, thiserror::Error)]
pub enum RespondError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error(transparent)]
    Seal(#[from] SealError),
}

/// How the approval screen identifies the REQUESTER.
//
// Spec: User Approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RequesterDisplay {
    /// The response goes to this domain, or to an app verified for it.
    Verified { domain: String },
    /// No receiving domain is known. `scheme_host` is the `redirect_uri`'s
    /// scheme and host, e.g. `myprotocol://`; `None` for mailbox delivery.
    Unverified { scheme_host: Option<String> },
}

/// The user's decision on a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// A credential was created with the requested scopes and permissions.
    Approved {
        credential: String,
        expires_at: Option<TimestampMs>,
    },
    Rejected,
    /// The WALLET could not create the credential.
    Failed(String),
}

/// How the WALLET delivers a response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryAction {
    /// Open this url; the OS routes it to the REQUESTER.
    Redirect(String),
    /// Perform this request,
    /// e.g. with `LexeConnectClient::deliver` from the `http` module.
    Http(HttpDelivery),
}

/// A POST the WALLET must make. A mailbox accepts a byte-identical re-post
/// but rejects any other, so retry with the same body rather than re-sealing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpDelivery {
    pub url: String,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

#[cfg(feature = "crypto")]
impl HttpDelivery {
    // Spec: `post_url` delivery and `mailbox_url` delivery.
    const CONTENT_TYPE_JSON: &str = "application/json";
    const CONTENT_TYPE_OCTET_STREAM: &str = "application/octet-stream";
}

impl CredentialRequest {
    pub fn requester_display(&self) -> RequesterDisplay {
        let uri = match &self.params.delivery {
            Delivery::Redirect(uri) | Delivery::Post(uri) => Uri::parse(uri),
            Delivery::Mailbox(_) =>
                return RequesterDisplay::Unverified { scheme_host: None },
        };

        let Ok(uri) = uri else {
            return RequesterDisplay::Unverified { scheme_host: None };
        };
        let scheme = uri.scheme;
        match uri.host() {
            Some(host) if uri.is_https() => RequesterDisplay::Verified {
                domain: host.into_owned(),
            },
            Some(host) => RequesterDisplay::Unverified {
                scheme_host: Some(format!("{scheme}://{host}")),
            },
            None => RequesterDisplay::Unverified {
                scheme_host: Some(match uri.authority {
                    true => format!("{scheme}://"),
                    false => format!("{scheme}:"),
                }),
            },
        }
    }

    /// Build the response for `outcome` and package it for delivery.
    //
    // Spec: Credential response.
    #[cfg(feature = "crypto")]
    pub fn respond(
        &self,
        rng: &mut impl Crng,
        outcome: Outcome,
    ) -> Result<DeliveryAction, RespondError> {
        self.validate()?;

        let result = match outcome {
            Outcome::Approved {
                credential,
                expires_at,
            } => CredentialResult::Granted(Grant {
                credential,
                scopes: self.params.scopes.clone(),
                permissions: self.params.permissions.clone(),
                expires_at,
            }),
            Outcome::Rejected => CredentialResult::Error(CredentialError {
                code: ErrorCode::UserRejected,
                message: None,
            }),
            Outcome::Failed(message) =>
                CredentialResult::Error(CredentialError {
                    code: ErrorCode::Other,
                    message: Some(message),
                }),
        };
        let response = CredentialResponse {
            result,
            one_time_secret: self.one_time_secret,
            account: self.params.account.clone(),
            metadata: self.params.metadata.clone(),
        };

        let action = match &self.params.delivery {
            Delivery::Redirect(uri) => {
                let blob = response.seal(rng, self)?.to_base64url();
                let mut uri =
                    Uri::parse(uri).expect("validate() above parsed it");
                uri.push_param("response", blob);
                DeliveryAction::Redirect(uri.to_string())
            }
            Delivery::Post(url) if self.ephemeral_hpke_pubkey.is_some() =>
                DeliveryAction::Http(HttpDelivery {
                    url: url.clone(),
                    content_type: HttpDelivery::CONTENT_TYPE_OCTET_STREAM,
                    body: response.seal(rng, self)?.0,
                }),
            Delivery::Post(url) => DeliveryAction::Http(HttpDelivery {
                url: url.clone(),
                content_type: HttpDelivery::CONTENT_TYPE_JSON,
                body: response.to_json(),
            }),
            Delivery::Mailbox(url) => DeliveryAction::Http(HttpDelivery {
                url: {
                    let mut url =
                        Uri::parse(url).expect("validate() above parsed it");
                    url.push_param(
                        "address",
                        self.mailbox_address().to_string(),
                    );
                    url.to_string()
                },
                content_type: HttpDelivery::CONTENT_TYPE_OCTET_STREAM,
                body: response.seal(rng, self)?.0,
            }),
        };

        Ok(action)
    }
}

#[cfg(test)]
mod test {
    use lexe_byte_array::ByteArray;
    use lexe_crypto::rng::SysRng;

    use super::*;
    use crate::request::{CredentialRequestParams, OneTimeSecret};

    /// A hand-built request that skips [`CredentialRequest::parse`].
    fn request(delivery: Delivery) -> CredentialRequest {
        CredentialRequest {
            ephemeral_hpke_pubkey: None,
            one_time_secret: OneTimeSecret::from_array([0; 16]),
            params: CredentialRequestParams {
                delivery,
                account: None,
                metadata: None,
                scopes: ["read_info".to_owned()].into(),
                permissions: Default::default(),
                label: None,
                expires_at: None,
            },
        }
    }

    #[test]
    fn requester_display() {
        let display =
            |delivery: Delivery| request(delivery).requester_display();
        let verified = |domain: &str| RequesterDisplay::Verified {
            domain: domain.into(),
        };
        let unverified =
            |scheme_host: Option<&str>| RequesterDisplay::Unverified {
                scheme_host: scheme_host.map(String::from),
            };
        assert_eq!(
            display(Delivery::Redirect("https://BillSplit.com/cb".into())),
            verified("billsplit.com")
        );
        assert_eq!(
            display(Delivery::Redirect("https://a@evil.com:1/".into())),
            verified("evil.com")
        );
        assert_eq!(
            display(Delivery::Post("https://paygate.com/x".into())),
            verified("paygate.com")
        );
        assert_eq!(
            display(Delivery::Redirect("myprotocol://".into())),
            unverified(Some("myprotocol://"))
        );
        assert_eq!(
            display(Delivery::Redirect("http://a.com/cb".into())),
            unverified(Some("http://a.com"))
        );
        assert_eq!(
            display(Delivery::Mailbox("https://lexe.app/mailbox".into())),
            unverified(None)
        );
    }

    /// `respond` validates, so an unparsed request can't reach the `expect`s.
    #[test]
    fn respond_validates() {
        let mut rng = SysRng::new();
        let insecure = request(Delivery::Post("http://paygate.com/x".into()));
        assert!(matches!(
            insecure.respond(&mut rng, Outcome::Rejected),
            Err(RespondError::Request(RequestError::Invalid("post_url", _)))
        ));
    }
}
