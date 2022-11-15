// TODO

pub mod certs;
/// TLS configurations for the client to the node.
pub mod tls;

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::{IntoProxyScheme, Url};

use crate::api::auth::{
    UserAuthRequest, UserAuthResponse, UserAuthenticator, UserSignupRequest,
};
use crate::api::command::{GetInvoiceRequest, ListChannels, NodeInfo};
use crate::api::def::{OwnerNodeProvisionApi, OwnerNodeRunApi, UserBackendApi};
use crate::api::error::{BackendApiError, NodeApiError, NodeErrorKind};
use crate::api::provision::NodeProvisionRequest;
use crate::api::rest::{RequestBuilderExt, RestClient, GET, POST};
use crate::ln::invoice::LxInvoice;
use crate::rng::Crng;
use crate::root_seed::RootSeed;
use crate::{attest, ed25519};

pub struct NodeClient {
    rest: RestClient,
    gateway_url: String,
    provision_url: &'static str,
    run_url: &'static str,
    authenticator: Arc<UserAuthenticator>,
}

impl NodeClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new<R: Crng>(
        rng: &mut R,
        seed: &RootSeed,
        authenticator: Arc<UserAuthenticator>,
        gateway_url: String,
        gateway_ca: &rustls::Certificate,
        attest_verifier: attest::ServerCertVerifier,
        provision_url: &'static str,
        run_url: &'static str,
    ) -> anyhow::Result<Self> {
        let proxy = Self::proxy_config(
            &gateway_url,
            provision_url,
            run_url,
            authenticator.clone(),
        )
        .context("Invalid proxy config")?;

        let tls =
            tls::client_tls_config(rng, gateway_ca, seed, attest_verifier)?;

        let client = reqwest::Client::builder()
            .proxy(proxy)
            .user_agent("lexe-client")
            .use_preconfigured_tls(tls)
            .build()
            .context("Failed to build client")?;

        let rest = RestClient::from_preconfigured_client(client);

        Ok(Self {
            rest,
            gateway_url,
            provision_url,
            run_url,
            authenticator,
        })
    }

    /// User nodes are not exposed to the public internet. Instead a secure
    /// tunnel is first established via the lexe gateway proxy to the user's
    /// node only after they have successfully authenticated. Requests to the
    /// user's node are then sent over the secure tunnel.
    ///
    /// This function sets up a client-side [`reqwest::Proxy`] config which
    /// looks for requests to the user node (i.e., urls starting with the fake
    /// DNS name `provision.lexe.tech` or `run.lexe.tech`) and instructs
    /// `reqwest` to use an HTTPS CONNECT tunnel over which to send the
    /// requests.
    fn proxy_config(
        gateway_url: &str,
        provision_url: &str,
        run_url: &str,
        authenticator: Arc<UserAuthenticator>,
    ) -> anyhow::Result<reqwest::Proxy> {
        let provision_url =
            Url::parse(provision_url).context("Invalid provision url")?;
        let run_url = Url::parse(run_url).context("Invalid run url")?;

        let proxy_scheme_no_auth = gateway_url
            .into_proxy_scheme()
            .context("Invalid proxy url")?;

        // TODO(phlip9): https only mode in production
        // match proxy_scheme_no_auth {
        //     reqwest::ProxyScheme::Https { .. } => (),
        //     _ => anyhow::bail!(
        //         "proxy connection must be https! gateway url: {gateway_url}"
        //     ),
        // }

        // ugly hack to get auth token to proxy
        //
        // Ideally we could just call `authenticator.get_token().await` here,
        // but this callback isn't async... Instead we have to read the most
        // recently cached token and be diligent about calling
        // `self.ensure_authed()` before calling any auth'ed API.
        Ok(reqwest::Proxy::custom(move |url| {
            if url_base_eq(url, &run_url) || url_base_eq(url, &provision_url) {
                let auth_token = authenticator
                    .get_maybe_cached_token()
                    .map(|token_with_exp| token_with_exp.token)
                    .expect("user authenticator MUST fetch token!");

                // TODO(phlip9): include "Bearer " prefix in auth token
                let auth_header = http::HeaderValue::from_str(&format!(
                    "Bearer {auth_token}"
                ))
                .unwrap();

                let mut proxy_scheme = proxy_scheme_no_auth.clone();
                proxy_scheme.set_http_auth(auth_header);

                Some(proxy_scheme)
            } else {
                None
            }
        }))
    }

    /// Ensure the client has a fresh auth token for the gateway proxy.
    ///
    /// This function is a bit hacky, since the proxy config is blocking and
    /// can't just call `authenticator.get_token().await` as it pleases. Instead
    /// we have this ugly "out-of-band" communication where we have to remember
    /// to always call `ensure_authed()` in each request caller...
    async fn ensure_authed(&self) -> Result<(), NodeApiError> {
        self.authenticator
            .get_token(self, SystemTime::now())
            .await
            .map(|_token| ())
            .map_err(|err| {
                // TODO(phlip9): how to best convert `BackendApiError` to
                //               `NodeApiError`?
                NodeApiError {
                    kind: NodeErrorKind::BadAuth,
                    msg: format!("{err:#}"),
                }
            })
    }
}

#[async_trait]
impl UserBackendApi for NodeClient {
    async fn signup(
        &self,
        signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/signup"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }

    async fn user_auth(
        &self,
        signed_req: ed25519::Signed<UserAuthRequest>,
    ) -> Result<UserAuthResponse, BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/user_auth"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }
}

#[async_trait]
impl OwnerNodeProvisionApi for NodeClient {
    async fn provision(
        &self,
        data: NodeProvisionRequest,
    ) -> Result<(), NodeApiError> {
        self.ensure_authed().await?;
        let provision_url = &self.provision_url;
        let req = self.rest.post(format!("{provision_url}/provision"), &data);
        self.rest.send(req).await
    }
}

#[async_trait]
impl OwnerNodeRunApi for NodeClient {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let req = self.rest.builder(GET, format!("{run_url}/owner/node_info"));
        self.rest.send(req).await
    }

    async fn list_channels(&self) -> Result<ListChannels, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let req = self.rest.builder(GET, format!("{run_url}/owner/channels"));
        self.rest.send(req).await
    }

    async fn get_invoice(
        &self,
        data: GetInvoiceRequest,
    ) -> Result<LxInvoice, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/get_invoice");
        let req = self.rest.post(url, &data);
        self.rest.send(req).await
    }

    async fn send_payment(
        &self,
        invoice: LxInvoice,
    ) -> Result<(), NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/owner/send_payment");
        let req = self.rest.post(url, &invoice);
        self.rest.send(req).await
    }
}

fn url_base_eq(u1: &Url, u2: &Url) -> bool {
    u1.scheme() == u2.scheme()
        && u1.host() == u2.host()
        && u1.port_or_known_default() == u2.port_or_known_default()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_url_base_eq() {
        // multiple disjoint equivalence classes of urls, according to the
        // equivalence relation `url_base_eq`.
        let eq_classes = vec![
            vec![
                "https://hello.world",
                "https://hello.world/",
                "https://hello.world/my_cool_method",
                "https://hello.world/my_cool_method&query=params",
                "https://hello.world/&query=params",
            ],
            vec![
                "http://hello.world",
                "http://hello.world/",
                "http://hello.world/my_cool_method",
                "http://hello.world/my_cool_method&query=params",
                "http://hello.world/&query=params",
            ],
            vec![
                "https://hello.world:8080",
                "https://hello.world:8080/",
                "https://hello.world:8080/my_cool_method",
                "https://hello.world:8080/my_cool_method&query=params",
                "https://hello.world:8080/&query=params",
            ],
            vec![
                "https://127.0.0.1:8080",
                "https://127.0.0.1:8080/",
                "https://127.0.0.1:8080/my_cool_method",
                "https://127.0.0.1:8080/my_cool_method&query=params",
                "https://127.0.0.1:8080/&query=params",
            ],
        ];

        let eq_classes = eq_classes
            .into_iter()
            .map(|eq_class| {
                eq_class
                    .into_iter()
                    .map(|url| Url::parse(url).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let n_classes = eq_classes.len();
        let n_urls = eq_classes[0].len();

        // all elements of an equivalence class are equal
        for eq_class in &eq_classes {
            for idx_u1 in 0..n_urls {
                // start at `idx_u1` to also check reflexivity
                for idx_u2 in idx_u1..n_urls {
                    let u1 = &eq_class[idx_u1];
                    let u2 = &eq_class[idx_u2];
                    assert!(url_base_eq(u1, u2));
                    // check symmetry
                    assert!(url_base_eq(u2, u1));
                }
            }
        }

        // elements from disjoint equivalence classes are not equal
        for idx_class1 in 0..(n_classes - 1) {
            let eq_class1 = &eq_classes[idx_class1];
            for eq_class2 in eq_classes.iter().skip(idx_class1 + 1) {
                for u1 in eq_class1 {
                    for u2 in eq_class2 {
                        // check disjoint
                        assert!(!url_base_eq(u1, u2));
                        assert!(!url_base_eq(u2, u1));
                    }
                }
            }
        }
    }
}
