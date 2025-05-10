//! This module contains the code for the [`NodeClient`] and [`GatewayClient`]
//! that the app uses to connect to the user node / gateway respectively, as
//! well as related TLS configurations and certificates for both the client side
//! (app) and server side (node/gateway).
//!
//! [`NodeClient`]: crate::client::NodeClient
//! [`GatewayClient`]: crate::client::GatewayClient

use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use async_trait::async_trait;
use common::{
    api::{
        auth::{
            BearerAuthRequestWire, BearerAuthResponse, BearerAuthenticator,
            UserSignupRequest,
        },
        command::{
            CloseChannelRequest, CreateInvoiceRequest, CreateInvoiceResponse,
            CreateOfferRequest, CreateOfferResponse, GetAddressResponse,
            GetNewPayments, ListChannelsResponse, NodeInfo, OpenChannelRequest,
            OpenChannelResponse, PayInvoiceRequest, PayInvoiceResponse,
            PayOfferRequest, PayOfferResponse, PayOnchainRequest,
            PayOnchainResponse, PaymentIndexes, PreflightCloseChannelRequest,
            PreflightCloseChannelResponse, PreflightOpenChannelRequest,
            PreflightOpenChannelResponse, PreflightPayInvoiceRequest,
            PreflightPayInvoiceResponse, PreflightPayOfferRequest,
            PreflightPayOfferResponse, PreflightPayOnchainRequest,
            PreflightPayOnchainResponse, UpdatePaymentNote,
        },
        def::{
            AppBackendApi, AppGatewayApi, AppNodeProvisionApi, AppNodeRunApi,
            BearerAuthBackendApi,
        },
        error::{
            BackendApiError, GatewayApiError, NodeApiError, NodeErrorKind,
        },
        fiat_rates::FiatRates,
        models::{
            SignMsgRequest, SignMsgResponse, VerifyMsgRequest,
            VerifyMsgResponse,
        },
        provision::NodeProvisionRequest,
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            GetRevocableClients, RevocableClients, RevokeClient,
            UpdateClientExpiration, UpdateClientLabel, UpdateClientScope,
        },
        version::NodeRelease,
        Empty,
    },
    constants::{self, node_provision_dns},
    ed25519,
    enclave::Measurement,
    env::DeployEnv,
    ln::payments::VecBasicPayment,
    rng::Crng,
    root_seed::RootSeed,
};
use lexe_api::{
    rest::{RequestBuilderExt, RestClient, POST},
    tls::{self, lexe_ca},
};
use reqwest::Url;

/// The client to the gateway itself, i.e. requests terminate at the gateway.
#[derive(Clone)]
pub struct GatewayClient {
    rest: RestClient,
    gateway_url: String,
}

/// The client to the user node.
///
/// Requests are proxied via the gateway CONNECT proxies. These proxies avoid
/// exposing user nodes to the public internet and enforce user authentication
/// and other request rate limits.
///
/// - Requests made to running nodes use the Run-specific [`RestClient`] which
///   includes a TLS configuration for [`RootSeed`]-based mTLS.
/// - Requests made to provisioning nodes as a [`RestClient`] which is created
///   on-the-fly. This is because it is necessary to include a TLS config which
///   checks the server's remote attestation against a [`Measurement`] which is
///   only known at provisioning time. This is also desirable because provision
///   requests generally happen only once, so there is no need to maintain a
///   connection pool after provisioning has complete.
pub struct NodeClient {
    gateway_client: GatewayClient,
    /// The [`RestClient`] used to communicate with a Run node.
    run_rest: RestClient,
    run_url: String,
    use_sgx: bool,
    deploy_env: DeployEnv,
    authenticator: Arc<BearerAuthenticator>,
}

// --- impl GatewayClient --- //

impl GatewayClient {
    pub fn new(
        deploy_env: DeployEnv,
        gateway_url: String,
        user_agent: impl Into<Cow<'static, str>>,
    ) -> anyhow::Result<Self> {
        fn inner(
            deploy_env: DeployEnv,
            gateway_url: String,
            user_agent: Cow<'static, str>,
        ) -> anyhow::Result<GatewayClient> {
            let tls_config = lexe_ca::app_gateway_client_config(deploy_env);
            let rest = RestClient::new(user_agent, "gateway", tls_config);
            Ok(GatewayClient { rest, gateway_url })
        }
        inner(deploy_env, gateway_url, user_agent.into())
    }
}

impl AppBackendApi for GatewayClient {
    async fn signup(
        &self,
        signed_req: &ed25519::Signed<&UserSignupRequest>,
    ) -> Result<Empty, BackendApiError> {
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
impl BearerAuthBackendApi for GatewayClient {
    async fn bearer_auth(
        &self,
        signed_req: &ed25519::Signed<&BearerAuthRequestWire>,
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

impl AppGatewayApi for GatewayClient {
    async fn get_fiat_rates(&self) -> Result<FiatRates, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/app/v1/fiat_rates"), &Empty {});
        self.rest.send(req).await
    }

    async fn latest_release(&self) -> Result<NodeRelease, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/app/v1/latest_release"), &Empty {});
        self.rest.send(req).await
    }
}

// --- impl NodeClient --- //

impl NodeClient {
    pub fn new(
        rng: &mut impl Crng,
        use_sgx: bool,
        root_seed: &RootSeed,
        deploy_env: DeployEnv,
        authenticator: Arc<BearerAuthenticator>,
        gateway_client: GatewayClient,
    ) -> anyhow::Result<Self> {
        let run_dns = constants::NODE_RUN_DNS;
        let run_url = format!("https://{run_dns}");

        let run_rest = {
            let proxy = Self::proxy_config(
                &gateway_client.gateway_url,
                &run_url,
                authenticator.clone(),
            )
            .context("Invalid proxy config")?;

            let tls_config = tls::shared_seed::app_node_run_client_config(
                rng, deploy_env, root_seed,
            )?;

            let (from, to) =
                (gateway_client.rest.user_agent().clone(), "node-run");
            let reqwest_client = RestClient::client_builder(&from)
                .proxy(proxy)
                .use_preconfigured_tls(tls_config)
                .build()
                .context("Failed to build client")?;

            RestClient::from_inner(reqwest_client, from, to)
        };

        Ok(Self {
            gateway_client,
            run_rest,
            run_url,
            use_sgx,
            deploy_env,
            authenticator,
        })
    }

    /// User nodes are not exposed to the public internet. Instead, a secure
    /// tunnel (TLS) is first established via the lexe gateway proxy to the
    /// user's node only after they have successfully authenticated with Lexe.
    ///
    /// Essentially, we have a TLS-in-TLS scheme:
    ///
    /// - The outer layer terminates at Lexe's gateway proxy and prevents the
    ///   public internet from seeing auth tokens sent to the gateway proxy.
    /// - The inner layer terminates inside the SGX enclave and prevents the
    ///   Lexe operators from snooping on or tampering with data sent to/from
    ///   the app <-> node.
    ///
    /// This function sets up a client-side [`reqwest::Proxy`] config which
    /// looks for requests to the user node (i.e., urls starting with one of the
    /// fake DNS names `{mr_short}.provision.lexe.app` or `run.lexe.app`) and
    /// instructs `reqwest` to use an HTTPS CONNECT tunnel over which to send
    /// the requests.
    fn proxy_config(
        gateway_url: &str,
        node_url: &str,
        authenticator: Arc<BearerAuthenticator>,
    ) -> anyhow::Result<reqwest::Proxy> {
        use reqwest::IntoProxyScheme;

        let node_url = Url::parse(node_url).context("Invalid node url")?;

        let proxy_scheme_no_auth = gateway_url
            .into_proxy_scheme()
            .context("Invalid proxy url")?;

        // App->Gateway connection must be HTTPS
        match proxy_scheme_no_auth {
            reqwest::ProxyScheme::Https { .. } => (),
            _ => anyhow::bail!(
                "proxy connection must be https: gateway url: {gateway_url}"
            ),
        }

        // ugly hack to get auth token to proxy
        //
        // Ideally we could just call `authenticator.get_token().await` here,
        // but this callback isn't async... Instead we have to read the most
        // recently cached token and be diligent about calling
        // `self.ensure_authed()` before calling any auth'ed API.
        let proxy = reqwest::Proxy::custom(move |url| {
            if url_base_eq(url, &node_url) {
                let auth_token = authenticator
                    .get_maybe_cached_token()
                    .expect("bearer authenticator MUST fetch token!");

                // TODO(phlip9): include "Bearer " prefix in auth token
                let auth_header = http::HeaderValue::from_str(&format!(
                    "Bearer {auth_token}"
                ))
                .unwrap();

                let mut proxy_scheme = proxy_scheme_no_auth.clone();
                proxy_scheme.set_custom_http_auth(auth_header);

                Some(proxy_scheme)
            } else {
                None
            }
        });

        Ok(proxy)
    }

    /// Ensure the client has a fresh auth token for the gateway proxy.
    ///
    /// This function is a bit hacky, since the proxy config is blocking and
    /// can't just call `authenticator.get_token().await` as it pleases. Instead
    /// we have this ugly "out-of-band" communication where we have to remember
    /// to always call `ensure_authed()` in each request caller...
    async fn ensure_authed(&self) -> Result<(), NodeApiError> {
        self.authenticator
            .get_token(&self.gateway_client, SystemTime::now())
            .await
            .map(|_token| ())
            // TODO(phlip9): how to best convert `BackendApiError` to
            //               `NodeApiError`?
            .map_err(|backend_error| {
                // Contains backend kind msg and regular msg
                let msg = format!("{backend_error:#}");

                let BackendApiError {
                    data, sensitive, ..
                } = backend_error;

                NodeApiError {
                    kind: NodeErrorKind::BadAuth,
                    msg,
                    data,
                    sensitive,
                }
            })
    }

    /// Builds a Provision-specific [`RestClient`] which can be used to make a
    /// provision request to a provisioning node.
    fn provision_rest_client(
        &self,
        user_agent: Cow<'static, str>,
        measurement: Measurement,
        provision_url: &str,
    ) -> anyhow::Result<RestClient> {
        let proxy = Self::proxy_config(
            &self.gateway_client.gateway_url,
            provision_url,
            self.authenticator.clone(),
        )
        .context("Invalid proxy config")?;

        let tls_config = tls::attestation::app_node_provision_client_config(
            self.use_sgx,
            self.deploy_env,
            measurement,
        );

        let (from, to) = (user_agent, "node-provision");
        let reqwest_client = RestClient::client_builder(&from)
            .proxy(proxy)
            .use_preconfigured_tls(tls_config)
            // Provision can take longer than 5 sec. <3 gdrive : )
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to build client")?;

        let provision_rest = RestClient::from_inner(reqwest_client, from, to);

        Ok(provision_rest)
    }
}

impl AppNodeProvisionApi for NodeClient {
    async fn provision(
        &self,
        measurement: Measurement,
        data: NodeProvisionRequest,
    ) -> Result<Empty, NodeApiError> {
        let mr_short = measurement.short();
        let provision_dns = node_provision_dns(&mr_short);
        let provision_url = format!("https://{provision_dns}");

        // Create rest client on the fly
        let provision_rest = self
            .provision_rest_client(
                self.gateway_client.rest.user_agent().clone(),
                measurement,
                &provision_url,
            )
            .context("Failed to build provision rest client")
            .map_err(NodeApiError::provision)?;

        self.ensure_authed().await?;
        let req = provision_rest
            .post(format!("{provision_url}/app/provision"), &data);
        provision_rest.send(req).await
    }
}

impl AppNodeRunApi for NodeClient {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/node_info");
        let req = self.run_rest.get(url, &Empty {});
        self.run_rest.send(req).await
    }

    async fn list_channels(
        &self,
    ) -> Result<ListChannelsResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/list_channels");
        let req = self.run_rest.get(url, &Empty {});
        self.run_rest.send(req).await
    }

    async fn sign_message(
        &self,
        data: SignMsgRequest,
    ) -> Result<SignMsgResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/sign_message");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn verify_message(
        &self,
        data: VerifyMsgRequest,
    ) -> Result<VerifyMsgResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/verify_message");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn open_channel(
        &self,
        data: OpenChannelRequest,
    ) -> Result<OpenChannelResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/open_channel");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn preflight_open_channel(
        &self,
        data: PreflightOpenChannelRequest,
    ) -> Result<PreflightOpenChannelResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/preflight_open_channel");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn close_channel(
        &self,
        data: CloseChannelRequest,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/close_channel");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn preflight_close_channel(
        &self,
        data: PreflightCloseChannelRequest,
    ) -> Result<PreflightCloseChannelResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/preflight_close_channel");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn create_invoice(
        &self,
        data: CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/create_invoice");
        let req = self.run_rest.post(url, &data);
        self.run_rest.send(req).await
    }

    async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> Result<PayInvoiceResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/pay_invoice");
        // `pay_invoice` may call `max_flow` which takes a long time.
        let req = self
            .run_rest
            .post(url, &req)
            .timeout(constants::MAX_FLOW_TIMEOUT + Duration::from_secs(2));
        self.run_rest.send(req).await
    }

    async fn preflight_pay_invoice(
        &self,
        req: PreflightPayInvoiceRequest,
    ) -> Result<PreflightPayInvoiceResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/preflight_pay_invoice");
        // `preflight_pay_invoice` may call `max_flow` which takes a long time.
        let req = self
            .run_rest
            .post(url, &req)
            .timeout(constants::MAX_FLOW_TIMEOUT + Duration::from_secs(2));
        self.run_rest.send(req).await
    }

    async fn pay_onchain(
        &self,
        req: PayOnchainRequest,
    ) -> Result<PayOnchainResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/pay_onchain");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn preflight_pay_onchain(
        &self,
        req: PreflightPayOnchainRequest,
    ) -> Result<PreflightPayOnchainResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/preflight_pay_onchain");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> Result<CreateOfferResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/create_offer");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> Result<PayOfferResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/pay_offer");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn preflight_pay_offer(
        &self,
        req: PreflightPayOfferRequest,
    ) -> Result<PreflightPayOfferResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/preflight_pay_offer");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn get_address(&self) -> Result<GetAddressResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/get_address");
        let req = self.run_rest.post(url, &Empty {});
        self.run_rest.send(req).await
    }

    async fn get_payments_by_indexes(
        &self,
        req: PaymentIndexes,
    ) -> Result<VecBasicPayment, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/payments/indexes");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn get_new_payments(
        &self,
        req: GetNewPayments,
    ) -> Result<VecBasicPayment, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/payments/new");
        let req = self.run_rest.get(url, &req);
        self.run_rest.send(req).await
    }

    async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/payments/note");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
    }

    async fn get_revocable_clients(
        &self,
        req: GetRevocableClients,
    ) -> Result<RevocableClients, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients");
        let req = self.run_rest.get(url, &req);
        self.run_rest.send(req).await
    }

    async fn create_revocable_client(
        &self,
        req: CreateRevocableClientRequest,
    ) -> Result<CreateRevocableClientResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients");
        let req = self.run_rest.post(url, &req);
        self.run_rest.send(req).await
    }

    async fn update_client_expiration(
        &self,
        req: UpdateClientExpiration,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients/expiration");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
    }

    async fn update_client_label(
        &self,
        req: UpdateClientLabel,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients/label");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
    }

    async fn update_client_scope(
        &self,
        req: UpdateClientScope,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients/scope");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
    }

    async fn revoke_client(
        &self,
        req: RevokeClient,
    ) -> Result<Empty, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients/revoke");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
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
            vec![
                "https://[::1]:8080",
                "https://[::1]:8080/",
                "https://[::1]:8080/my_cool_method",
                "https://[::1]:8080/my_cool_method&query=params",
                "https://[::1]:8080/&query=params",
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
