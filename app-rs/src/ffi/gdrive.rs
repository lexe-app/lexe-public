use anyhow::Context;
use common::rng::SysRng;
use flutter_rust_bridge::{frb, RustOpaqueNom};

/// Context required to execute the Google Drive OAuth2 authorization flow.
pub struct GDriveOauth2Flow {
    pub client_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub redirect_uri_scheme: String,
    pub url: String,
}

/// A basic authenticated Google Drive client, before we know which `UserPk`
/// to use.
pub struct GDriveClient {
    pub inner: RustOpaqueNom<GDriveClientInner>,
}

#[allow(dead_code)] // TODO(phlip9): remove
pub(crate) struct GDriveClientInner {
    client: gdrive::ReqwestClient,
    credentials: gdrive::oauth2::GDriveCredentials,
}

impl GDriveOauth2Flow {
    /// Begin the OAuth2 flow for the given mobile `client_id`. We'll also get
    /// a `server_code` we can exchange at the node provision enclave, which
    /// uses `server_client_id`.
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

    /// After the user has authorized access and we've gotten the redirect,
    /// call this fn to exchange the client auth code for credentials + client.
    pub async fn exchange(
        &self,
        result_uri: &str,
    ) -> anyhow::Result<GDriveClient> {
        let code = gdrive::oauth2::parse_redirect_result_uri(result_uri)?;

        // // Uncomment while debugging client auth
        // tracing::info!("export GOOGLE_AUTH_CODE=\"{code}\"");

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

        // // Uncomment while debugging server auth
        // {
        //     let server_code = credentials.server_code.unwrap();
        //     tracing::info!("export GOOGLE_AUTH_CODE=\"{server_code}\"");
        // }

        Ok(GDriveClient {
            inner: RustOpaqueNom::new(GDriveClientInner {
                client,
                credentials,
            }),
        })
    }
}

impl GDriveClient {
    #[frb(sync)]
    pub fn server_code(&self) -> Option<String> {
        self.inner.credentials.server_code.clone()
    }
}
