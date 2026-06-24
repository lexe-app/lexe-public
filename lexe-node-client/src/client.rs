//! This module contains the code for the [`NodeClient`] and [`GatewayClient`]
//! that the user uses to connect to the user node / gateway respectively, as
//! well as related TLS configurations and certificates for both the client side
//! (user) and server side (node/gateway).
//!
//! [`NodeClient`]: crate::client::NodeClient
//! [`GatewayClient`]: crate::client::GatewayClient

use std::{
    borrow::Cow,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, ensure};
use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use lexe_api::{
    auth::{self, BearerAuthenticator},
    credentials::{ClientCredentials, CredentialsRef},
    def::{
        BearerAuthBackendApi, UserBackendApi, UserGatewayApi,
        UserNodeProvisionApi, UserNodeRunApi,
    },
    error::{BackendApiError, GatewayApiError, NodeApiError, NodeErrorKind},
    models::{
        command::{
            BackupInfo, CloseChannelPreflightRequest,
            CloseChannelPreflightResponse, CloseChannelRequest,
            CreateInvoiceRequest, CreateInvoiceResponse, CreateOfferRequest,
            CreateOfferResponse, DebugInfo, EnclavesToProvisionRequest,
            GetHumanBitcoinAddressResponse, GetNewPayments,
            GetNextUnusedAddressResponse, GetUpdatedPayments,
            HumanBitcoinAddressV1, ListChannelsResponse, NodeInfo,
            OpenChannelPreflightRequest, OpenChannelPreflightResponse,
            OpenChannelRequest, OpenChannelResponse,
            PayInvoicePreflightRequest, PayInvoicePreflightResponse,
            PayInvoiceRequest, PayInvoiceResponse, PayOfferPreflightRequest,
            PayOfferPreflightResponse, PayOfferRequest, PayOfferResponse,
            PayOnchainPreflightRequest, PayOnchainPreflightResponse,
            PayOnchainRequest, PayOnchainResponse, PaymentCreatedIndexes,
            PaymentIdStruct, SetupGDrive, UpdatePersonalNote,
            UpsertHumanBitcoinAddressResponse,
        },
        nwc::{
            CreateNwcClientRequest, CreateNwcClientResponse,
            ListNwcClientResponse, NostrPkStruct, UpdateNwcClientRequest,
            UpdateNwcClientResponse,
        },
    },
    rest::{POST, RequestBuilderExt, RestClient},
    revocable_clients::{
        RevocableClient, RevocableClients,
        models::{
            CreateRevocableClientRequest, CreateRevocableClientResponse,
            ListRevocableClients, UpdateClientRequest, UpdateClientResponse,
        },
    },
    types::{
        Empty,
        payments::{MaybeBasicPaymentV2, VecBasicPaymentV1, VecBasicPaymentV2},
        username::UsernameStruct,
    },
};
use lexe_common::{
    api::{
        auth::{
            BearerAuthRequestWire, BearerAuthResponse, BearerAuthToken,
            LexeScope, TokenWithExpiration, UserSignupRequestWire,
            UserSignupRequestWireV1,
        },
        fiat_rates::FiatRates,
        models::{
            SignMsgRequest, SignMsgResponse, VerifyMsgRequest,
            VerifyMsgResponse,
        },
        provision::NodeProvisionRequest,
        user::UserPk,
        version::{CurrentEnclaves, EnclavesToProvision, NodeEnclave},
    },
    byte_str::ByteStr,
    constants::{self, node_provision_dns},
    env::DeployEnv,
};
use lexe_crypto::{ed25519, rng::Crng};
use lexe_enclave::enclave::Measurement;
use lexe_tls::{attest_client, lexe_ca, rustls};
use reqwest::Url;

/// The client to the gateway itself, i.e. requests terminate at the gateway.
#[derive(Clone)]
pub struct GatewayClient {
    rest: RestClient,
    gateway_url: Cow<'static, str>,
}

/// The client to the user node.
///
/// Requests are proxied via the gateway CONNECT proxies. These proxies avoid
/// exposing user nodes to the public internet and enforce user authentication
/// and other request rate limits.
///
/// - Requests made to running nodes use the Run-specific [`RestClient`].
/// - Requests made to provisioning nodes use a [`RestClient`] which is created
///   on-the-fly. This is because it is necessary to include a TLS config which
///   checks the server's remote attestation against a [`Measurement`] which is
///   only known at provisioning time. This is also desirable because provision
///   requests generally happen only once, so there is no need to maintain a
///   connection pool after provisioning has complete.
#[derive(Clone)]
pub struct NodeClient {
    inner: Arc<NodeClientInner>,
}

struct NodeClientInner {
    /// The user's public key, if available from credentials.
    user_pk: Option<UserPk>,
    gateway_client: GatewayClient,
    /// The [`RestClient`] used to communicate with a Run node.
    ///
    /// This is an [`ArcSwapOption`] so that we can atomically swap in a new
    /// client with a new proxy config when the auth token expires.
    ///
    /// Previously, we used this patch to dynamically set the proxy auth header
    /// with the latest auth token:
    /// [proxy: allow setting proxy-auth at intercept time](https://github.com/lexe-app/reqwest/commit/dea2dd7a1d3c52e50d1c47803fdc57d73e35c769)
    /// This approach has the best connection reuse, since the connection pool
    /// is shared across all tokens; we should only need to reconnect if the
    /// underlying connection times out.
    ///
    /// This approach removes the need for a patch. One downside: it replaces
    /// the connection pool whenever we need to re-auth. Until we get
    /// per-request proxy configs in `reqwest`, this is likely the best we can
    /// do. Though one reconnection per 10 min. is probably ok.
    run_rest: ArcSwapOption<RunRestClient>,
    run_url: &'static str,
    use_sgx: bool,
    deploy_env: DeployEnv,
    authenticator: Arc<BearerAuthenticator>,
    tls_config: rustls::ClientConfig,
}

/// A [`RestClient`] with required proxy configuration needed to communicate
/// with a user node.
struct RunRestClient {
    client: RestClient,
    /// When the auth token used in the proxy config expires, or `None` if it
    /// never expires.
    token_expiration: Option<SystemTime>,
}

// --- impl GatewayClient --- //

impl GatewayClient {
    pub fn new(
        deploy_env: DeployEnv,
        gateway_url: impl Into<Cow<'static, str>>,
        user_agent: impl Into<Cow<'static, str>>,
    ) -> anyhow::Result<Self> {
        fn inner(
            deploy_env: DeployEnv,
            gateway_url: Cow<'static, str>,
            user_agent: Cow<'static, str>,
        ) -> anyhow::Result<GatewayClient> {
            let tls_config = lexe_ca::user_gateway_client_config(deploy_env);
            let rest = RestClient::new(user_agent, "gateway", tls_config);
            Ok(GatewayClient { rest, gateway_url })
        }
        inner(deploy_env, gateway_url.into(), user_agent.into())
    }
}

impl UserBackendApi for GatewayClient {
    async fn signup_v2(
        &self,
        signed_req: &ed25519::Signed<&UserSignupRequestWire>,
    ) -> Result<Empty, BackendApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .builder(POST, format!("{gateway_url}/user/v2/signup"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }

    async fn signup_v1(
        &self,
        _signed_req: &ed25519::Signed<&UserSignupRequestWireV1>,
    ) -> Result<Empty, BackendApiError> {
        debug_assert!(false, "Use `signup_v2`");
        Err(BackendApiError::not_found("Use `/user/v2/signup`"))
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
            .builder(POST, format!("{gateway_url}/user/bearer_auth"))
            .signed_bcs(signed_req)
            .map_err(BackendApiError::bcs_serialize)?;
        self.rest.send(req).await
    }
}

impl UserGatewayApi for GatewayClient {
    async fn get_fiat_rates(&self) -> Result<FiatRates, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/user/v1/fiat_rates"), &Empty {});
        self.rest.send(req).await
    }

    async fn enclaves_to_provision(
        &self,
        req: &EnclavesToProvisionRequest,
        auth: BearerAuthToken,
    ) -> Result<EnclavesToProvision, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let url = format!("{gateway_url}/user/v1/enclaves_to_provision");
        let req = self.rest.post(url, req).bearer_auth(&auth);
        self.rest.send(req).await
    }

    async fn latest_release(&self) -> Result<NodeEnclave, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/user/v1/latest_release"), &Empty {});
        self.rest.send(req).await
    }

    async fn current_releases(
        &self,
    ) -> Result<CurrentEnclaves, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/user/v1/current_releases"), &Empty {});
        self.rest.send(req).await
    }

    async fn current_enclaves(
        &self,
    ) -> Result<CurrentEnclaves, GatewayApiError> {
        let gateway_url = &self.gateway_url;
        let req = self
            .rest
            .get(format!("{gateway_url}/user/v1/current_enclaves"), &Empty {});
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
        credentials: CredentialsRef<'_>,
    ) -> anyhow::Result<Self> {
        let run_url = constants::NODE_RUN_URL;

        let gateway_url = &gateway_client.gateway_url;
        ensure!(
            gateway_url.starts_with("https://"),
            "proxy connection must be https: gateway url: {gateway_url}",
        );

        let user_pk = credentials.user_pk();
        let authenticator = credentials.bearer_authenticator()?;
        let tls_config = credentials.tls_config(rng, deploy_env)?;
        let run_rest = ArcSwapOption::from(None);

        Ok(Self {
            inner: Arc::new(NodeClientInner {
                user_pk,
                gateway_client,
                run_rest,
                run_url,
                use_sgx,
                deploy_env,
                authenticator,
                tls_config,
            }),
        })
    }

    /// Returns the user's public key, if available from the credentials.
    ///
    /// Returns `None` if credentials were created before node v0.8.11,
    /// which didn't include user_pk.
    pub fn user_pk(&self) -> Option<UserPk> {
        self.inner.user_pk
    }

    /// Get an authenticated [`RunRestClient`] for making requests to the user
    /// node's run endpoint via the gateway CONNECT proxy.
    ///
    /// The returned client always has a fresh auth token for the gateway proxy.
    ///
    /// In the common case where our token is still fresh, this is a fast atomic
    /// load of the cached client. If the token is expired, we will request a
    /// new token, build a new client, and swap it in atomically.
    async fn authed_run_rest(
        &self,
    ) -> Result<Arc<RunRestClient>, NodeApiError> {
        let now = SystemTime::now();

        // Fast path: we already have a fresh token and client
        if let Some(run_rest) = self.maybe_authed_run_rest(now) {
            return Ok(run_rest);
        }

        // TODO(phlip9): `std::hint::cold_path()` here when that stabilizes

        // Get an unexpired auth token. This is probably a new token, but we may
        // race with other tasks here, so we could also get a cached token.
        // Connecting to the user node only requires `GatewayProxy`.
        let auth_token =
            self.get_auth_token(now, LexeScope::GatewayProxy).await?;

        // Check again if another task concurrently swapped in a fresh client.
        // A little hacky, but significantly reduces the chance that we create
        // multiple clients.
        if let Some(run_rest) = self.maybe_authed_run_rest(now) {
            // TODO(phlip9): `std::hint::cold_path()` here when that stabilizes
            return Ok(run_rest);
        }

        // Build a new client with the new token
        let run_rest = RunRestClient::new(
            &self.inner.gateway_client,
            self.inner.run_url,
            auth_token,
            self.inner.tls_config.clone(),
        )
        .map_err(NodeApiError::bad_auth)?;
        let run_rest = Arc::new(run_rest);

        // Swap it in
        self.inner.run_rest.swap(Some(run_rest.clone()));

        Ok(run_rest)
    }

    /// Returns `Some(_)` if we already have an authenticated run rest client
    /// whose token is unexpired.
    fn maybe_authed_run_rest(
        &self,
        now: SystemTime,
    ) -> Option<Arc<RunRestClient>> {
        let maybe_run_rest = self.inner.run_rest.load_full();
        if let Some(run_rest) = maybe_run_rest
            && !run_rest.token_needs_refresh(now)
        {
            Some(run_rest)
        } else {
            None
        }
    }

    /// Get an unexpired auth token (maybe cached, maybe new) for the given
    /// `scope`.
    async fn get_auth_token(
        &self,
        now: SystemTime,
        scope: LexeScope,
    ) -> Result<TokenWithExpiration, NodeApiError> {
        self.inner
            .authenticator
            .get_token_with_exp(&self.inner.gateway_client, now, scope)
            .await
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
    ///
    /// This client doesn't automatically refresh its auth token, so avoid
    /// holding onto this client for too long.
    fn provision_rest_client(
        &self,
        provision_url: &str,
        auth_token: BearerAuthToken,
        measurement: Measurement,
    ) -> anyhow::Result<RestClient> {
        let proxy = static_proxy_config(
            &self.inner.gateway_client.gateway_url,
            provision_url,
            auth_token,
        )
        .context("Invalid proxy config")?;

        let tls_config = attest_client::user_node_provision_client_config(
            self.inner.use_sgx,
            self.inner.deploy_env,
            measurement,
        );

        let user_agent = self.inner.gateway_client.rest.user_agent().clone();
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
        // Register a new revocable client.
        let resp = self.create_revocable_client(req.clone()).await?;

        let client = RevocableClient {
            pubkey: resp.pubkey,
            created_at: resp.created_at,
            label: req.label,
            scope: req.scope,
            expires_at: req.expires_at,
            is_revoked: false,
        };

        let client_credentials = ClientCredentials::from(resp);

        Ok((client, client_credentials))
    }

    /// Get a [`LexeScope::GatewayProxy`] token for requests to the gateway.
    //
    // This helper exists because `GatewayClient::enclaves_to_provision` needs a
    // token, but `GatewayClient` doesn't hold a `BearerAuthenticator`.
    pub async fn get_gateway_token(&self) -> anyhow::Result<BearerAuthToken> {
        let now = SystemTime::now();
        self.inner
            .authenticator
            .get_token(&self.inner.gateway_client, now, LexeScope::GatewayProxy)
            .await
            .context("Failed to get gateway token")
    }
}

impl UserNodeProvisionApi for NodeClient {
    async fn provision(
        &self,
        measurement: Measurement,
        data: NodeProvisionRequest,
    ) -> Result<Empty, NodeApiError> {
        let now = SystemTime::now();
        let mr_short = measurement.short();
        let provision_dns = node_provision_dns(&mr_short);
        let provision_url = format!("https://{provision_dns}");

        // Create rest client on the fly.
        let auth_token = self
            .get_auth_token(now, LexeScope::GatewayProxy)
            .await?
            .token;
        let provision_rest = self
            .provision_rest_client(&provision_url, auth_token, measurement)
            .context("Failed to build provision rest client")
            .map_err(NodeApiError::provision)?;

        let req = provision_rest
            .post(format!("{provision_url}/user/provision"), &data);
        provision_rest.send(req).await
    }
}

impl UserNodeRunApi for NodeClient {
    async fn node_info(&self) -> Result<NodeInfo, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/v2/node_info");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn debug_info(&self) -> Result<DebugInfo, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/debug_info");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn list_channels(
        &self,
    ) -> Result<ListChannelsResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/list_channels");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn sign_message(
        &self,
        data: SignMsgRequest,
    ) -> Result<SignMsgResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/sign_message");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn verify_message(
        &self,
        data: VerifyMsgRequest,
    ) -> Result<VerifyMsgResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/verify_message");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn open_channel(
        &self,
        data: OpenChannelRequest,
    ) -> Result<OpenChannelResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/open_channel");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn open_channel_preflight(
        &self,
        data: OpenChannelPreflightRequest,
    ) -> Result<OpenChannelPreflightResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/open_channel_preflight");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn close_channel(
        &self,
        data: CloseChannelRequest,
    ) -> Result<Empty, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/close_channel");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn close_channel_preflight(
        &self,
        data: CloseChannelPreflightRequest,
    ) -> Result<CloseChannelPreflightResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/close_channel_preflight");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn create_invoice(
        &self,
        data: CreateInvoiceRequest,
    ) -> Result<CreateInvoiceResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/create_invoice");
        let req = run_rest.post(url, &data);
        run_rest.send(req).await
    }

    async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> Result<PayInvoiceResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_invoice");
        // `pay_invoice` may call `max_flow` which takes a long time.
        let req = run_rest
            .post(url, &req)
            .timeout(constants::MAX_FLOW_TIMEOUT + Duration::from_secs(2));
        run_rest.send(req).await
    }

    async fn pay_invoice_preflight(
        &self,
        req: PayInvoicePreflightRequest,
    ) -> Result<PayInvoicePreflightResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_invoice_preflight");
        // `pay_invoice_preflight` may call `max_flow` which takes a long time.
        let req = run_rest
            .post(url, &req)
            .timeout(constants::MAX_FLOW_TIMEOUT + Duration::from_secs(2));
        run_rest.send(req).await
    }

    async fn pay_onchain(
        &self,
        req: PayOnchainRequest,
    ) -> Result<PayOnchainResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_onchain");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn pay_onchain_preflight(
        &self,
        req: PayOnchainPreflightRequest,
    ) -> Result<PayOnchainPreflightResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_onchain_preflight");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> Result<CreateOfferResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/create_offer");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> Result<PayOfferResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_offer");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn pay_offer_preflight(
        &self,
        req: PayOfferPreflightRequest,
    ) -> Result<PayOfferPreflightResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/pay_offer_preflight");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn get_next_unused_address(
        &self,
    ) -> Result<GetNextUnusedAddressResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/get_next_unused_address");
        let req = run_rest.post(url, &Empty {});
        run_rest.send(req).await
    }

    async fn get_payments_by_indexes(
        &self,
        _: PaymentCreatedIndexes,
    ) -> Result<VecBasicPaymentV1, NodeApiError> {
        unimplemented!("Deprecated")
    }

    async fn get_new_payments(
        &self,
        _: GetNewPayments,
    ) -> Result<VecBasicPaymentV1, NodeApiError> {
        unimplemented!("Deprecated")
    }

    async fn get_updated_payments(
        &self,
        req: GetUpdatedPayments,
    ) -> Result<VecBasicPaymentV2, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/payments/updated");
        let req = run_rest.get(url, &req);
        run_rest.send(req).await
    }

    async fn get_payment_by_id(
        &self,
        req: PaymentIdStruct,
    ) -> Result<MaybeBasicPaymentV2, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/v1/payments/id");
        let req = run_rest.get(url, &req);
        run_rest.send(req).await
    }

    async fn update_personal_note(
        &self,
        req: UpdatePersonalNote,
    ) -> Result<Empty, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/payments/note");
        let req = run_rest.put(url, &req);
        run_rest.send(req).await
    }

    async fn list_revocable_clients(
        &self,
        req: ListRevocableClients,
    ) -> Result<RevocableClients, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/clients");
        let req = run_rest.get(url, &req);
        run_rest.send(req).await
    }

    async fn create_revocable_client(
        &self,
        req: CreateRevocableClientRequest,
    ) -> Result<CreateRevocableClientResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/clients");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn update_revocable_client(
        &self,
        req: UpdateClientRequest,
    ) -> Result<UpdateClientResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/clients");
        let req = run_rest.put(url, &req);
        run_rest.send(req).await
    }

    async fn list_broadcasted_txs(
        &self,
    ) -> Result<serde_json::Value, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/list_broadcasted_txs");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn backup_info(&self) -> Result<BackupInfo, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/backup");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn setup_gdrive(
        &self,
        req: SetupGDrive,
    ) -> Result<Empty, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/backup/gdrive");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn get_human_bitcoin_address(
        &self,
    ) -> Result<GetHumanBitcoinAddressResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/v2/human_bitcoin_address");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn get_human_bitcoin_address_v1(
        &self,
    ) -> Result<HumanBitcoinAddressV1, NodeApiError> {
        unimplemented!("Deprecated")
    }

    async fn upsert_custom_human_bitcoin_address(
        &self,
        req: UsernameStruct,
    ) -> Result<UpsertHumanBitcoinAddressResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/v2/human_bitcoin_address");
        let req = run_rest.put(url, &req);
        run_rest.send(req).await
    }

    async fn get_payment_address_v1(
        &self,
    ) -> Result<HumanBitcoinAddressV1, NodeApiError> {
        unimplemented!("Deprecated")
    }

    async fn list_nwc_clients(
        &self,
    ) -> Result<ListNwcClientResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/nwc_clients");
        let req = run_rest.get(url, &Empty {});
        run_rest.send(req).await
    }

    async fn create_nwc_client(
        &self,
        req: CreateNwcClientRequest,
    ) -> Result<CreateNwcClientResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/nwc_clients");
        let req = run_rest.post(url, &req);
        run_rest.send(req).await
    }

    async fn update_nwc_client(
        &self,
        req: UpdateNwcClientRequest,
    ) -> Result<UpdateNwcClientResponse, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/nwc_clients");
        let req = run_rest.put(url, &req);
        run_rest.send(req).await
    }

    async fn delete_nwc_client(
        &self,
        req: NostrPkStruct,
    ) -> Result<Empty, NodeApiError> {
        let run_rest = &self.authed_run_rest().await?.client;
        let run_url = &self.inner.run_url;
        let url = format!("{run_url}/user/nwc_clients");
        let req = run_rest.delete(url, &req);
        run_rest.send(req).await
    }
}

// --- impl RunRestClient --- //

impl RunRestClient {
    fn new(
        gateway_client: &GatewayClient,
        run_url: &str,
        auth_token: TokenWithExpiration,
        tls_config: rustls::ClientConfig,
    ) -> anyhow::Result<Self> {
        let TokenWithExpiration { expiration, token } = auth_token;
        let (from, to) = (gateway_client.rest.user_agent().clone(), "node-run");
        let proxy =
            static_proxy_config(&gateway_client.gateway_url, run_url, token)?;
        let client = RestClient::client_builder(&from)
            .proxy(proxy)
            .use_preconfigured_tls(tls_config.clone())
            .build()
            .context("Failed to build client")?;
        let client = RestClient::from_inner(client, from, to);

        Ok(Self {
            client,
            token_expiration: expiration,
        })
    }

    /// Returns `true` if we should refresh the token (i.e., it's expired or
    /// about to expire).
    fn token_needs_refresh(&self, now: SystemTime) -> bool {
        auth::helpers::token_needs_refresh(now, self.token_expiration)
    }
}

/// Build a static [`reqwest::Proxy`] config which proxies requests to the user
/// node via the lexe gateway CONNECT proxy and authenticates using the provided
/// bearer auth token.
///
/// User nodes are not exposed to the public internet. Instead, a secure
/// tunnel (TLS) is first established via the lexe gateway proxy to the
/// user's node only after they have successfully authenticated with Lexe.
///
/// Essentially, we have a TLS-in-TLS scheme:
///
/// - The outer layer terminates at Lexe's gateway proxy and prevents the public
///   internet from seeing auth tokens sent to the gateway proxy.
/// - The inner layer terminates inside the SGX enclave and prevents the Lexe
///   operators from snooping on or tampering with data sent to/from the app <->
///   node.
///
/// This function sets up a client-side [`reqwest::Proxy`] config which
/// looks for requests to the user node (i.e., urls starting with one of the
/// fake DNS names `{mr_short}.provision.lexe.app` or `run.lexe.app`) and
/// instructs `reqwest` to use an HTTPS CONNECT tunnel over which to send
/// the requests.
fn static_proxy_config(
    gateway_url: &str,
    node_url: &str,
    auth_token: BearerAuthToken,
) -> anyhow::Result<reqwest::Proxy> {
    let node_url = Url::parse(node_url).context("Invalid node url")?;
    let gateway_url = gateway_url.to_owned();

    // TODO(phlip9): include "Bearer " prefix in auth token
    let auth_header = http::HeaderValue::from_maybe_shared(ByteStr::from(
        format!("Bearer {auth_token}"),
    ))?;

    let proxy = reqwest::Proxy::custom(move |url| {
        // Proxy requests to the user node via the gateway CONNECT proxy
        if url_base_eq(url, &node_url) {
            Some(gateway_url.clone())
        } else {
            None
        }
    })
    // Authenticate with the gateway CONNECT proxy
    // `Proxy-Authorization: Bearer <token>`
    .custom_http_auth(auth_header);

    Ok(proxy)
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
