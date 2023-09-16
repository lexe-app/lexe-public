//! A crate which abstracts over the Google Drive v3 API to provide a ["VFS"]
//! interface which can be used to store, fetch, update, and delete data stored
//! in Google's 3rd party cloud.
//!
//! ["VFS"]: common::api::vfs
//!
//! ## Requirements
//!
//! The main public interfaces are [`ApiCredentials`] and [`GoogleVfs`]. The
//! crate user is expected to walk the user through the [installed app OAuth2
//! flow], and is required to supply five pieces of information which are
//! necessary to construct the [`ApiCredentials`]:
//!
//! - `client_id`
//! - `client_secret`
//! - `access_token`
//! - `refresh_token`
//! - `access_token_expiry` (Can be set to 0 if unknown)
//!
//! The [`GoogleVfs`] can then be initialized using the [`ApiCredentials`] and
//! used as an atomic VFS thereafter.
//!
//! The VFS interface takes and returns raw [`Vec<u8>`] ciphertexts; it is up to
//! the caller to handle encryption/decryption and check the integrity of
//! returned ciphertexts.
//!
//! [installed app OAuth2 flow]: https://developers.google.com/identity/protocols/oauth2/native-app
//!
//! ## Rollback Protection
//!
//! Since Google is an independent 3rd party, and Lexe does not have access to
//! the Google Drive OAuth2 tokens used to make API requests, the [`GoogleVfs`]
//! is a suitable place to store data which must be resistant to rollbacks. API
//! calls can be quite expensive, however, so non security-critical data is more
//! suitably stored in Lexe's DB.
//!
//! ## The LexeData directory
//!
//! VFS files are stored in a folder in a user's My Drive called "X LexeData (DO
//! NOT RENAME, MODIFY, OR DELETE)". The folder can be moved within "My Drive"
//! but it cannot be renamed. It is also very important that users do not delete
//! or modify the files inside, which may result in data corruption and/or funds
//! loss. See the `lexe_dir` module-level docs for more info.
//!
//! ## Authorization Scopes
//!
//! The only required scope is `https://www.googleapis.com/auth/drive.file`,
//! which gives the ability to read/create/modify/delete files in "My Drive"
//! which were created by our app. Since this scope cannot be used to read a
//! user's other data, this qualifies as a "non-sensitive" scope which comes
//! which reduced app review requirements. Unlike the "appDataDir", data stored
//! in "My Drive" is NOT deleted if the user uninstalls their Lexe app. However,
//! the more restricted nature of this scope may pose problems if a user wishes
//! to restore their mobile app from a Google Drive backup that was previously
//! persisted under a different Lexe app ID, as those older files will not be
//! accessible to the newer app ID. Thus, we should avoid changing the Lexe app
//! ID, otherwise we will be forced to apply for the "restricted"
//! `https://www.googleapis.com/auth/drive` scope which gives access to all
//! files contained in a user's My Drive.
//!
//! ## Notes on testing
//!
//! - All tests in this crate make real API calls and are thus `#[ignored]`.
//! - Run tests like `cargo test -p gdrive -- --ignored <test> --show-output`.
//! - If an access token was refreshed during a test run, it will be printed to
//!   stdout. Set `--show-output` if you want to update your local env vars.
//! - Tests require setting env vars for each of the [`ApiCredentials`] fields.
//!   Note that the vars must be `export`ed for the test binary to detect them.
//! - Some tests create and delete the `regtest` VFS dir. Use `--test-threads=1`
//!   to avoid duplicates when running multiple ignored tests in one batch.
//!
//! Test run template:
//!
//! ```bash
//! export GOOGLE_CLIENT_ID="<client_id>"
//! export GOOGLE_CLIENT_SECRET="<client_secret>"
//! export GOOGLE_REFRESH_TOKEN="<refresh_token>"
//! export GOOGLE_ACCESS_TOKEN="<access_token>"
//! export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
//! cargo test -p gdrive -- --test-threads=1 --ignored --show-output [<test-name>]
//! ```

use reqwest::StatusCode;
use thiserror::Error;

/// Higher-level "Google VFS" interface.
pub mod gvfs;
/// Google OAuth2 credentials.
pub mod oauth2;

/// Lower-level API client.
pub(crate) mod api;
/// Utilities relating to the Lexe data dir in My Drive.
pub(crate) mod lexe_dir;
/// API models.
pub(crate) mod models;

pub use gvfs::GoogleVfs;
pub use oauth2::ApiCredentials;

#[derive(Debug, Error)]
pub enum Error {
    // -- OAuth2 Token errors -- //
    #[error("Error occurred during token refresh: {0}")]
    TokenRefresh(Box<Self>),
    #[error("Token expired")]
    TokenExpired,
    #[error("Token did not have sufficient scopes (permissions): {scope}")]
    InsufficientScopes { scope: String },
    #[error("Token had an access_type other than 'offline': {access_type}")]
    WrongAccessType { access_type: String },
    #[error("Token had a token_type other than 'Bearer': {token_type}")]
    WrongTokenType { token_type: String },

    // -- API error -- //
    #[error("API returned error response ({code}). Response: {resp_str}")]
    Api { code: StatusCode, resp_str: String },

    // -- Underlying error -- //
    #[error("serde_json error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Reqwest error: {0:#}")]
    Reqwest(#[from] reqwest::Error),
}
