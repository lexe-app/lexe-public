//! LexeConnect: one-tap sharing of a Lexe client credential with an app or
//! service. Implements the protocol parts of the [LexeConnect spec]; UI and
//! credential creation are left to the WALLET.
//!
//! 1. The REQUESTER builds a `PendingRequest` and shows its connection string
//!    as a link or QR code.
//! 2. The WALLET [parses] it, shows the user a [`RequesterDisplay`], and
//!    responds with a [`DeliveryAction`].
//! 3. The REQUESTER accepts the delivered bytes, which yields the verified
//!    [`CredentialResponse`].
//!
//! - Comments cite the spec by section heading, e.g. `// Spec: Encryption`.
//! - Everything is I/O-free except the `http` module.
//! - The REQUESTER side and `respond` need the `crypto` feature; without it the
//!   crate is types only, e.g. for parsing connection strings.
//!
//! [LexeConnect spec]: https://github.com/lexe-app/lexe-connect
//! [parses]: request::CredentialRequest::parse
//! [`RequesterDisplay`]: wallet::RequesterDisplay
//! [`DeliveryAction`]: wallet::DeliveryAction
//! [`CredentialResponse`]: response::CredentialResponse

/// HTTP delivery and a mailbox client.
#[cfg(feature = "http")]
pub mod http;

/// The mailbox address and an in-memory mailbox store.
pub mod mailbox;
/// The credential request and its connection string.
pub mod request;
/// The REQUESTER side: build requests, accept and verify responses.
#[cfg(feature = "crypto")]
pub mod requester;
/// The credential response and its JSON form.
pub mod response;
/// HPKE sealing of responses.
pub mod seal;
/// The WALLET side: approval screen model and response delivery.
pub mod wallet;

/// Lexe's public mailbox.
//
// Spec: `mailbox_url` delivery.
pub const LEXE_MAILBOX_URL: &str = "https://lexe.app/mailbox";
