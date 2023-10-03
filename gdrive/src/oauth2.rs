#[cfg(test)]
use std::env;
use std::time::{Duration, SystemTime};

use common::{api::provision::GDriveCredentials, const_assert};
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

/// Makes a call to Google's `tokeninfo` endpoint to check:
///
/// - The access token has the required scope(s).
/// - The access token has the expected access type.
/// - Google recognizes our access token, and confirms it is not expired.
///
/// We should not need to call this method regularly - just once when the
/// credentials were initially obtained from the user should be enough.
///
/// Returns [`true`] if the token was updated as a result of this call.
#[instrument(skip_all, name = "(oauth2-check-token-info)")]
pub async fn check_token_info(
    client: &reqwest::Client,
    credentials: &mut GDriveCredentials,
) -> Result<bool, Error> {
    debug!("Checking token info");

    #[derive(Deserialize)]
    struct TokenInfo {
        scope: String,
        expires_in: u32,
        access_type: String,
    }

    let http_resp = client
        .post("https://www.googleapis.com/oauth2/v3/tokeninfo")
        .json(&[("access_token", &credentials.access_token)])
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

    let mut updated = false;
    if credentials.expires_at != expires_at {
        credentials.expires_at = expires_at;
        updated = true;
    }

    Ok(updated)
}

/// Refreshes the access token contained in this set of credentials if its
/// remaining lifetime is less than [`MINIMUM_TOKEN_LIFETIME`], or if it has
/// already expired. Returns a [`bool`] representing whether the access token
/// was updated. If so, the credentials should be repersisted in order to avoid
/// an unnecessary refresh in the future.
///
/// These credentials are guaranteed to be valid (without needing a refresh) for
/// [`MINIMUM_TOKEN_LIFETIME`] after a call to this method.
#[instrument(skip_all, name = "(oauth2-refresh-if-necessary)")]
pub async fn refresh_if_necessary(
    client: &reqwest::Client,
    credentials: &mut GDriveCredentials,
) -> Result<bool, Error> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs();

    if credentials.expires_at > now + MINIMUM_TOKEN_LIFETIME.as_secs() {
        // No refresh needed
        trace!("Skipping API token refresh");
        Ok(false)
    } else {
        refresh(client, credentials, now).await.map(|()| true)
    }
}

async fn refresh(
    client: &reqwest::Client,
    credentials: &mut GDriveCredentials,
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
        access_token: String,
        expires_in: u32,
        scope: String,
        token_type: String,
    }

    let req = RefreshRequest {
        grant_type: "refresh_token",
        client_id: &credentials.client_id,
        client_secret: &credentials.client_secret,
        refresh_token: &credentials.refresh_token,
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

    credentials.access_token = access_token;
    credentials.expires_at = expires_at;

    // For convenience keeping our test tokens up-to-date,
    // print commands to update the corresponding env vars,
    // EXCEPT if SKIP_GDRIVE_TOKEN_PRINT=1 (e.g. in CI)
    #[cfg(test)]
    if !matches!(env::var("SKIP_GDRIVE_TOKEN_PRINT").as_deref(), Ok("1")) {
        let access_token = &credentials.access_token;
        println!("API access token updated; set this in env:");
        println!("```bash");
        println!("export GOOGLE_ACCESS_TOKEN=\"{access_token}\"");
        println!("export GOOGLE_ACCESS_TOKEN_EXPIRY=\"{expires_at}\"");
        println!("```");
    }

    Ok(())
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
        let mut credentials = GDriveCredentials::from_env().unwrap();
        let client = reqwest::Client::new();

        match check_token_info(&client, &mut credentials).await {
            Ok(_) => (),
            // We mostly just care that the request worked, since the token
            // we're testing with isn't guaranteed to be configured correctly.
            Err(Error::TokenExpired { .. })
            | Err(Error::InsufficientScopes { .. }) => (),
            // The request failed.
            _ => panic!("Validation failed"),
        }

        refresh_if_necessary(&client, &mut credentials)
            .await
            .expect("refresh_if_necessary failed");
    }
}
