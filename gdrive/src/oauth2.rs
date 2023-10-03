#[cfg(test)]
use std::env;
use std::{
    fmt,
    time::{Duration, SystemTime},
};

use common::const_assert;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, trace};

use crate::Error;

/// The expected value of `access_type`.
// 'offline' tells Google to give us refresh token that allows us to refresh the
// access token while the user is offline.
const ACCESS_TYPE: &str = "offline";
/// The expected value of `scope`.
// Gives us the ability to manage files and folders in My Drive that were
// created by our app. Qualifies as one of Google's "non-sensitive" scopes.
const API_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
/// The expected value of `token_type`.
// For the foreseeable future we are only interested in bearer auth tokens.
const TOKEN_TYPE: &str = "Bearer";
/// The minimum amount of time that access tokens are guaranteed to be valid
/// after a call to `refresh_if_necessary`. If an access token will expire in
/// time less than this (or if the token has already expired),
/// `refresh_if_necessary` will refresh the token.
pub const MINIMUM_TOKEN_LIFETIME: Duration = Duration::from_secs(60);
// Newly refreshed access tokens usually live for only 3600 seconds
const_assert!(MINIMUM_TOKEN_LIFETIME.as_secs() < 3600);

/// A complete set of credentials which will allow us to make requests to the
/// Google Drive API and periodically refresh our access tokens.
#[derive(Clone, Serialize, Deserialize)]
pub struct ApiCredentials {
    client_id: ApiSecret,
    client_secret: ApiSecret,
    refresh_token: ApiSecret,
    access_token: ApiSecret,
    /// Unix timestamp (in seconds) at which the current access token expires.
    expires_at: u64,
    /// Whether these credentials have been updated since deserialization.
    /// If [`true`], these credentials should be repersisted to avoid an
    /// unnecessary refresh in the future.
    // Tells serde:
    // - When serializing, skip this field
    // - When deserializing, set this field to the output of `default_updated`
    #[serde(skip_serializing, default = "default_updated")]
    updated: bool,
}

fn default_updated() -> bool {
    false
}

/// A wrapper to help prevent accidentally logging API secrets. We still need
/// make sure secrets are not included in query parameters, since those tend to
/// show up in logs. Use the bearer auth header or POST requests instead.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ApiSecret(String);

impl fmt::Debug for ApiSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiSecret(..)")
    }
}

impl ApiCredentials {
    pub fn new(
        client_id: String,
        client_secret: String,
        refresh_token: String,
        access_token: String,
        expires_at: u64,
    ) -> Self {
        Self {
            client_id: ApiSecret(client_id),
            client_secret: ApiSecret(client_secret),
            refresh_token: ApiSecret(refresh_token),
            access_token: ApiSecret(access_token),
            expires_at,
            updated: false,
        }
    }

    /// Attempts to construct an [`ApiCredentials`] from env.
    ///
    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// ```
    #[cfg(test)] // Don't think we need this outside of tests
    pub fn from_env() -> anyhow::Result<Self> {
        use std::str::FromStr;

        use anyhow::Context;

        let client_id = env::var("GOOGLE_CLIENT_ID")
            .context("Missing 'GOOGLE_CLIENT_ID' in env")?;
        let client_secret = env::var("GOOGLE_CLIENT_SECRET")
            .context("Missing 'GOOGLE_CLIENT_SECRET' in env")?;
        let refresh_token = env::var("GOOGLE_REFRESH_TOKEN")
            .context("Missing 'GOOGLE_REFRESH_TOKEN' in env")?;
        let access_token = env::var("GOOGLE_ACCESS_TOKEN")
            .context("Missing 'GOOGLE_ACCESS_TOKEN' in env")?;
        let expires_at_str = env::var("GOOGLE_ACCESS_TOKEN_EXPIRY")
            .context("Missing 'GOOGLE_ACCESS_TOKEN_EXPIRY' in env")?;
        let expires_at = u64::from_str(&expires_at_str)
            .context("Invalid GOOGLE_ACCESS_TOKEN_EXPIRY")?;

        Ok(Self::new(
            client_id,
            client_secret,
            refresh_token,
            access_token,
            expires_at,
        ))
    }

    /// Get a reference to the contained `access_token`.
    pub(crate) fn access_token(&self) -> &str {
        &self.access_token.0
    }

    /// Whether these credentials have been updated since deserialization.
    /// If [`true`], the caller should repersist these credentials to avoid an
    /// unnecessary refresh in the future.
    pub fn updated(&self) -> bool {
        self.updated
    }

    /// Makes a call to Google's `tokeninfo` endpoint to check:
    ///
    /// - The access token has the required scope(s).
    /// - The access token has the expected access type.
    /// - Google recognizes our access token, and confirms it is not expired.
    ///
    /// We should not need to call this method regularly - just once when the
    /// credentials were initially obtained from the user should be enough.
    #[instrument(skip_all, name = "(oauth2-check-token-info)")]
    pub async fn check_token_info(
        &mut self,
        client: &reqwest::Client,
    ) -> Result<(), Error> {
        debug!("Checking token info");

        #[derive(Deserialize)]
        struct TokenInfo {
            scope: String,
            expires_in: u32,
            access_type: String,
        }

        let http_resp = client
            .post("https://www.googleapis.com/oauth2/v3/tokeninfo")
            .json(&[("access_token", &self.access_token.0)])
            .send()
            .await?;

        let token_info = match http_resp.status() {
            StatusCode::OK => http_resp.json::<TokenInfo>().await?,
            // We get a 400 if the token expired
            StatusCode::BAD_REQUEST => return Err(Error::TokenExpired),
            code => {
                let resp_str = http_resp.text().await?;
                return Err(Error::Api { code, resp_str });
            }
        };

        let TokenInfo {
            scope,
            expires_in,
            access_type,
        } = token_info;

        if access_type != ACCESS_TYPE {
            return Err(Error::WrongAccessType { access_type });
        }

        if scope != API_SCOPE {
            return Err(Error::InsufficientScopes { scope });
        }

        let now_timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_secs();
        let expires_at = now_timestamp + expires_in as u64;

        if self.expires_at != expires_at {
            self.expires_at = expires_at;
            self.updated = true;
        }

        Ok(())
    }

    /// Refreshes the access token contained in this set of credentials if its
    /// remaining lifetime is less than [`MINIMUM_TOKEN_LIFETIME`], or if
    /// it has already expired.
    ///
    /// These credentials are guaranteed to be valid (without needing a refresh)
    /// for [`MINIMUM_TOKEN_LIFETIME`] after a call to this method.
    #[instrument(skip_all, name = "(oauth2-refresh-if-necessary)")]
    pub async fn refresh_if_necessary(
        &mut self,
        client: &reqwest::Client,
    ) -> Result<(), Error> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_secs();

        if self.expires_at > now + MINIMUM_TOKEN_LIFETIME.as_secs() {
            // No refresh needed
            trace!("Skipping API token refresh");
            Ok(())
        } else {
            self.refresh(client, now).await
        }
    }

    async fn refresh(
        &mut self,
        client: &reqwest::Client,
        now: u64,
    ) -> Result<(), Error> {
        debug!("Refreshing access token");

        #[derive(Serialize)]
        struct RefreshRequest<'a> {
            grant_type: &'a str,
            client_id: &'a str,
            client_secret: &'a str,
            refresh_token: &'a str,
        }

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: ApiSecret,
            expires_in: u32,
            scope: String,
            token_type: String,
        }

        let req = RefreshRequest {
            grant_type: "refresh_token",
            client_id: &self.client_id.0,
            client_secret: &self.client_secret.0,
            refresh_token: &self.refresh_token.0,
        };

        let http_resp = client
            .post("https://oauth2.googleapis.com/token")
            .json(&req)
            .send()
            .await?;

        let refresh_response = match http_resp.status() {
            StatusCode::OK => http_resp.json::<RefreshResponse>().await?,
            code => {
                let resp_str = http_resp.text().await?;
                return Err(Error::Api { code, resp_str });
            }
        };

        let RefreshResponse {
            access_token,
            expires_in,
            scope,
            token_type,
        } = refresh_response;

        if scope != API_SCOPE {
            return Err(Error::InsufficientScopes { scope });
        }

        if token_type != TOKEN_TYPE {
            return Err(Error::WrongTokenType { token_type });
        }

        let expires_at = now + expires_in as u64;

        self.access_token = access_token;
        self.expires_at = expires_at;
        self.updated = true;

        // For convenience keeping our test tokens up-to-date,
        // print commands to update the corresponding env vars,
        // EXCEPT if SKIP_GDRIVE_TOKEN_PRINT=1 (e.g. in CI)
        #[cfg(test)]
        if !matches!(env::var("SKIP_GDRIVE_TOKEN_PRINT").as_deref(), Ok("1")) {
            let access_token = &self.access_token.0;
            println!("API access token updated; set this in env:");
            println!("```bash");
            println!("export GOOGLE_ACCESS_TOKEN=\"{access_token}\"");
            println!("export GOOGLE_ACCESS_TOKEN_EXPIRY=\"{expires_at}\"");
            println!("```");
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// cargo test -p gdrive -- --ignored test_credentials --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_credentials() {
        let mut credentials = ApiCredentials::from_env().unwrap();
        let client = reqwest::Client::new();

        match credentials.check_token_info(&client).await {
            Ok(_) => (),
            // We mostly just care that the request worked, since the token
            // we're testing with isn't guaranteed to be configured correctly.
            Err(Error::TokenExpired { .. })
            | Err(Error::InsufficientScopes { .. }) => (),
            // The request failed.
            _ => panic!("Validation failed"),
        }

        credentials
            .refresh_if_necessary(&client)
            .await
            .expect("refresh_if_necessary failed");
    }
}
