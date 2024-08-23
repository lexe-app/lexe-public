use anyhow::Context;
use common::rng::SysRng;
use flutter_rust_bridge::frb;

pub struct GDriveOauth2Flow {
    pub client_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub redirect_uri_scheme: String,
    pub url: String,
}

impl GDriveOauth2Flow {
    #[frb(sync)]
    pub fn init(client_id: String, server_client_id: &str) -> Self {
        let pkce =
            gdrive::oauth2::Oauth2PkceCodeChallenge::gen(&mut SysRng::new());

        // TODO(phlip9): Linux and Windows need to provide their own
        // `http://localhost:{port}` redirect URI.

        // Mobile clients use a "custom URI scheme", which is just their client
        // id with the DNS name segments reversed.
        let redirect_uri_scheme = client_id
            .as_str()
            .split('.')
            .rev()
            .collect::<Vec<_>>()
            .join(".");
        let redirect_uri = format!("{redirect_uri_scheme}:/");

        let url = gdrive::oauth2::auth_code_url(
            &client_id,
            Some(server_client_id),
            &redirect_uri,
            &pkce.code_challenge,
        );

        Self {
            client_id,
            code_verifier: pkce.code_verifier,
            redirect_uri,
            redirect_uri_scheme,
            url,
        }
    }

    pub async fn exchange(&self, result_uri: &str) -> anyhow::Result<String> {
        let code = gdrive::oauth2::parse_redirect_result_uri(result_uri)?;

        // // Uncomment while debugging
        // tracing::info!("code: {code}");

        let client = gdrive::oauth2::ReqwestClient::new();
        let client_secret = None;
        let credentials = gdrive::oauth2::auth_code_for_token(
            &client,
            &self.client_id,
            client_secret,
            &self.redirect_uri,
            code,
            Some(&self.code_verifier),
        )
        .await
        .context("Auth code exchange failed")?;

        let server_code = credentials.server_code.context(
            "Auth code exchange response is missing the `server_code`",
        )?;

        // // Uncomment while debugging
        // tracing::info!("export GOOGLE_AUTH_CODE=\"{server_code}\"");

        Ok(server_code)
    }
}
