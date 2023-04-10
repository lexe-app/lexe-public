// TODO

pub mod certs;
/// TLS configurations for the client to the node.
pub mod tls;

use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use async_trait::async_trait;
use reqwest::{IntoProxyScheme, Url};

use crate::api::auth::{
    BearerAuthRequest, BearerAuthResponse, BearerAuthenticator,
    UserSignupRequest,
};
use crate::api::command::{CreateInvoiceRequest, ListChannels, NodeInfo};
use crate::api::def::{
    AppBackendApi, AppGatewayApi, AppNodeProvisionApi, AppNodeRunApi,
    BearerAuthBackendApi,
};
use crate::api::error::{
    BackendApiError, GatewayApiError, NodeApiError, NodeErrorKind,
};
use crate::api::fiat_rates::FiatRates;
use crate::api::provision::NodeProvisionRequest;
use crate::api::qs::{EmptyData, GetNewPayments, GetPaymentsByIds};
use crate::api::rest::{RequestBuilderExt, RestClient, GET, POST};
use crate::ln::invoice::LxInvoice;
use crate::ln::payments::BasicPayment;
use crate::rng::Crng;
use crate::root_seed::RootSeed;
use crate::{attest, ed25519};

/// The Lexe app's client to the user node.
pub struct NodeClient {
    rest: RestClient,
    gateway_url: String,
    provision_url: &'static str,
    run_url: &'static str,
    authenticator: Arc<BearerAuthenticator>,
}

// Why are we manually impl'ing `UnwindSafe` and `RefUnwindSafe` for
// `NodeClient`?
//
// ## Unwind Safety
//
// Technically, NodeClient is not 100% unwind safe, since `BearerAuthenticator`
// contains a `tokio::sync::Mutex`, which doesn't impl lock poisoning [1].
//
// However, unwind safety feels pretty niche and doesn't seem worth the
// inconvenience. We use panics for unrecoverable errors; our programs should
// crash and burn on panic--maybe try to log and display an error message, but
// not try to recover.
//
// ## Background
//
// A type is unwind safe if, after panicking and _then recovering from the
// panic_ (using `std::panic::catch_unwind`), we can't observe any undefined
// state in the type.
//
// Normally, we don't use `catch_unwind`, so a panic will drop each stack frame
// as it unwinds and we can never observe any weird states (because the types
// are dropped and gone).
//
// ## Our Situation
//
// The app FFI layer, provided by `flutter_rust_bridge`, does use
// `catch_unwind`, since it's unsafe to panic across an FFI boundary:
//
// > Rust's unwinding strategy is not specified to be fundamentally compatible
// > with any other language's unwinding. As such, unwinding into Rust from
// > another language, or unwinding into another language from Rust is
// > Undefined Behavior. You must absolutely catch any panics at the FFI
// > boundary! What you do at that point is up to you, but something must be
// > done. If you fail to do this, at best, your application will crash and
// > burn. At worst, your application won't crash and burn, and will proceed
// > with completely clobbered state.
// >
// > [The Rustonomicon > Unwinding](https://doc.rust-lang.org/nomicon/unwinding.html)
//
// When a panic occurs in the app's Rust code, we should ideally just log and
// report the error+stacktrace to something like Sentry or Firebase and then
// kill the app--MAYBE show a nice user-friendly alert box first.
//
// ## Refs
//
// [1]: https://docs.rs/tokio/1.21.1/tokio/sync/struct.Mutex.html
//
// > Note that in contrast to std::sync::Mutex, this implementation does not
// > poison the mutex when a thread holding the MutexGuard panics. In such a
// > case, the mutex will be unlocked. If the panic is caught, this might leave
// > the data protected by the mutex in an inconsistent state.

impl UnwindSafe for NodeClient {}
impl RefUnwindSafe for NodeClient {}

impl NodeClient {
    pub fn new<R: Crng>(
        rng: &mut R,
        seed: &RootSeed,
        authenticator: Arc<BearerAuthenticator>,
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
        authenticator: Arc<BearerAuthenticator>,
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
                    .expect("bearer authenticator MUST fetch token!");

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
impl AppBackendApi for NodeClient {
    async fn signup(
        &self,
        signed_req: ed25519::Signed<UserSignupRequest>,
    ) -> Result<(), BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/app/v1/signup"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }
}

#[async_trait]
impl BearerAuthBackendApi for NodeClient {
    async fn bearer_auth(
        &self,
        signed_req: ed25519::Signed<BearerAuthRequest>,
    ) -> Result<BearerAuthResponse, BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/app/bearer_auth"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }
}

#[async_trait]
impl AppGatewayApi for NodeClient {
    async fn get_fiat_rates(&self) -> Result<FiatRates, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/app/v1/fiat_rates"), &EmptyData {});
        self.rest.send(req).await
    }
}

#[async_trait]
impl AppNodeProvisionApi for NodeClient {
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
impl AppNodeRunApi for NodeClient {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/node_info");
        let req = self.rest.builder(GET, url);
        self.rest.send(req).await
    }

    async fn list_channels(&self) -> Result<ListChannels, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/channels");
        let req = self.rest.builder(GET, url);
        self.rest.send(req).await
    }

    async fn create_invoice(
        &self,
        data: CreateInvoiceRequest,
    ) -> Result<LxInvoice, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/create_invoice");
        let req = self.rest.post(url, &data);
        self.rest.send(req).await
    }

    async fn pay_invoice(
        &self,
        invoice: LxInvoice,
    ) -> Result<(), NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/pay_invoice");
        let req = self.rest.post(url, &invoice);
        self.rest.send(req).await
    }

    async fn get_payments_by_ids(
        &self,
        req: GetPaymentsByIds,
    ) -> Result<Vec<BasicPayment>, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/payments/ids");
        let req = self.rest.post(url, &req);
        self.rest.send(req).await
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<Vec<BasicPayment>, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/payments/new");
        let req = self.rest.get(url, &req);
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
