use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use axum::{
    Router,
    extract::State,
    routing::{get, post, put},
};
use lexe::{
    config::WalletEnvConfig,
    types::{
        auth::Credentials,
        bitcoin::{ClaimMethod, PaymentMethod},
        command::{
            AnalyzeRequest, CashAppBuyRequest, CashAppBuyResponse,
            ClaimableDetails as SdkClaimableDetails, ClientInfoResponse,
            CloseChannelRequest, CreateClientRequest, CreateClientResponse,
            CreateInvoiceRequest, CreateInvoiceResponse, CreateOfferRequest,
            CreateOfferResponse, GetHumanBitcoinAddressResponse,
            GetPaymentRequest, GetPaymentResponse, GetUpdatedPaymentsRequest,
            GetUpdatedPaymentsResponse, ListChannelsResponse,
            ListClientsResponse, ListPaymentsResponse, NodeInfo,
            OpenChannelRequest, OpenChannelResponse, PayInvoiceRequest,
            PayLnurlRequest as SdkPayLnurlRequest, PayOfferRequest,
            PayRequest as SdkPayRequest, PayableDetails as SdkPayableDetails,
            PaymentSyncSummary, RevokeClientRequest,
            UpdateClientRequest as SdkUpdateClientRequest,
            UpdatePersonalNoteRequest,
            WithdrawLnurlRequest as SdkWithdrawLnurlRequest,
        },
        payment::Payment,
    },
    wallet::LexeWallet,
};
use lexe_api::{
    error::{SdkApiError, SdkErrorKind},
    server::{LxJson, extract::LxQuery},
    types::{Empty, payments::PaymentCreatedIndex},
};
use tokio::sync::mpsc;
use tracing::{debug, instrument, warn};

use crate::{
    api::{
        AnalyzeResponse, ClaimableDetails, HealthCheckResponse,
        ListPaymentsRequest, PayLnurlRequest, PayRequest, PayableDetails,
        SignupRequest, UpdateClientRequest, WithdrawLnurlRequest,
    },
    extract::{
        CredentialsExtractor, WalletAndCredentialsExtractor, WalletExtractor,
    },
    webhook::{TrackRequest, WalletCache},
};

/// A percent encoding set intended for use in HTTP query parameters.
const HTTP_PERCENT_ENCODE_SET: percent_encoding::AsciiSet =
    percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');

pub(crate) struct RouterState {
    pub sidecar_url: String,
    /// The data directory for persisted state.
    pub data_dir: PathBuf,
    /// The default [`LexeWallet`] and [`Credentials`] from env/CLI.
    /// Used when no per-request credentials are provided.
    pub default: Option<(Arc<LexeWallet>, Arc<Credentials>)>,
    /// Shared cache of [`LexeWallet`]s
    pub wallet_cache: Arc<Mutex<WalletCache>>,
    pub wallet_env_config: WalletEnvConfig,
    /// Channel to send track requests to the webhook sender.
    pub webhook_tx: Option<mpsc::Sender<TrackRequest>>,
}

pub(crate) fn router(state: Arc<RouterState>) -> Router<()> {
    // NOTE: If making a breaking change, bump the version of *all* endpoints.
    // This is because we don't want to trip up dumb AIs which fail to
    // distinguish between v1/v2. A consistent version is more reliable.
    Router::new()
        // v2
        .route("/v2/health", get(sidecar::health))
        .route("/v2/node/signup", put(node::signup))
        .route("/v2/node/provision", put(node::provision))
        .route("/v2/node/node_info", get(node::node_info))
        .route("/v2/node/analyze", get(node::analyze))
        .route("/v2/node/pay", post(node::pay))
        .route("/v2/node/create_invoice", post(node::create_invoice))
        .route("/v2/node/pay_invoice", post(node::pay_invoice))
        .route("/v2/node/create_offer", post(node::create_offer))
        .route("/v2/node/pay_offer", post(node::pay_offer))
        .route("/v2/node/pay_lnurl", post(node::pay_lnurl))
        .route("/v2/node/withdraw_lnurl", post(node::withdraw_lnurl))
        .route("/v2/node/buy_with_cash_app", post(node::buy_with_cash_app))
        .route(
            "/v2/node/human_bitcoin_address",
            get(node::get_human_bitcoin_address),
        )
        .route("/v2/node/sync_payments", put(node::sync_payments))
        .route("/v2/node/list_payments", get(node::list_payments))
        .route("/v2/node/clear_payments", post(node::clear_payments))
        .route("/v2/node/payment", get(node::get_payment))
        .route("/v2/node/updated_payments", get(node::get_updated_payments))
        .route(
            "/v2/node/update_personal_note",
            post(node::update_personal_note),
        )
        .route("/v2/node/list_clients", get(node::list_clients))
        .route("/v2/node/create_client", post(node::create_client))
        .route("/v2/node/update_client", put(node::update_client))
        .route("/v2/node/revoke_client", post(node::revoke_client))
        .route("/v2/node/list_channels", get(node::list_channels))
        .route("/v2/node/open_channel", post(node::open_channel))
        .route("/v2/node/close_channel", post(node::close_channel))
        // v1 (legacy)
        .route("/v1/health", get(sidecar::health))
        .route("/v1/node/node_info", get(node::node_info))
        .route("/v1/node/create_invoice", post(node::create_invoice))
        .route("/v1/node/pay_invoice", post(node::pay_invoice))
        .route("/v1/node/payment", get(node::get_payment_v1))
        .with_state(state)
}

mod sidecar {
    use super::*;

    #[instrument(skip_all, name = "(health)")]
    pub(crate) async fn health(
        state: State<Arc<RouterState>>,
        CredentialsExtractor(maybe_credentials): CredentialsExtractor,
    ) -> Result<LxJson<HealthCheckResponse>, SdkApiError> {
        let has_default = state.default.is_some();
        let has_request_credentials = maybe_credentials.is_some();

        let status = if has_default || has_request_credentials {
            Cow::from("ok")
        } else {
            Cow::from(
                "warning: No client credentials configured. \
                 Credentials must be set per-request via the Authorization \
                 header. Alternatively, one of the following flags \
                 can be set:\n\
                 \t--client-credentials / $LEXE_CLIENT_CREDENTIALS\n\
                 \t--client-credentials-path / $LEXE_CLIENT_CREDENTIALS_PATH\n\
                 \t--root-seed / $LEXE_ROOT_SEED\n\
                 \t--root-seed-path / $LEXE_ROOT_SEED_PATH",
            )
        };

        Ok(LxJson(HealthCheckResponse { status }))
    }
}

mod node {
    use super::*;

    #[instrument(skip_all, name = "(signup)")]
    pub(crate) async fn signup(
        State(state): State<Arc<RouterState>>,
        CredentialsExtractor(maybe_credentials): CredentialsExtractor,
        req: Option<LxJson<SignupRequest>>,
    ) -> Result<LxJson<Empty>, SdkApiError> {
        // Ensure that per-request credentials weren't passed
        if maybe_credentials.is_some() {
            return Err(SdkApiError::bad_auth(
                "Signup requires root seed credentials, but per-request client \
                 credentials were provided. \
                 Ensure that the Authorization header is empty.",
            ));
        };
        let (wallet, root_seed) = match state
            .default
            .as_ref()
            .map(|(w, creds)| (w.as_ref(), creds.as_ref()))
        {
            None =>
                return Err(SdkApiError::bad_auth(
                    "Signup requires root seed credentials, but none were \
                     provided.\n\
                     Ensure that one of the following are set:\n\
                     \t--root-seed / $LEXE_ROOT_SEED\n\
                     \t--root-seed-path / $LEXE_ROOT_SEED_PATH",
                )),
            Some((_, Credentials::ClientCredentials(_))) =>
                return Err(SdkApiError::bad_auth(
                    "Signup requires root seed credentials, but client \
                     credentials were provided instead.\n\
                     Ensure that none of the following are set,\n\
                     \t--client-credentials / $LEXE_CLIENT_CREDENTIALS\n\
                     \t--client-credentials-path / $LEXE_CLIENT_CREDENTIALS_PATH\n\
                     and ensure that only one of the following are set:\n\
                     \t--root-seed / $LEXE_ROOT_SEED\n\
                     \t--root-seed-path / $LEXE_ROOT_SEED_PATH",
                )),
            Some((w, Credentials::RootSeed(seed))) => (w, seed),
        };
        let partner_pk = req.and_then(|LxJson(req)| req.partner_pk);
        wallet
            .signup(root_seed, partner_pk)
            .await
            .map_err(SdkApiError::command)?;

        Ok(LxJson(Empty {}))
    }

    #[instrument(skip_all, name = "(provision)")]
    pub(crate) async fn provision(
        State(_): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
    ) -> Result<LxJson<Empty>, SdkApiError> {
        wallet
            .provision(Credentials::as_ref(&credentials))
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(Empty {}))
    }

    #[instrument(skip_all, name = "(node-info)")]
    pub(crate) async fn node_info(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<NodeInfo>, SdkApiError> {
        let info = wallet.node_info().await.map_err(SdkApiError::command)?;
        Ok(LxJson(info))
    }

    #[instrument(skip_all, name = "(analyze)")]
    pub(crate) async fn analyze(
        State(state): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxQuery(req): LxQuery<AnalyzeRequest>,
    ) -> Result<LxJson<AnalyzeResponse>, SdkApiError> {
        let resp = wallet.analyze(req).await.map_err(SdkApiError::command)?;
        let payables = resp
            .payables
            .into_iter()
            .map(|details| {
                let SdkPayableDetails {
                    payable,
                    method,
                    description,
                    amount,
                    min_amount,
                    max_amount,
                    expires_at,
                } = details;

                // Construct callback
                let sidecar_url = &state.sidecar_url;
                let encoded = percent_encoding::utf8_percent_encode(
                    &payable,
                    &HTTP_PERCENT_ENCODE_SET,
                );
                let callback =
                    format!("{sidecar_url}/v2/node/pay?payable={encoded}");

                // Translate method
                let kind = method.kind().to_string();

                // Get method-specific string
                let mut invoice = None;
                let mut offer = None;
                let mut lnurl = None;
                let mut onchain = None;
                match method {
                    PaymentMethod::Invoice { invoice: inv } =>
                        invoice = Some(inv.to_string()),
                    PaymentMethod::Offer { offer: off, .. } =>
                        offer = Some(off.to_string()),
                    PaymentMethod::LnurlPay { lnurl: uri, .. } =>
                        lnurl = Some(uri),
                    PaymentMethod::Onchain { address, .. } =>
                        onchain = Some(address.to_string()),
                }

                PayableDetails {
                    callback,
                    kind,
                    invoice,
                    offer,
                    lnurl,
                    onchain,
                    description,
                    amount,
                    min_amount,
                    max_amount,
                    expires_at,
                }
            })
            .collect();

        let claimables = resp
            .claimables
            .into_iter()
            .map(|details| {
                let SdkClaimableDetails {
                    claimable,
                    method,
                    description,
                    min_amount,
                    max_amount,
                } = details;

                // Construct callback
                let sidecar_url = &state.sidecar_url;
                let encoded = percent_encoding::utf8_percent_encode(
                    &claimable,
                    &HTTP_PERCENT_ENCODE_SET,
                );
                let callback = format!(
                    "{sidecar_url}/v2/node/withdraw_lnurl?lnurl={encoded}"
                );

                // Translate method
                let kind = method.kind().to_string();

                // Get method-specific string
                let lnurl = match method {
                    ClaimMethod::LnurlWithdraw { lnurl: uri, .. } => Some(uri),
                };

                ClaimableDetails {
                    callback,
                    kind,
                    lnurl,
                    description,
                    min_amount,
                    max_amount,
                }
            })
            .collect();

        Ok(LxJson(AnalyzeResponse {
            payables,
            claimables,
        }))
    }

    #[instrument(skip_all, name = "(pay)")]
    pub(crate) async fn pay(
        State(state): State<Arc<RouterState>>,
        LxQuery(params): LxQuery<PayRequest>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        maybe_req: Option<LxJson<PayRequest>>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let req = maybe_req.map(|LxJson(r)| r);

        // Ensure that query params and request body don't conflict
        let merged = if let Some(pay_req) = req {
            pay_req
                .merge_no_dups(params)
                .with_context(|| {
                    "Pay request argument can be passed via the body or \
                         through query parameters, but not both"
                })
                .map_err(SdkApiError::command)?
        } else {
            params
        };

        // Ensure we have a payable to pay
        let payable = merged.payable.ok_or_else(|| {
            SdkApiError::command(
                "A payable string must be specified via the request body or \
                 query parameters",
            )
        })?;

        debug!(
            ?payable,
            ?merged.amount,
            ?merged.message,
            ?merged.personal_note,
            "Merged pay request"
        );

        let req = SdkPayRequest {
            payable,
            amount: merged.amount,
            message: merged.message,
            personal_note: merged.personal_note,
        };

        let resp = wallet.pay(req).await.map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(create-invoice)")]
    pub(crate) async fn create_invoice(
        State(state): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        LxJson(req): LxJson<CreateInvoiceRequest>,
    ) -> Result<LxJson<CreateInvoiceResponse>, SdkApiError> {
        let resp = wallet
            .create_invoice(req)
            .await
            .map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(pay-invoice)")]
    pub(crate) async fn pay_invoice(
        State(state): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        LxJson(req): LxJson<PayInvoiceRequest>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let resp = wallet
            .pay_invoice(req)
            .await
            .map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(create-offer)")]
    pub(crate) async fn create_offer(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<CreateOfferRequest>,
    ) -> Result<LxJson<CreateOfferResponse>, SdkApiError> {
        let resp = wallet
            .create_offer(req)
            .await
            .map_err(SdkApiError::command)?;

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(pay-offer)")]
    pub(crate) async fn pay_offer(
        State(state): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        LxJson(req): LxJson<PayOfferRequest>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let resp = wallet.pay_offer(req).await.map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(pay-lnurl)")]
    pub(crate) async fn pay_lnurl(
        State(state): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        LxJson(req): LxJson<PayLnurlRequest>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let resp = wallet
            .pay_lnurl(SdkPayLnurlRequest::from(req))
            .await
            .map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(withdraw-lnurl)")]
    pub(crate) async fn withdraw_lnurl(
        State(state): State<Arc<RouterState>>,
        LxQuery(params): LxQuery<WithdrawLnurlRequest>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        maybe_req: Option<LxJson<WithdrawLnurlRequest>>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let req = maybe_req.map(|LxJson(r)| r);

        // Ensure that query params and request body don't conflict
        let merged = if let Some(withdraw_req) = req {
            withdraw_req
                .merge_no_dups(params)
                .with_context(|| {
                    "Withdraw request argument can be passed via the body or \
                     through query parameters, but not both"
                })
                .map_err(SdkApiError::command)?
        } else {
            params
        };

        // Ensure we have an lnurl to withdraw from
        if merged.lnurl.is_none() {
            return Err(SdkApiError::command(
                "An lnurl string must be specified via the request body or \
                 query parameters",
            ));
        }

        let resp = wallet
            .withdraw_lnurl(SdkWithdrawLnurlRequest::from(merged))
            .await
            .map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(buy-with-cash-app)")]
    pub(crate) async fn buy_with_cash_app(
        State(state): State<Arc<RouterState>>,
        WalletAndCredentialsExtractor {
            wallet,
            credentials,
        }: WalletAndCredentialsExtractor,
        LxJson(req): LxJson<CashAppBuyRequest>,
    ) -> Result<LxJson<CashAppBuyResponse>, SdkApiError> {
        let resp = wallet
            .buy_with_cash_app(req)
            .await
            .map_err(SdkApiError::command)?;

        helpers::try_track_payment(&state, credentials, resp.index);

        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(get-human-bitcoin-address)")]
    pub(crate) async fn get_human_bitcoin_address(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<GetHumanBitcoinAddressResponse>, SdkApiError> {
        let resp = wallet
            .get_human_bitcoin_address()
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(sync-payments)")]
    pub(crate) async fn sync_payments(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<PaymentSyncSummary>, SdkApiError> {
        let resp =
            wallet.sync_payments().await.map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(list-payments)")]
    pub(crate) async fn list_payments(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxQuery(req): LxQuery<ListPaymentsRequest>,
    ) -> Result<LxJson<ListPaymentsResponse>, SdkApiError> {
        let resp = wallet
            .list_payments(
                &req.filter,
                req.order,
                req.limit,
                req.after.as_ref(),
            )
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(clear-payments)")]
    pub(crate) async fn clear_payments(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<Empty>, SdkApiError> {
        wallet.clear_payments().map_err(SdkApiError::command)?;
        Ok(LxJson(Empty {}))
    }

    /// Legacy: Returns `{ "payment": null }` if not found.
    #[instrument(skip_all, name = "(get-payment-v1)")]
    pub(crate) async fn get_payment_v1(
        state: State<Arc<RouterState>>,
        wallet: WalletExtractor,
        req: LxQuery<GetPaymentRequest>,
    ) -> Result<LxJson<GetPaymentResponse>, SdkApiError> {
        // Wraps the v2 logic to return `{ "payment": null }` if not found.
        match get_payment(state, wallet, req).await {
            Ok(LxJson(payment)) => Ok(LxJson(GetPaymentResponse {
                payment: Some(payment),
            })),
            Err(e) if e.kind == SdkErrorKind::NotFound =>
                Ok(LxJson(GetPaymentResponse { payment: None })),
            Err(e) => Err(e),
        }
    }

    /// NOTE: For the v2 endpoint and above, we return the response as a
    /// [`Payment`] rather than a [`GetPaymentResponse`], because the
    /// `{ "payment": { ... } }` nesting trips up dumb AIs when vibe-coding on
    /// the Sidecar SDK, as discovered by Mat Balez et al. If the payment is
    /// missing, we use HTTP 404 to indicate this.
    ///
    /// If we need to add more fields to the response which don't fit in
    /// [`Payment`], we can always reintroduce the response type but with
    /// `#[serde(flatten)]` on the [`Payment`] field (since missing payments
    /// are now indicated by HTTP 404), or do another version bump.
    #[instrument(skip_all, name = "(get-payment)")]
    pub(crate) async fn get_payment(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxQuery(req): LxQuery<GetPaymentRequest>,
    ) -> Result<LxJson<Payment>, SdkApiError> {
        let resp = wallet
            .get_payment(req)
            .await
            .map_err(SdkApiError::command)?;
        let payment = resp
            .payment
            .ok_or_else(|| SdkApiError::not_found("Payment not found"))?;
        Ok(LxJson(payment))
    }

    #[instrument(skip_all, name = "(get-updated-payments)")]
    pub(crate) async fn get_updated_payments(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxQuery(req): LxQuery<GetUpdatedPaymentsRequest>,
    ) -> Result<LxJson<GetUpdatedPaymentsResponse>, SdkApiError> {
        let resp = wallet
            .get_updated_payments(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(update-personal-note)")]
    pub(crate) async fn update_personal_note(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<UpdatePersonalNoteRequest>,
    ) -> Result<LxJson<Empty>, SdkApiError> {
        wallet
            .update_personal_note(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(Empty {}))
    }

    #[instrument(skip_all, name = "(list-clients)")]
    pub(crate) async fn list_clients(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<ListClientsResponse>, SdkApiError> {
        let resp = wallet.list_clients().await.map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(create-client)")]
    pub(crate) async fn create_client(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<CreateClientRequest>,
    ) -> Result<LxJson<CreateClientResponse>, SdkApiError> {
        let resp = wallet
            .create_client(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(update-client)")]
    pub(crate) async fn update_client(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<UpdateClientRequest>,
    ) -> Result<LxJson<ClientInfoResponse>, SdkApiError> {
        let req = SdkUpdateClientRequest::new(
            req.client_pk,
            req.label,
            req.clear_label,
            req.expires_at,
            req.clear_expiration,
        )
        .map_err(SdkApiError::command)?;
        let resp = wallet
            .update_client(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(revoke-client)")]
    pub(crate) async fn revoke_client(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<RevokeClientRequest>,
    ) -> Result<LxJson<ClientInfoResponse>, SdkApiError> {
        let resp = wallet
            .revoke_client(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(list-channels)")]
    pub(crate) async fn list_channels(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
    ) -> Result<LxJson<ListChannelsResponse>, SdkApiError> {
        let resp =
            wallet.list_channels().await.map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(open-channel)")]
    pub(crate) async fn open_channel(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<OpenChannelRequest>,
    ) -> Result<LxJson<OpenChannelResponse>, SdkApiError> {
        let resp = wallet
            .open_channel(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(resp))
    }

    #[instrument(skip_all, name = "(close-channel)")]
    pub(crate) async fn close_channel(
        State(_): State<Arc<RouterState>>,
        WalletExtractor(wallet): WalletExtractor,
        LxJson(req): LxJson<CloseChannelRequest>,
    ) -> Result<LxJson<Empty>, SdkApiError> {
        wallet
            .close_channel(req)
            .await
            .map_err(SdkApiError::command)?;
        Ok(LxJson(Empty {}))
    }
}

mod helpers {
    use super::*;
    use crate::webhook::CredentialsOrDefault;

    /// Try to track a payment for webhook notifications.
    ///
    /// No-op if webhooks are not configured.
    pub(super) fn try_track_payment(
        state: &RouterState,
        credentials: Arc<Credentials>,
        index: PaymentCreatedIndex,
    ) {
        let Some(tx) = &state.webhook_tx else { return };

        let req = TrackRequest {
            creds_or_default: CredentialsOrDefault::from(&*credentials),
            payment_created_index: index,
        };

        if tx.try_send(req).is_err() {
            warn!("Webhook channel full, payment not tracked");
        }
    }
}
