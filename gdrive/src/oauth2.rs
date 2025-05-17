#[cfg(test)]
use std::env;
use std::{
    fmt,
    ops::Deref,
    time::{Duration, SystemTime},
};

use base64::Engine;
#[cfg(test)]
use common::test_utils::arbitrary;
use common::{
    constants,
    rng::{Crng, RngExt},
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, trace};

use crate::{Error, API_SCOPE};

/// The default timeout for requests to Google APIs
const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The expected value of `access_type`.
// 'offline' tells Google to give us refresh token that allows us to refresh the
// access token while the user is offline.
const ACCESS_TYPE: &str = "offline";
/// The expected value of `token_type`.
// For the foreseeable future we are only interested in bearer auth tokens.
const TOKEN_TYPE: &str = "Bearer";
/// The minimum amount of time that access tokens are guaranteed to be valid
/// after a call to `refresh_if_necessary`. If an access token will expire in
/// time less than this (or if the token has already expired),
/// `refresh_if_necessary` will refresh the token.
pub const MINIMUM_TOKEN_LIFETIME: Duration = Duration::from_secs(60);
// Newly refreshed access tokens usually live for only 3600 seconds
lexe_std::const_assert!(MINIMUM_TOKEN_LIFETIME.as_secs() < 3600);

/// A newtype for [`reqwest::Client`] which ensures that any passed-in clients
/// have TLS, timeouts etc configured correctly for Google Drive.
#[derive(Clone)]
pub struct ReqwestClient(reqwest::Client);

impl Deref for ReqwestClient {
    type Target = reqwest::Client;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ReqwestClient {
    #[allow(clippy::new_without_default)] // TODO(max): How to disable this?
    pub fn new() -> Self {
        let gts_r1_ca =
            reqwest::Certificate::from_der(constants::GTS_ROOT_R1_CA_CERT_DER)
                .expect("Checked in tests");
        let gts_r2_ca =
            reqwest::Certificate::from_der(constants::GTS_ROOT_R2_CA_CERT_DER)
                .expect("Checked in tests");
        let gts_r3_ca =
            reqwest::Certificate::from_der(constants::GTS_ROOT_R3_CA_CERT_DER)
                .expect("Checked in tests");
        let gts_r4_ca =
            reqwest::Certificate::from_der(constants::GTS_ROOT_R4_CA_CERT_DER)
                .expect("Checked in tests");
        let gs_r4_ca =
            reqwest::Certificate::from_der(constants::GS_ROOT_R4_CA_CERT_DER)
                .expect("Checked in tests");
        reqwest::Client::builder()
            .https_only(true)
            .add_root_certificate(gts_r1_ca)
            .add_root_certificate(gts_r2_ca)
            .add_root_certificate(gts_r3_ca)
            .add_root_certificate(gts_r4_ca)
            .add_root_certificate(gs_r4_ca)
            .timeout(API_REQUEST_TIMEOUT)
            .build()
            .map(Self)
            .expect("Failed to build ReqwestClient")
    }
}

/// A complete set of OAuth2 credentials which allows making requests to the
/// Google Drive v3 API and periodically refreshing the contained access token.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Arbitrary))]
pub struct GDriveCredentials {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_id: String,

    /// Mobile clients (Android, iOS, macOS) don't have a `client_secret`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_option_string()"))]
    pub client_secret: Option<String>,

    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub refresh_token: String,

    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub access_token: String,

    /// Mobile clients (Android, iOS, macOS) may also request the server auth
    /// code that will be passed to the node enclave during provision. We get
    /// this via the `audience=<server_client_id>` parameter in the redirect
    /// URI during authz.
    ///
    /// This field is also not persisted, since it's only used once.
    #[serde(skip)]
    #[cfg_attr(test, proptest(strategy = "proptest::strategy::Just(None)"))]
    pub server_code: Option<String>,

    /// Unix timestamp (in seconds) at which the current access token expires.
    /// Set to 0 if unknown; the tokens will just be refreshed at next use.
    pub expires_at: u64,
}

impl GDriveCredentials {
    /// Attempts to construct an [`GDriveCredentials`] from env.
    ///
    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>" # Optional, depending on client
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_SERVER_CODE="<server_code>" # Optional, depending on client
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// ```
    #[cfg(test)]
    pub fn from_env() -> anyhow::Result<Self> {
        use std::str::FromStr;

        use anyhow::Context;

        let client_id = env::var("GOOGLE_CLIENT_ID")
            .context("Missing 'GOOGLE_CLIENT_ID' in env")?;
        let client_secret = env::var("GOOGLE_CLIENT_SECRET").ok();
        let refresh_token = env::var("GOOGLE_REFRESH_TOKEN")
            .context("Missing 'GOOGLE_REFRESH_TOKEN' in env")?;
        let access_token = env::var("GOOGLE_ACCESS_TOKEN")
            .context("Missing 'GOOGLE_ACCESS_TOKEN' in env")?;
        let server_code = env::var("GOOGLE_SERVER_CODE").ok();
        let expires_at_str = env::var("GOOGLE_ACCESS_TOKEN_EXPIRY")
            .context("Missing 'GOOGLE_ACCESS_TOKEN_EXPIRY' in env")?;
        let expires_at = u64::from_str(&expires_at_str)
            .context("Invalid GOOGLE_ACCESS_TOKEN_EXPIRY")?;

        Ok(Self {
            client_id,
            client_secret,
            refresh_token,
            access_token,
            server_code,
            expires_at,
        })
    }
}

impl fmt::Debug for GDriveCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let client_id = &self.client_id;
        let expires_at = &self.expires_at;
        write!(
            f,
            "GDriveCredentials {{ \
                client_id: {client_id}, \
                expires_at: {expires_at}, \
                .. \
            }}"
        )
    }
}

//
// [Step 1: Generate a code verifier and challenge](https://developers.google.com/identity/protocols/oauth2/native-app#step1-code-verifier)
//

/// OAuth2 "Proof Key for Code Exchange" impl.
///
/// Used by public OAuth2 clients to secure the OAuth2 flow. PKCE is effectively
/// a commitment scheme that prevents an attacker who successfully intercepts
/// the client auth code after redirect from being able to exchange for an
/// access+refresh token, since they don't know the committed `code_verifier`.
///
/// See: [RFC 7636](https://datatracker.ietf.org/doc/html/rfc7636)
#[cfg_attr(test, derive(Debug, PartialEq))]
pub struct OAuth2PkceCodeChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
}

impl OAuth2PkceCodeChallenge {
    const METHOD: &'static str = "S256";

    /// Generate a new PKCE verifier and challenge.
    ///
    /// ```not_rust
    /// code-verifier := <alphanum>*64
    /// code-challenge := base64url-unpadded(sha256(code-verifier))
    /// ```
    pub fn from_rng(rng: &mut impl Crng) -> Self {
        Self::from_code_verifier(Self::gen_code_verifier(rng))
    }

    fn gen_code_verifier(rng: &mut impl Crng) -> String {
        String::from_utf8(rng.gen_alphanum_bytes::<48>().to_vec())
            .expect("Always valid string")
    }

    fn from_code_verifier(code_verifier: String) -> Self {
        let code_challenge = sha256::digest(code_verifier.as_bytes());
        let code_challenge = base64::prelude::BASE64_URL_SAFE_NO_PAD
            .encode(code_challenge.as_slice());
        Self {
            code_verifier,
            code_challenge,
        }
    }
}

//
// [Step 2: Send a request to Google's OAuth 2.0 server](https://developers.google.com/identity/protocols/oauth2/native-app#step-2:-send-a-request-to-googles-oauth-2.0-server)
//

/// Build the URL that the user's device should navigate to. Assuming the user
/// consents to the permissions (`scope`) we request, they'll be redirected to
/// `redirect_uri`, which should contain our client auth code in its query
/// params.
pub fn auth_code_url(
    client_id: &str,
    server_client_id: Option<&str>,
    redirect_uri: &str,
    code_challenge: &str,
) -> String {
    #[derive(Serialize)]
    struct QueryParams<'a> {
        client_id: &'a str,
        redirect_uri: &'a str,
        response_type: &'static str,
        code_challenge: &'a str,
        code_challenge_method: &'static str,
        scope: &'static str,
        access_type: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        audience: Option<&'a str>,
    }

    let query_params = QueryParams {
        client_id,
        redirect_uri,
        response_type: "code",
        code_challenge,
        code_challenge_method: OAuth2PkceCodeChallenge::METHOD,
        scope: API_SCOPE,
        access_type: "offline",
        // Include this field so we also receive the server auth code when we
        // exchange for a token below. This server auth code is what the enclave
        // uses to exchange for its auth token and long-term refresh token.
        audience: server_client_id,
    };

    let query_string = serde_urlencoded::to_string(query_params)
        .expect("Failed to serialize URL");

    format!("https://accounts.google.com/o/oauth2/v2/auth?{query_string}")
}

//
// [Step 3: Google prompts user for consent](https://developers.google.com/identity/protocols/oauth2/native-app#handlingresponse)
// [Step 4: Handle the OAuth 2.0 server response](https://developers.google.com/identity/protocols/oauth2/native-app#handlingresponse)
//

/// Parse the client auth code from the redirect result URI's query parameters.
///
/// We get this URI when the user gets redirected to our `redirect_uri` after
/// authorizing our gdrive access.
pub fn parse_redirect_result_uri(
    redirect_result_uri: &str,
) -> Result<&str, Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Response<'a> {
        Code {
            code: &'a str,
        },
        Error {
            error: &'a str,
            error_description: String,
        },
    }

    let (_, query_params) = redirect_result_uri
        .split_once('?')
        .ok_or(Error::RedirectIsNotUri)?;

    let response = serde_urlencoded::from_str::<Response>(query_params)?;

    match response {
        Response::Code { code } => Ok(code),
        Response::Error {
            error,
            error_description,
        } => Err(Error::RedirectError(format!(
            "{error}: {error_description}"
        ))),
    }
}

//
// [Step 5: Exchange authorization code for refresh and access tokens](https://developers.google.com/identity/protocols/oauth2/native-app#exchange-authorization-code)
//

/// Exchanges an auth `code` (and other info) for an `access_token`. Returns
/// [`GDriveCredentials`] which can then be persisted.
///
/// <https://developers.google.com/identity/protocols/oauth2/native-app#exchange-authorization-code>
pub async fn auth_code_for_token(
    client: &ReqwestClient,
    client_id: &str,
    client_secret: Option<&str>,
    redirect_uri: &str,
    code: &str,
    code_verifier: Option<&str>,
) -> Result<GDriveCredentials, Error> {
    #[derive(Serialize)]
    struct Request<'a> {
        client_id: &'a str,
        // For mobile clients this is `None`. For enclave provision this is
        // `Some`.
        #[serde(skip_serializing_if = "Option::is_none")]
        client_secret: Option<&'a str>,
        redirect_uri: &'a str,
        code: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        code_verifier: Option<&'a str>,
        grant_type: &'static str,
    }

    #[derive(Deserialize)]
    struct Response {
        refresh_token: String,
        access_token: String,
        expires_in: u32,
        scope: String,
        token_type: String,
        // We should get a `server_code` during mobile client auth code
        // exchange, but not during enclave provision server auth code
        // exchange.
        server_code: Option<String>,
    }

    let request = Request {
        client_id,
        client_secret,
        redirect_uri,
        code,
        code_verifier,
        grant_type: "authorization_code",
    };

    let http_resp = client
        .post("https://oauth2.googleapis.com/token")
        .json(&request)
        .send()
        .await?;

    let code = http_resp.status();
    let response = if code.is_success() {
        http_resp.json::<Response>().await?
    } else {
        let resp_str = match http_resp.bytes().await {
            Ok(b) => String::from_utf8_lossy(&b).to_string(),
            Err(e) => format!("Failed to get error response text: {e:#}"),
        };
        return Err(Error::Api { code, resp_str });
    };

    let Response {
        refresh_token,
        access_token,
        expires_in,
        scope,
        token_type,
        server_code,
    } = response;

    // Validate response fields
    if token_type != TOKEN_TYPE {
        return Err(Error::WrongTokenType { token_type });
    }
    // Ensure we were actually granted the required gdrive API scope
    verify_response_scope(scope)?;

    let now_timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs();
    let expires_at = now_timestamp + expires_in as u64;

    Ok(GDriveCredentials {
        client_id: client_id.to_owned(),
        client_secret: client_secret.map(str::to_owned),
        refresh_token,
        access_token,
        server_code,
        expires_at,
    })
}

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
    client: &ReqwestClient,
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
            let resp_str = match http_resp.bytes().await {
                Ok(b) => String::from_utf8_lossy(&b).to_string(),
                Err(e) => format!("Failed to get error response text: {e:#}"),
            };
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
    // Ensure we were actually granted the required gdrive API scope
    verify_response_scope(scope)?;

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
    client: &ReqwestClient,
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
    client: &ReqwestClient,
    credentials: &mut GDriveCredentials,
    now: u64,
) -> Result<(), Error> {
    debug!("Refreshing access token");

    #[derive(Serialize)]
    struct RefreshRequest<'a> {
        grant_type: &'a str,
        client_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        client_secret: Option<&'a str>,
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
        client_secret: credentials.client_secret.as_deref(),
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

    if token_type != TOKEN_TYPE {
        return Err(Error::WrongTokenType { token_type });
    }
    // Ensure we were actually granted the required gdrive API scope
    verify_response_scope(scope)?;

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

/// Verify that the GDrive OAuth response has a scope that contains our required
/// [`API_SCOPE`].
fn verify_response_scope(scope: String) -> Result<(), Error> {
    if scope.split(' ').any(|s| s == API_SCOPE) {
        Ok(())
    } else {
        Err(Error::InsufficientScopes { scope })
    }
}

#[cfg(test)]
mod test {
    use common::{rng::FastRng, test_utils::roundtrip};

    use super::*;

    #[test]
    fn auth_code_url_snapshot() {
        let client_id = "495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5.apps.googleusercontent.com";
        let server_client_id = Some("495704988639-19bfg8k5f3runiio4apbicpounc10gh1.apps.googleusercontent.com");
        let redirect_uri = "com.googleusercontent.apps.495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5:/";
        let code_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        let actual = auth_code_url(
            client_id,
            server_client_id,
            redirect_uri,
            code_challenge,
        );
        let expected = "\
            https://accounts.google.com/o/oauth2/v2/auth\
              ?client_id=495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5.apps.googleusercontent.com\
              &redirect_uri=com.googleusercontent.apps.495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5%3A%2F\
              &response_type=code\
              &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM\
              &code_challenge_method=S256\
              &scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file\
              &access_type=offline\
              &audience=495704988639-19bfg8k5f3runiio4apbicpounc10gh1.apps.googleusercontent.com\
        ";
        assert_eq!(actual, expected);

        let server_client_id = None;
        let actual = auth_code_url(
            client_id,
            server_client_id,
            redirect_uri,
            code_challenge,
        );
        let expected = "\
            https://accounts.google.com/o/oauth2/v2/auth\
              ?client_id=495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5.apps.googleusercontent.com\
              &redirect_uri=com.googleusercontent.apps.495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5%3A%2F\
              &response_type=code\
              &code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM\
              &code_challenge_method=S256\
              &scope=https%3A%2F%2Fwww.googleapis.com%2Fauth%2Fdrive.file\
              &access_type=offline\
        ";
        assert_eq!(actual, expected);
    }

    #[test]
    fn pkce_snapshot() {
        let mut rng = FastRng::from_u64(654984984);
        let actual = OAuth2PkceCodeChallenge::from_rng(&mut rng);
        let expected = OAuth2PkceCodeChallenge {
            code_verifier: "Im1AGo673tWX11XcfKt5Aog51PV3ZTZt2qeoWXWidR5DgsfD"
                .to_owned(),
            code_challenge: "wtX8v-ik3YSY1DsKdfNG4r9rphH9QZL2v68gg8JIXz8"
                .to_owned(),
        };
        assert_eq!(actual, expected);
    }

    #[test]
    fn pkce_test_vector() {
        // From: <https://datatracker.ietf.org/doc/html/rfc7636#appendix-B>
        let expected = OAuth2PkceCodeChallenge {
            code_verifier: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
                .to_owned(),
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
                .to_owned(),
        };
        let actual = OAuth2PkceCodeChallenge::from_code_verifier(
            expected.code_verifier.clone(),
        );
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_parse_redirect_result_uri() {
        let uri = "com.googleusercontent.apps.495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5:/?code=4/sdfm5981MI3h-9z_84sodifjOSIDJFsdlfkjsdf4849645h-LisED14891q5s_849844A7A&scope=https://www.googleapis.com/auth/drive.file";
        let actual = parse_redirect_result_uri(uri);
        let expected = "4/sdfm5981MI3h-9z_84sodifjOSIDJFsdlfkjsdf4849645h-LisED14891q5s_849844A7A";
        assert_eq!(actual.unwrap(), expected);

        let uri = "com.googleusercontent.apps.495704988639-2rqsnvobrvlnbkqdin38q2r3cph537l5:/?error=invalid_request&error_description=Unsupported%20operation";
        let actual = parse_redirect_result_uri(uri);
        match actual {
            Err(Error::RedirectError(msg)) =>
                assert_eq!(msg, "invalid_request: Unsupported operation"),
            _ => panic!("unexpected: {actual:?}"),
        }
    }

    #[test]
    fn credentials_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<GDriveCredentials>();
    }

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
        let client = ReqwestClient::new();

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

    /// Tests the [`auth_code_for_token`] function.
    ///
    /// Running this test requires:
    /// 1) Passing `--ignored` to `cargo test`
    /// 2) Setting the `GOOGLE_AUTH_CODE` env var (must be `export`ed)
    ///
    /// If `GOOGLE_AUTH_CODE` is missing, this test will pass silently.
    /// This is because `#[ignore]`d tests are run in CI, but it is non-trivial
    /// to programmatically obtain an auth code in order to run this test.
    ///
    /// Full instructions for running this test:
    ///
    /// ```bash
    /// # Get these from a Lexe dev. You may need to be added a test user.
    /// export GOOGLE_CLIENT_ID="<CLIENT_ID>"
    /// export GOOGLE_CLIENT_SECRET="<CLIENT_SECRET>"
    ///
    /// # Run this block and follow the instructions:
    /// (
    ///     base_url="https://accounts.google.com/o/oauth2/v2/auth"
    ///     # client_id and redirect_uri are configured in the
    ///     # 'Lexe "web" OAuth client' in Google Cloud, for testing only
    ///     client_id="$GOOGLE_CLIENT_ID"
    ///     redirect_uri="https://localhost:6969/bogus"
    ///     # Tell Google to give us an authorization code.
    ///     response_type="code"
    ///     # The auth/drive.file permissions.
    ///     # Everything else is empirically observed from the actual mobile app
    ///     # auth flow.
    ///     scope="https://www.googleapis.com/auth/drive.file openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile"
    ///     access_type="offline"
    ///
    ///     urlencode() {
    ///         jq -Rr '@uri' <<< "$1"
    ///     }
    ///     encoded_url="${base_url}?client_id=${client_id}&redirect_uri=$(urlencode "$redirect_uri")&response_type=$(urlencode "$response_type")&scope=$(urlencode "$scope")&access_type=${access_type}"
    ///     
    ///     echo
    ///     echo "Visit this url in your browser, logging in with your lexe.app email:"
    ///     echo "$encoded_url"
    ///     echo
    ///     echo "Then note the value of the 'code' parameter after the bogus redirect"
    /// )
    ///
    /// # Save the url-encoded 'code' param to an env var, then decode it.
    /// URL_ENCODED_AUTH_CODE="<CODE>"
    /// export GOOGLE_AUTH_CODE=$(python3 -c "import sys, urllib.parse as ul; print(ul.unquote_plus(sys.argv[1]))" $URL_ENCODED_AUTH_CODE)
    /// echo "Decoded auth code: $GOOGLE_AUTH_CODE"
    ///
    /// # Finally, run the test
    /// cargo test -p gdrive -- --ignored test_auth_code_for_token --show-output
    /// ```
    #[ignore]
    #[tokio::test]
    async fn test_auth_code_for_token() {
        // Skip the test body if "GOOGLE_AUTH_CODE" was not set
        let code = match env::var("GOOGLE_AUTH_CODE") {
            Ok(c) => c,
            Err(_) => return,
        };
        let client_id = env::var("GOOGLE_CLIENT_ID")
            .expect("This test requires GOOGLE_CLIENT_ID to be set");
        let client_secret = env::var("GOOGLE_CLIENT_SECRET").ok();
        let redirect_uri = env::var("GOOGLE_REDIRECT_URI")
            .unwrap_or_else(|_| "https://localhost:6969/bogus".to_owned());

        let code_verifier = None;

        let client = ReqwestClient::new();
        let credentials = auth_code_for_token(
            &client,
            &client_id,
            client_secret.as_deref(),
            &redirect_uri,
            &code,
            code_verifier,
        )
        .await
        .unwrap();

        // Convenience to make it easier to run other gdrive tests
        if !matches!(env::var("SKIP_GDRIVE_TOKEN_PRINT").as_deref(), Ok("1")) {
            let refresh_token = &credentials.refresh_token;
            let access_token = &credentials.access_token;
            let expires_at = &credentials.expires_at;
            println!("Received API credentials; set this in env:");
            println!("```bash");
            println!("export GOOGLE_REFRESH_TOKEN=\"{refresh_token}\"");
            println!("export GOOGLE_ACCESS_TOKEN=\"{access_token}\"");
            println!("export GOOGLE_ACCESS_TOKEN_EXPIRY=\"{expires_at}\"");
            println!("```");
        }
    }
}
