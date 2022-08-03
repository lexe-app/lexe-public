// TODO

use anyhow::{Context, Result};
use reqwest::{Client, Proxy};

use crate::api::provision::ProvisionRequest;
use crate::api::UserPk;
use crate::attest::EnclavePolicy;
use crate::client::tls;

pub struct ProvisionClient {
    client: Client,
    provision_url: String,
}

impl ProvisionClient {
    pub fn new(
        user_pk: &UserPk,
        proxy_url: &str,
        proxy_ca: &rustls::Certificate,
        provision_url: String,
        expect_dummy_quote: bool,
        enclave_policy: EnclavePolicy,
    ) -> Result<Self> {
        // TODO(phlip9): actual auth in proxy header
        // TODO(phlip9): https only mode

        let proxy = Proxy::https(proxy_url)
            .context("Invalid proxy url")?
            // TODO(phlip9): should be bearer auth
            .basic_auth(&user_pk.to_string(), "");

        let tls = tls::client_provision_tls_config(
            proxy_ca,
            expect_dummy_quote,
            enclave_policy,
        )?;

        let client = Client::builder()
            .proxy(proxy)
            .user_agent("lexe-provision-client")
            .use_preconfigured_tls(tls)
            .build()
            .context("Failed to build client")?;

        Ok(Self {
            client,
            provision_url,
        })
    }

    pub async fn provision(&self, req: ProvisionRequest) -> Result<()> {
        let provision_url = &self.provision_url;
        let url = format!("{provision_url}/provision");

        let resp = self.client.post(url).json(&req).send().await?;

        Ok(resp.error_for_status().map(|_| ())?)
    }
}
