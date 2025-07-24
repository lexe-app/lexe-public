//! This module contains the code for the [`NodeClient`] and [`GatewayClient`]
//! that the app uses to connect to the user node / gateway respectively, as
//! well as related TLS configurations and certificates for both the client side
//! (app) and server side (node/gateway).
//!
//! [`NodeClient`]: crate::client::NodeClient
//! [`GatewayClient`]: crate::client::GatewayClient

use std::{
    borrow::Cow,
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::Context;
use async_trait::async_trait;
use base64::Engine;
use common::{
    api::{
        auth::{
            BearerAuthRequestWire, BearerAuthResponse, BearerAuthToken, Scope,
            UserSignupRequestWire, UserSignupRequestWireV1,
        },
        fiat_rates::FiatRates,
        models::{
            SignMsgRequest, SignMsgResponse, VerifyMsgRequest,
            VerifyMsgResponse,
        },
        provision::NodeProvisionRequest,
        revocable_clients::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            GetRevocableClients, RevocableClient, RevocableClients,
            UpdateClientRequest, UpdateClientResponse,
        },
        version::{NodeRelease, NodeReleases},
    },
    constants::{self, node_provision_dns},
    ed25519,
    enclave::Measurement,
    env::DeployEnv,
    rng::Crng,
    root_seed::RootSeed,
};
use lexe_api::{
    auth::BearerAuthenticator,
    def::{
        AppBackendApi, AppGatewayApi, AppNodeProvisionApi, AppNodeRunApi,
        BearerAuthBackendApi,
    },
    error::{BackendApiError, GatewayApiError, NodeApiError, NodeErrorKind},
    models::command::{
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
    rest::{RequestBuilderExt, RestClient, POST},
    types::{payments::VecBasicPayment, Empty},
};
use lexe_tls::{
    attestation, lexe_ca, rustls, shared_seed,
    types::{LxCertificateDer, LxPrivatePkcs8KeyDer},
};
#[cfg(test)]
use proptest_derive::Arbitrary;
use reqwest::Url;
use serde::{Deserialize, Serialize};

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
/// - Requests made to running nodes use the Run-specific [`RestClient`].
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

/// Credentials required to connect to a user node via mTLS.
pub enum Credentials<'a> {
    /// Using a [`RootSeed`]. Ex: app.
    RootSeed(&'a RootSeed),
    /// Using a revocable client cert. Ex: SDK sidecar.
    ClientCredentials(&'a ClientCredentials),
}

/// All secrets required for a non-RootSeed client to authenticate and
/// communicate with a user's node.
///
/// This is exposed to users as a base64-encoded JSON blob.
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq, Arbitrary))]
pub struct ClientCredentials {
    /// The base64 encoded long-lived connect token.
    pub lexe_auth_token: BearerAuthToken,
    /// The hex-encoded client public key.
    pub client_pk: ed25519::PublicKey,
    /// The DER-encoded client key.
    pub rev_client_key_der: LxPrivatePkcs8KeyDer,
    /// The DER-encoded cert of the revocable client.
    pub rev_client_cert_der: LxCertificateDer,
    /// The DER-encoded cert of the ephemeral issuing CA.
    pub eph_ca_cert_der: LxCertificateDer,
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
    async fn signup_v2(
        &self,
        signed_req: &ed25519::Signed<&UserSignupRequestWire>,
    ) -> Result<Empty, BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/app/v2/signup"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }

    async fn signup_v1(
        &self,
        _signed_req: &ed25519::Signed<&UserSignupRequestWireV1>,
    ) -> Result<Empty, BackendApiError> {
        debug_assert!(false, "Use `signup_v2`");
        Err(BackendApiError::not_found("Use `/app/v2/signup`"))
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

    async fn latest_releases(&self) -> Result<NodeReleases, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/app/v1/latest_releases"), &Empty {});
        self.rest.send(req).await
    }
}

// --- impl NodeClient --- //

impl NodeClient {
    pub fn new(
        rng: &mut impl Crng,
        use_sgx: bool,
        deploy_env: DeployEnv,
        gateway_client: GatewayClient,
        credentials: Credentials<'_>,
    ) -> anyhow::Result<Self> {
        let run_dns = constants::NODE_RUN_DNS;
        let run_url = format!("https://{run_dns}");
        let authenticator = credentials.bearer_authenticator();

        let run_rest = {
            let proxy = Self::proxy_config(
                &gateway_client.gateway_url,
                &run_url,
                authenticator.clone(),
            )
            .context("Invalid proxy config")?;

            let (from, to) =
                (gateway_client.rest.user_agent().clone(), "node-run");
            let tls_config = credentials.tls_config(rng, deploy_env)?;
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

        let tls_config = attestation::app_node_provision_client_config(
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

    /// Ask the user node to create a new [`RevocableClient`] and return it
    /// along with its [`ClientCredentials`].
    pub async fn create_client_credentials(
        &self,
        req: CreateRevocableClientRequest,
    ) -> anyhow::Result<(RevocableClient, ClientCredentials)> {
        // Mint a new long-lived connect token
        let lexe_auth_token = self.request_long_lived_connect_token().await?;

        // Register a new revocable client
        let resp = self.create_revocable_client(req.clone()).await?;

        let client = RevocableClient {
            pubkey: resp.pubkey,
            created_at: resp.created_at,
            label: req.label,
            scope: req.scope,
            expires_at: req.expires_at,
            is_revoked: false,
        };

        let client_credentials =
            ClientCredentials::from_response(lexe_auth_token, resp);

        Ok((client, client_credentials))
    }

    /// Get a new long-lived auth token scoped only for the gateway connect
    /// proxy. Used for the SDK to connect to the node.
    async fn request_long_lived_connect_token(
        &self,
    ) -> anyhow::Result<BearerAuthToken> {
        let user_key_pair = self
            .authenticator
            .user_key_pair()
            .context("Somehow using a static bearer auth token")?;

        let now = SystemTime::now();
        let lifetime_secs = 10 * 365 * 24 * 60 * 60; // 10 years
        let scope = Some(Scope::NodeConnect);
        let long_lived_connect_token = lexe_api::auth::do_bearer_auth(
            &self.gateway_client,
            now,
            user_key_pair,
            lifetime_secs,
            scope,
        )
        .await
        .context("Failed to get long-lived connect token")?;

        Ok(long_lived_connect_token.token)
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

    async fn update_revocable_client(
        &self,
        req: UpdateClientRequest,
    ) -> Result<UpdateClientResponse, NodeApiError> {
        self.ensure_authed().await?;
        let run_url = &self.run_url;
        let url = format!("{run_url}/app/clients");
        let req = self.run_rest.put(url, &req);
        self.run_rest.send(req).await
    }
}

fn url_base_eq(u1: &Url, u2: &Url) -> bool {
    u1.scheme() == u2.scheme()
        && u1.host() == u2.host()
        && u1.port_or_known_default() == u2.port_or_known_default()
}

// --- impl Credentials --- //

impl<'a> Credentials<'a> {
    pub fn from_root_seed(root_seed: &'a RootSeed) -> Self {
        Credentials::RootSeed(root_seed)
    }

    pub fn from_client_credentials(
        client_credentials: &'a ClientCredentials,
    ) -> Self {
        Credentials::ClientCredentials(client_credentials)
    }

    /// Create a [`BearerAuthenticator`] appropriate for the given credentials.
    ///
    /// Currently limits to [`Scope::NodeConnect`] for [`RootSeed`] credentials.
    fn bearer_authenticator(&self) -> Arc<BearerAuthenticator> {
        match self {
            Credentials::RootSeed(root_seed) => {
                let maybe_cached_token = None;
                Arc::new(BearerAuthenticator::new_with_scope(
                    root_seed.derive_user_key_pair(),
                    maybe_cached_token,
                    Some(Scope::NodeConnect),
                ))
            }
            Credentials::ClientCredentials(client_credentials) =>
                Arc::new(BearerAuthenticator::new_static_token(
                    client_credentials.lexe_auth_token.clone(),
                )),
        }
    }

    /// Build a TLS client config appropriate for the given credentials.
    fn tls_config(
        &self,
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
    ) -> anyhow::Result<rustls::ClientConfig> {
        match self {
            Credentials::RootSeed(root_seed) =>
                shared_seed::app_node_run_client_config(
                    rng, deploy_env, root_seed,
                )
                .context("Failed to build RootSeed TLS client config"),
            Credentials::ClientCredentials(client_credentials) =>
                shared_seed::sdk_node_run_client_config(
                    deploy_env,
                    &client_credentials.eph_ca_cert_der,
                    client_credentials.rev_client_cert_der.clone(),
                    client_credentials.rev_client_key_der.clone(),
                )
                .context("Failed to build revocable client TLS config"),
        }
    }
}

// --- impl ClientCredentials --- //

impl ClientCredentials {
    pub fn from_response(
        lexe_auth_token: BearerAuthToken,
        resp: CreateRevocableClientResponse,
    ) -> Self {
        ClientCredentials {
            lexe_auth_token,
            client_pk: resp.pubkey,
            rev_client_key_der: LxPrivatePkcs8KeyDer(
                resp.rev_client_cert_key_der,
            ),
            rev_client_cert_der: LxCertificateDer(resp.rev_client_cert_der),
            eph_ca_cert_der: LxCertificateDer(resp.eph_ca_cert_der),
        }
    }

    /// Encodes a [`ClientCredentials`] to a base64 blob using
    /// [`base64::engine::general_purpose::STANDARD_NO_PAD`].
    // We use `STANDARD_NO_PAD` because trailing `=`s cause problems with
    // autocomplete on iPhone. For example, if the base64 string ends with:
    //
    // - `NzB2mIn0=`
    // - `NzBm2In0=`
    //
    // the iPhone autocompletes it to the following respectively when pasted
    // into iMessage, even if you 'tap away' to reject the suggestion:
    //
    // - `NzB2mIn0=120 secs`
    // - `NzBm2In0=0 in`
    pub fn to_base64_blob(&self) -> String {
        let json_str =
            serde_json::to_string(self).expect("Failed to JSON serialize");
        base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(json_str.as_bytes())
    }

    /// Decodes a [`ClientCredentials`] from a base64 blob encoded with either
    /// [`base64::engine::general_purpose::STANDARD`] or
    /// [`base64::engine::general_purpose::STANDARD_NO_PAD`].
    // NOTE: This function accepts `STANDARD` encodings because historical
    // client credentials were encoded with the `STANDARD` engine until we
    // discovered that iPhones interpret the trailing `=` as part of a unit
    // conversion, resulting in unintended autocompletions.
    pub fn try_from_base64_blob(s: &str) -> anyhow::Result<Self> {
        let s = s.trim().trim_end_matches('=');
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(s)
            .context("String is not valid base64")?;
        let string =
            String::from_utf8(bytes).context("String is not valid UTF-8")?;
        serde_json::from_str(&string).context("Failed to deserialize")
    }
}

impl FromStr for ClientCredentials {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from_base64_blob(s)
    }
}

#[cfg(test)]
mod test {
    use common::{byte_str::ByteStr, rng::FastRng};
    use proptest::{prelude::any, prop_assert_eq, proptest};
    use shared_seed::certs::{
        EphemeralIssuingCaCert, RevocableClientCert, RevocableIssuingCaCert,
    };

    use super::*;

    /// Tests [`ClientCredentials`] roundtrip to/from base64.
    ///
    /// We also test compatibility: client credentials encoded with the old
    /// STANDARD engine can be decoded with the new try_from_base64_blob method
    /// which should accept both STANDARD and STANDARD_NO_PAD.
    #[test]
    fn prop_client_credentials_base64_roundtrip() {
        proptest!(|(creds1 in any::<ClientCredentials>())| {
            // Encode using `to_base64_blob` (STANDARD_NO_PAD).
            // Decode using `try_from_base64_blob`.
            {
                let new_base64_blob = creds1.to_base64_blob();

                let creds2 =
                    ClientCredentials::try_from_base64_blob(&new_base64_blob)
                        .expect("Failed to decode from new format");

                prop_assert_eq!(&creds1, &creds2);
            }

            // Compatibility test:
            // Encode using the engine used by old clients (STANDARD).
            // Decode using `try_from_base64_blob`.
            {
                let json_str = serde_json::to_string(&creds1)
                    .expect("Failed to JSON serialize");
                let old_base64_blob = base64::engine::general_purpose::STANDARD
                    .encode(json_str.as_bytes());
                let creds2 =
                    ClientCredentials::try_from_base64_blob(&old_base64_blob)
                        .expect("Failed to decode from old format");

                prop_assert_eq!(&creds1, &creds2);
            }
        })
    }

    /// Tests that the `STANDARD_NO_PAD` engine can decode any base64 string
    /// encoded with the `STANDARD` engine after removing trailing `=`s.
    #[test]
    fn prop_base64_pad_to_no_pad_compat() {
        proptest!(|(bytes1 in any::<Vec<u8>>())| {
            let string =
                base64::engine::general_purpose::STANDARD.encode(&bytes1);
            let trimmed = string.trim_end_matches('=');
            let bytes2 = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(trimmed)
                .expect("Failed to decode base64");
            prop_assert_eq!(bytes1, bytes2);
        })
    }

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

    #[test]
    fn test_client_auth_encoding() {
        let mut rng = FastRng::from_u64(202505121546);
        let root_seed = RootSeed::from_rng(&mut rng);

        let eph_ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let eph_ca_cert_der = eph_ca_cert.serialize_der_self_signed().unwrap();

        let rev_ca_cert = RevocableIssuingCaCert::from_root_seed(&root_seed);

        let rev_client_cert = RevocableClientCert::generate_from_rng(&mut rng);
        let rev_client_cert_der = rev_client_cert
            .serialize_der_ca_signed(&rev_ca_cert)
            .unwrap();
        let rev_client_key_der = rev_client_cert.serialize_key_der();
        let client_pk = rev_client_cert.public_key();

        let client_auth = ClientCredentials {
            lexe_auth_token: BearerAuthToken(ByteStr::from_static("9dTCUvC8y7qcNyUbqynz3nwIQQHbQqPVKeMhXUj1Afr-vgj9E217_2tCS1IQM7LFqfBUC8Ec9fcb-dQiCRy6ot2FN-kR60edRFJUztAa2Rxao1Q0BS1s6vE8grgfhMYIAJDLMWgAAAAASE4zaAAAAABpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaQE")),
            client_pk,
            rev_client_key_der,
            rev_client_cert_der,
            eph_ca_cert_der,
        };

        let client_auth_str = client_auth.to_base64_blob();
        // json: ~2.2 KiB, base64(json): ~2.9 KiB
        let expected_str = "eyJsZXhlX2F1dGhfdG9rZW4iOiI5ZFRDVXZDOHk3cWNOeVVicXluejNud0lRUUhiUXFQVktlTWhYVWoxQWZyLXZnajlFMjE3XzJ0Q1MxSVFNN0xGcWZCVUM4RWM5ZmNiLWRRaUNSeTZvdDJGTi1rUjYwZWRSRkpVenRBYTJSeGFvMVEwQlMxczZ2RThncmdmaE1ZSUFKRExNV2dBQUFBQVNFNHphQUFBQUFCcGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhUUUiLCJjbGllbnRfcGsiOiI3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiIiwicmV2X2NsaWVudF9rZXlfZGVyIjoiMzA1MzAyMDEwMTMwMDUwNjAzMmI2NTcwMDQyMjA0MjAwZjU4MGQzNDYxYzRlYTBiMzZiODM2ZDQ1MWMxYzE5OWVlM2UwNjQ2YWQwZDY0MjM1Mzc5NzM5ZDY4Njk5Mjg5YTEyMzAzMjEwMDcwODhhZjFmYzEyYWIwNGFkNmRkMTY1YmMzYTNjNWViMzA2MmI0MTFhMmY1NWExNjZiMGU0MDBiMzkwZmU0ZGIiLCJyZXZfY2xpZW50X2NlcnRfZGVyIjoiMzA4MjAxODMzMDgyMDEzNWEwMDMwMjAxMDIwMjE0NDBiZWRjNTZkMDNkNmI1MmYyODQyZDY0ZGY5MGQwMmQ2ZGEzNmE1YjMwMDUwNjAzMmI2NTcwMzA1NjMxMGIzMDA5MDYwMzU1MDQwNjBjMDI1NTUzMzEwYjMwMDkwNjAzNTUwNDA4MGMwMjQzNDEzMTExMzAwZjA2MDM1NTA0MGEwYzA4NmM2NTc4NjUyZDYxNzA3MDMxMjczMDI1MDYwMzU1MDQwMzBjMWU0YzY1Nzg2NTIwNzI2NTc2NmY2MzYxNjI2YzY1MjA2OTczNzM3NTY5NmU2NzIwNDM0MTIwNjM2NTcyNzQzMDIwMTcwZDM3MzUzMDMxMzAzMTMwMzAzMDMwMzAzMDVhMTgwZjM0MzAzOTM2MzAzMTMwMzEzMDMwMzAzMDMwMzA1YTMwNTIzMTBiMzAwOTA2MDM1NTA0MDYwYzAyNTU1MzMxMGIzMDA5MDYwMzU1MDQwODBjMDI0MzQxMzExMTMwMGYwNjAzNTUwNDBhMGMwODZjNjU3ODY1MmQ2MTcwNzAzMTIzMzAyMTA2MDM1NTA0MDMwYzFhNGM2NTc4NjUyMDcyNjU3NjZmNjM2MTYyNmM2NTIwNjM2YzY5NjU2ZTc0MjA2MzY1NzI3NDMwMmEzMDA1MDYwMzJiNjU3MDAzMjEwMDcwODhhZjFmYzEyYWIwNGFkNmRkMTY1YmMzYTNjNWViMzA2MmI0MTFhMmY1NWExNjZiMGU0MDBiMzkwZmU0ZGJhMzE3MzAxNTMwMTMwNjAzNTUxZDExMDQwYzMwMGE4MjA4NmM2NTc4NjUyZTYxNzA3MDMwMDUwNjAzMmI2NTcwMDM0MTAwN2IxN2JjOTUzODI2N2IzNTRmMDcyNmQ4OWNiMWVjMzEwYjEwMmU0MjJhYjk2OTZiODdkOWFlNzAwY2VmMmU4M2MxMzY2ZDBhZDE5MDM1ZDllM2VkMDRjZjVmN2YwNWRlZjY4YTcxZGUyMTJiODk4MzQ0Nzc5NDJhZTc2M2EyMGYiLCJlcGhfY2FfY2VydF9kZXIiOiIzMDgyMDFhZTMwODIwMTYwYTAwMzAyMDEwMjAyMTQxMGNkNWM5OTg5Zjk2NTIwOTQ5ZTBlOWFiNGNlNGRiZTE0NzY2NzEwMzAwNTA2MDMyYjY1NzAzMDUwMzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyMTMwMWYwNjAzNTUwNDAzMGMxODRjNjU3ODY1MjA3MzY4NjE3MjY1NjQyMDczNjU2NTY0MjA0MzQxMjA2MzY1NzI3NDMwMjAxNzBkMzczNTMwMzEzMDMxMzAzMDMwMzAzMDMwNWExODBmMzQzMDM5MzYzMDMxMzAzMTMwMzAzMDMwMzAzMDVhMzA1MDMxMGIzMDA5MDYwMzU1MDQwNjBjMDI1NTUzMzEwYjMwMDkwNjAzNTUwNDA4MGMwMjQzNDEzMTExMzAwZjA2MDM1NTA0MGEwYzA4NmM2NTc4NjUyZDYxNzA3MDMxMjEzMDFmMDYwMzU1MDQwMzBjMTg0YzY1Nzg2NTIwNzM2ODYxNzI2NTY0MjA3MzY1NjU2NDIwNDM0MTIwNjM2NTcyNzQzMDJhMzAwNTA2MDMyYjY1NzAwMzIxMDBlZmU5Y2UxYWJjYWVhYmNlZjhlYTJmNTRhNTY5NTUwZGNlZDRhOGYzYThjYmIwNGNkOTQ1ZDFiNGUyNDVmNjg3YTM0YTMwNDgzMDEzMDYwMzU1MWQxMTA0MGMzMDBhODIwODZjNjU3ODY1MmU2MTcwNzAzMDFkMDYwMzU1MWQwZTA0MTYwNDE0OTBjZDVjOTk4OWY5NjUyMDk0OWUwZTlhYjRjZTRkYmUxNDc2NjcxMDMwMTIwNjAzNTUxZDEzMDEwMWZmMDQwODMwMDYwMTAxZmYwMjAxMDAzMDA1MDYwMzJiNjU3MDAzNDEwMDM3MjU0MjlmNWJjYTgwNTYyMWMyYjJkYzQ0NTgwMmVkMjFjYWIyNDZiNDVhZDEyMWRkMmE0MzJlZmEyZjkzZWY3MjVlZmExNzgyZTY0MTA4ZDIyOThlODY5NGY0Njg2Y2VkOThjZTkyODBlZDc0OWQwYWQ0YjQ0YTRhMWNlZTBkIn0";
        assert_eq!(client_auth_str, expected_str);

        let client_auth2 =
            ClientCredentials::try_from_base64_blob(&client_auth_str)
                .expect("Failed to decode ClientAuth");
        assert_eq!(client_auth, client_auth2);
    }
}
