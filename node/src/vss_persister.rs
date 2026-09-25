use std::net::IpAddr;

use anyhow::{Context, ensure};
use lexe_api::{cli::node::VssProviderConfig, rest::RestClient};
use lexe_common::{constants, env::DeployEnv, root_seed::RootSeed};
use lexe_crypto::rng::Crng;
use lexe_tls::rustls;
use lexe_tls_attest_server::{self as tls_attest, NodeMode};
use lexe_vss_client::{
    KeyValue, NO_VERSION_CHECK, PutObjectRequest, VssClient,
};
use reqwest::{
    Url,
    header::{self, HeaderMap, HeaderName, HeaderValue},
    redirect,
};
use tokio::time;

use crate::{backup_persister::BackupBatch, client::USER_AGENT_EXTERNAL};

#[cfg(test)]
mod tests;

/// A backup persister authenticated to a specific user for use at a single VSS
/// provider.
///
/// ### Authentication
///
/// Users derive separate VSS auth keys per provider, so backups stored at one
/// provider are not easily transferable to another provider. Instead, they
/// must be re-persisted by the user.
///
/// See: [`RootSeed::derive_vss_auth_key`] for more details.
pub(crate) struct VssPersister {
    provider_name: String,
    client: VssClient,
}

/// A provider's HTTP client, shared across usernodes.
pub(crate) struct VssProvider {
    name: String,
    base_url: Url,
    http_client: reqwest::Client,
}

impl VssPersister {
    /// Create a persister authenticated for this user and VSS provider.
    pub(crate) fn new(root_seed: &RootSeed, provider: &VssProvider) -> Self {
        let hostname =
            provider.base_url.host_str().expect("VSS URL has a host");
        let client = VssClient::from_client(
            provider.base_url.as_str(),
            provider.http_client.clone(),
            root_seed.derive_vss_auth_key(hostname),
        );
        Self {
            provider_name: provider.name.clone(),
            client,
        }
    }

    /// The configured provider name.
    pub(crate) fn provider_name(&self) -> &str {
        &self.provider_name
    }

    /// Persist a [`BackupBatch`] to the remote VSS server (writes+deletes).
    pub(crate) async fn persist(
        &self,
        files: &mut BackupBatch,
    ) -> anyhow::Result<()> {
        let request = Self::build_request(files);
        time::timeout(
            constants::timeout::TRANSIENT_ERROR_TOLERANCE,
            self.client.put_object(&request),
        )
        .await
        .context("VSS backup timed out")?
        .context("VSS backup failed")?;
        Ok(())
    }

    fn build_request(files: &mut BackupBatch) -> PutObjectRequest {
        let mut request = PutObjectRequest {
            store_id: constants::VSS_STORE_ID.to_owned(),
            global_version: None,
            transaction_items: Vec::new(),
            delete_items: Vec::new(),
        };
        for (id, data) in files.drain() {
            let (value, items) = match data {
                Some(value) => (value, &mut request.transaction_items),
                // Note small VSS API quirk where deletes still take a
                // non-optional `value: Vec<u8>`
                None => (Vec::new(), &mut request.delete_items),
            };
            items.push(KeyValue {
                key: id.to_string(),
                version: NO_VERSION_CHECK,
                value,
            });
        }
        request
    }
}

impl VssProvider {
    /// Validate the config and build the provider's shared HTTP client.
    pub(crate) fn new(
        rng: &mut impl Crng,
        config: &VssProviderConfig,
        deploy_env: DeployEnv,
    ) -> anyhow::Result<Self> {
        let url = Self::parse_url(&config.url)?;
        let tls_config = Self::tls_config(rng, &url, deploy_env)?;
        let http_client = Self::client_builder(config)?
            .use_preconfigured_tls(tls_config)
            .build()
            .context("Failed to build VSS HTTP client")?;
        Ok(Self {
            name: config.name.clone(),
            base_url: url,
            http_client,
        })
    }

    fn parse_url(url: &str) -> anyhow::Result<Url> {
        let url = Url::parse(url).context("Invalid VSS URL")?;
        ensure!(
            url.scheme() == "https" && url.host_str().is_some(),
            "VSS URL must use HTTPS and have a host"
        );
        ensure!(
            url.username().is_empty() && url.password().is_none(),
            "VSS URL must not contain credentials"
        );
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "VSS URL must not contain a query or fragment"
        );
        Ok(url)
    }

    fn client_builder(
        config: &VssProviderConfig,
    ) -> anyhow::Result<reqwest::ClientBuilder> {
        let mut headers = HeaderMap::new();
        for (name, value) in &config.headers {
            let name = HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("Invalid VSS header name {name:?}"))?;
            ensure!(
                name != header::AUTHORIZATION,
                "VSS Authorization header is reserved for user signatures"
            );
            let mut value =
                HeaderValue::from_str(value).with_context(|| {
                    format!("Invalid VSS header value for {name}")
                })?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
        let client = RestClient::client_builder(USER_AGENT_EXTERNAL)
            .default_headers(headers)
            // Custom credentials must not follow redirects to other hosts.
            .redirect(redirect::Policy::none());
        Ok(client)
    }

    fn tls_config(
        rng: &mut impl Crng,
        url: &Url,
        deploy_env: DeployEnv,
    ) -> anyhow::Result<rustls::ClientConfig> {
        let host = url.host_str().context("Missing VSS host")?;
        if Self::uses_lexe_tls(host, deploy_env) {
            return tls_attest::node_lexe_client_config(
                rng,
                deploy_env,
                NodeMode::Run,
            )
            .context("Failed to build VSS client TLS config");
        }

        // External providers need WebPKI and general-purpose algorithms.
        #[allow(clippy::disallowed_methods)]
        let mut config = rustls::ClientConfig::builder_with_protocol_versions(
            lexe_tls::LEXE_TLS_PROTOCOL_VERSIONS,
        )
        .with_root_certificates(lexe_tls::WEBPKI_ROOT_CERTS.clone())
        .with_no_client_auth();
        config.alpn_protocols = lexe_tls::LEXE_ALPN_PROTOCOLS.clone();
        Ok(config)
    }

    fn uses_lexe_tls(host: &str, deploy_env: DeployEnv) -> bool {
        let host = host.strip_suffix('.').unwrap_or(host);
        let is_loopback = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        host == "lexe.app"
            || host.ends_with(".lexe.app")
            || host.ends_with(".lx")
            || (deploy_env.is_dev() && is_loopback)
    }
}
