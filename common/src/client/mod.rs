// TODO

pub mod certs;
pub mod tls;

use anyhow::{format_err, Context};
use bitcoin::secp256k1::PublicKey;
use serde::{Deserialize, Serialize};

use crate::api::provision::ProvisionRequest;
use crate::api::UserPk;
use crate::attest;
use crate::rng::Crng;
use crate::root_seed::RootSeed;

#[derive(Debug, Deserialize, Serialize)]
pub struct NodeInfo {
    pub node_pk: PublicKey,
    pub num_channels: usize,
    pub num_usable_channels: usize,
    pub local_balance_msat: u64,
    pub num_peers: usize,
}

pub struct NodeClient {
    client: reqwest::Client,
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

        Ok(Self {
            client,
            provision_url,
            run_url,
        })
    }

    pub async fn provision(&self, req: ProvisionRequest) -> anyhow::Result<()> {
        let provision_url = &self.provision_url;
        let url = format!("{provision_url}/provision");

        let resp = self.client.post(url).json(&req).send().await?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let err_txt = resp.text().await?;
            Err(format_err!("response error: {err_txt}"))
        }
    }

    pub async fn node_info(&self) -> anyhow::Result<NodeInfo> {
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/node_info");

        let resp = self.client.get(url).send().await?;

        if resp.status().is_success() {
            resp.json().await.map_err(Into::into)
        } else {
            let err_txt = resp.text().await?;
            Err(format_err!("response error: {err_txt}"))
        }
    }
}
