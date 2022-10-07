// TODO

pub mod certs;
/// TLS configurations for the client to the node.
pub mod tls;

use anyhow::Context;
use async_trait::async_trait;

use crate::api::command::{GetInvoiceRequest, ListChannels, NodeInfo};
use crate::api::def::{OwnerNodeProvisionApi, OwnerNodeRunApi};
use crate::api::error::NodeApiError;
use crate::api::provision::NodeProvisionRequest;
use crate::api::qs::EmptyData;
use crate::api::rest::{RestClient, GET, POST};
use crate::api::UserPk;
use crate::attest;
use crate::ln::invoice::LxInvoice;
use crate::rng::Crng;
use crate::root_seed::RootSeed;

pub struct NodeClient {
    rest: RestClient,
    provision_url: String,
    run_url: String,
}

impl NodeClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new<R: Crng>(
        rng: &mut R,
        seed: &RootSeed,
        user_pk: &UserPk,
        proxy_url: &str,
        proxy_ca: &rustls::Certificate,
        attest_verifier: attest::ServerCertVerifier,
        provision_url: String,
        run_url: String,
    ) -> anyhow::Result<Self> {
        // TODO(phlip9): actual auth in proxy header
        // TODO(phlip9): https only mode

        let proxy = reqwest::Proxy::https(proxy_url)
            .context("Invalid proxy url")?
            // TODO(phlip9): should be bearer auth
            .basic_auth(&user_pk.to_string(), "");

        let tls = tls::client_tls_config(rng, proxy_ca, seed, attest_verifier)?;

        let client = reqwest::Client::builder()
            .proxy(proxy)
            .user_agent("lexe-client")
            .use_preconfigured_tls(tls)
            .build()
            .context("Failed to build client")?;

        let rest = RestClient::from_preconfigured_client(client);

        Ok(Self {
            rest,
            provision_url,
            run_url,
        })
    }
}

#[async_trait]
impl OwnerNodeProvisionApi for NodeClient {
    async fn provision(
        &self,
        data: NodeProvisionRequest,
    ) -> Result<(), NodeApiError> {
        let provision_url = &self.provision_url;
        let url = format!("{provision_url}/provision");

        self.rest.request(POST, url, &data).await
    }
}

#[async_trait]
impl OwnerNodeRunApi for NodeClient {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/node_info");
        let data = EmptyData {};

        self.rest.request(GET, url, &data).await
    }

    async fn list_channels(&self) -> Result<ListChannels, NodeApiError> {
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/channels");
        let data = EmptyData {};

        self.rest.request(GET, url, &data).await
    }

    async fn get_invoice(
        &self,
        req: GetInvoiceRequest,
    ) -> Result<LxInvoice, NodeApiError> {
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/get_invoice");

        self.rest.request(POST, url, &req).await
    }
}
