use anyhow::Context;
use common::{
    api::{
        revocable_clients::{
            CreateRevocableClientRequest as CreateRevocableClientRequestRs,
            GetRevocableClients, UpdateClientRequest as UpdateClientRequestRs,
        },
        Empty,
    },
    env::DeployEnv,
    ln::payments::PaymentIndex as PaymentIndexRs,
    rng::SysRng,
    root_seed::RootSeed as RootSeedRs,
};
use flutter_rust_bridge::{frb, RustOpaqueNom};
use lexe_api::{
    def::{AppGatewayApi, AppNodeRunApi},
    models::command::{
        GetAddressResponse, OpenChannelRequest as OpenChannelRequestRs,
        PayInvoiceRequest as PayInvoiceRequestRs,
        PayOnchainRequest as PayOnchainRequestRs,
        PreflightOpenChannelRequest as PreflightOpenChannelRequestRs,
        UpdatePaymentNote as UpdatePaymentNoteRs,
    },
};
use tracing::instrument;

use crate::ffi::{
    api::{
        CloseChannelRequest, CreateClientRequest, CreateClientResponse,
        CreateInvoiceRequest, CreateInvoiceResponse, CreateOfferRequest,
        CreateOfferResponse, FiatRates, ListChannelsResponse, NodeInfo,
        OpenChannelRequest, OpenChannelResponse, PayInvoiceRequest,
        PayInvoiceResponse, PayOnchainRequest, PayOnchainResponse,
        PreflightCloseChannelRequest, PreflightCloseChannelResponse,
        PreflightOpenChannelRequest, PreflightOpenChannelResponse,
        PreflightPayInvoiceRequest, PreflightPayInvoiceResponse,
        PreflightPayOnchainRequest, PreflightPayOnchainResponse,
        UpdateClientRequest, UpdatePaymentNote,
    },
    settings::SettingsDb,
    types::{
        AppUserInfo, Config, Payment, PaymentIndex, RevocableClient, RootSeed,
        ShortPayment, ShortPaymentAndIndex,
    },
};
pub(crate) use crate::{app::App, settings::SettingsDb as SettingsDbRs};

/// The `AppHandle` is a Dart representation of an [`App`] instance.
pub struct AppHandle {
    pub inner: RustOpaqueNom<App>,
}

impl AppHandle {
    fn new(app: App) -> Self {
        Self {
            inner: RustOpaqueNom::new(app),
        }
    }

    pub async fn load(config: Config) -> anyhow::Result<Option<AppHandle>> {
        Ok(App::load(&mut SysRng::new(), config.into())
            .await
            .context("Failed to load saved App state")?
            .map(AppHandle::new))
    }

    pub async fn restore(
        config: Config,
        google_auth_code: String,
        root_seed: RootSeed,
    ) -> anyhow::Result<AppHandle> {
        // Ignored in local dev.
        //
        // Single-use `serverAuthCode` from Google OAuth 2 consent flow, used by
        // the enclave to get access+refresh tokens.
        let google_auth_code = match DeployEnv::from(config.deploy_env) {
            DeployEnv::Dev => None,
            DeployEnv::Prod | DeployEnv::Staging => Some(google_auth_code),
        };

        App::restore(
            &mut SysRng::new(),
            config.into(),
            google_auth_code,
            &root_seed.inner,
        )
        .await
        .context("Failed to restore wallet")
        .map(Self::new)
    }

    pub async fn signup(
        config: Config,
        google_auth_code: String,
        password: &str,
        signup_code: Option<String>,
    ) -> anyhow::Result<AppHandle> {
        // Ignored in local dev.
        //
        // Single-use `serverAuthCode` from Google OAuth 2 consent flow, used by
        // the enclave to get access+refresh tokens.
        let google_auth_code = match DeployEnv::from(config.deploy_env) {
            DeployEnv::Dev => None,
            DeployEnv::Prod | DeployEnv::Staging => Some(google_auth_code),
        };

        let mut rng = SysRng::new();
        let root_seed = RootSeedRs::from_rng(&mut rng);
        App::signup(
            &mut rng,
            config.into(),
            &root_seed,
            google_auth_code,
            Some(password),
            signup_code,
        )
        .await
        .context("Failed to generate and signup new wallet")
        .map(Self::new)
    }

    #[frb(sync)]
    pub fn settings_db(&self) -> SettingsDb {
        SettingsDb::new(self.inner.settings_db())
    }

    #[frb(sync)]
    pub fn user_info(&self) -> AppUserInfo {
        let (user_pk, node_pk, node_pk_proof) = self.inner.user_info();
        AppUserInfo {
            user_pk,
            node_pk,
            node_pk_proof,
        }
    }

    #[instrument(skip_all, name = "(node-info)")]
    pub async fn node_info(&self) -> anyhow::Result<NodeInfo> {
        self.inner
            .node_client()
            .node_info()
            .await
            .map(NodeInfo::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(list-channels)")]
    pub async fn list_channels(&self) -> anyhow::Result<ListChannelsResponse> {
        self.inner
            .node_client()
            .list_channels()
            .await
            .map(ListChannelsResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(open-channel)")]
    pub async fn open_channel(
        &self,
        req: OpenChannelRequest,
    ) -> anyhow::Result<OpenChannelResponse> {
        let req = OpenChannelRequestRs::try_from(req)?;
        self.inner
            .node_client()
            .open_channel(req)
            .await
            .map(OpenChannelResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-open-channel)")]
    pub async fn preflight_open_channel(
        &self,
        req: PreflightOpenChannelRequest,
    ) -> anyhow::Result<PreflightOpenChannelResponse> {
        let req = PreflightOpenChannelRequestRs::try_from(req)?;
        self.inner
            .node_client()
            .preflight_open_channel(req)
            .await
            .map(PreflightOpenChannelResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(close-channel)")]
    pub async fn close_channel(
        &self,
        req: CloseChannelRequest,
    ) -> anyhow::Result<()> {
        self.inner
            .node_client()
            .close_channel(req.try_into()?)
            .await
            .map(|Empty {}| ())
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-close-channel)")]
    pub async fn preflight_close_channel(
        &self,
        req: PreflightCloseChannelRequest,
    ) -> anyhow::Result<PreflightCloseChannelResponse> {
        self.inner
            .node_client()
            .preflight_close_channel(req.try_into()?)
            .await
            .map(PreflightCloseChannelResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(fiat-rates)")]
    pub async fn fiat_rates(&self) -> anyhow::Result<FiatRates> {
        self.inner
            .gateway_client()
            .get_fiat_rates()
            .await
            .map(FiatRates::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(pay-onchain)")]
    pub async fn pay_onchain(
        &self,
        req: PayOnchainRequest,
    ) -> anyhow::Result<PayOnchainResponse> {
        let req = PayOnchainRequestRs::try_from(req)?;
        let cid = req.cid;
        self.inner
            .node_client()
            .pay_onchain(req)
            .await
            .map(|resp| PayOnchainResponse::from_cid_and_response(cid, resp))
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-pay-onchain)")]
    pub async fn preflight_pay_onchain(
        &self,
        req: PreflightPayOnchainRequest,
    ) -> anyhow::Result<PreflightPayOnchainResponse> {
        self.inner
            .node_client()
            .preflight_pay_onchain(req.try_into()?)
            .await
            .map(PreflightPayOnchainResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(get-address)")]
    pub async fn get_address(&self) -> anyhow::Result<String> {
        self.inner
            .node_client()
            .get_address()
            .await
            .map(|GetAddressResponse { addr }| addr)
            .map(|addr| addr.assume_checked_ref().to_string())
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(create-invoice)")]
    pub async fn create_invoice(
        &self,
        req: CreateInvoiceRequest,
    ) -> anyhow::Result<CreateInvoiceResponse> {
        self.inner
            .node_client()
            .create_invoice(req.try_into()?)
            .await
            // TODO(phlip9): return new PaymentIndex
            .map(CreateInvoiceResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-pay-invoice)")]
    pub async fn preflight_pay_invoice(
        &self,
        req: PreflightPayInvoiceRequest,
    ) -> anyhow::Result<PreflightPayInvoiceResponse> {
        self.inner
            .node_client()
            .preflight_pay_invoice(req.try_into()?)
            .await
            .map(PreflightPayInvoiceResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(pay-invoice)")]
    pub async fn pay_invoice(
        &self,
        req: PayInvoiceRequest,
    ) -> anyhow::Result<PayInvoiceResponse> {
        let req = PayInvoiceRequestRs::try_from(req)?;
        let id = req.invoice.payment_id();
        self.inner
            .node_client()
            .pay_invoice(req)
            .await
            .map(|resp| PayInvoiceResponse::from_id_and_response(id, resp))
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(create-offer)")]
    pub async fn create_offer(
        &self,
        req: CreateOfferRequest,
    ) -> anyhow::Result<CreateOfferResponse> {
        self.inner
            .node_client()
            .create_offer(req.try_into()?)
            .await
            .map(CreateOfferResponse::from)
            .map_err(anyhow::Error::new)
    }

    /// Delete both the local payment state and the on-disk payment db.
    pub fn delete_payment_db(&self) -> anyhow::Result<()> {
        let mut db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.delete().context("Failed to delete PaymentDb")
    }

    /// Sync the local payment DB to the remote node.
    ///
    /// Returns `true` if any payment changed, so we know whether to reload the
    /// payment list UI.
    pub async fn sync_payments(&self) -> anyhow::Result<bool> {
        self.inner
            .sync_payments()
            .await
            .map(|summary| summary.any_changes())
    }

    pub fn get_vec_idx_by_payment_index(
        &self,
        payment_index: PaymentIndex,
    ) -> Option<usize> {
        let payment_index = PaymentIndexRs::try_from(payment_index).ok()?;
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().get_vec_idx_by_payment_index(&payment_index)
    }

    #[frb(sync)]
    pub fn get_payment_by_vec_idx(&self, vec_idx: usize) -> Option<Payment> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock
            .state()
            .get_payment_by_vec_idx(vec_idx)
            .map(Payment::from)
    }

    #[frb(sync)]
    pub fn get_short_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPaymentAndIndex> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().get_payment_by_scroll_idx(scroll_idx).map(
            |(vec_idx, payment)| ShortPaymentAndIndex {
                vec_idx,
                payment: ShortPayment::from(payment),
            },
        )
    }

    #[frb(sync)]
    pub fn get_pending_short_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPaymentAndIndex> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock
            .state()
            .get_pending_payment_by_scroll_idx(scroll_idx)
            .map(|(vec_idx, payment)| ShortPaymentAndIndex {
                vec_idx,
                payment: ShortPayment::from(payment),
            })
    }

    #[frb(sync)]
    pub fn get_finalized_short_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPaymentAndIndex> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock
            .state()
            .get_finalized_payment_by_scroll_idx(scroll_idx)
            .map(|(vec_idx, payment)| ShortPaymentAndIndex {
                vec_idx,
                payment: ShortPayment::from(payment),
            })
    }

    #[frb(sync)]
    pub fn get_pending_not_junk_short_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPaymentAndIndex> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock
            .state()
            .get_pending_not_junk_payment_by_scroll_idx(scroll_idx)
            .map(|(vec_idx, payment)| ShortPaymentAndIndex {
                vec_idx,
                payment: ShortPayment::from(payment),
            })
    }

    #[frb(sync)]
    pub fn get_finalized_not_junk_short_payment_by_scroll_idx(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPaymentAndIndex> {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock
            .state()
            .get_finalized_not_junk_payment_by_scroll_idx(scroll_idx)
            .map(|(vec_idx, payment)| ShortPaymentAndIndex {
                vec_idx,
                payment: ShortPayment::from(payment),
            })
    }

    #[frb(sync)]
    pub fn get_num_payments(&self) -> usize {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().num_payments()
    }

    #[frb(sync)]
    pub fn get_num_pending_payments(&self) -> usize {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().num_pending()
    }

    #[frb(sync)]
    pub fn get_num_finalized_payments(&self) -> usize {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().num_finalized()
    }

    #[frb(sync)]
    pub fn get_num_pending_not_junk_payments(&self) -> usize {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().num_pending_not_junk()
    }

    #[frb(sync)]
    pub fn get_num_finalized_not_junk_payments(&self) -> usize {
        let db_lock = self.inner.payment_db().lock().unwrap();
        db_lock.state().num_finalized_not_junk()
    }

    #[instrument(skip_all, name = "(update-payment-note)")]
    pub async fn update_payment_note(
        &self,
        req: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        let req = UpdatePaymentNoteRs::try_from(req)?;
        // Update remote store first
        self.inner
            .node_client()
            .update_payment_note(req.clone())
            .await
            .map(|Empty {}| ())
            .map_err(anyhow::Error::new)?;
        // Update local store after
        self.inner
            .payment_db()
            .lock()
            .unwrap()
            .update_payment_note(req)
    }

    #[instrument(skip_all, name = "(create-client)")]
    pub async fn create_client(
        &self,
        req: CreateClientRequest,
    ) -> anyhow::Result<CreateClientResponse> {
        let req = CreateRevocableClientRequestRs::from(req);
        let (client, client_credentials) = self
            .inner
            .node_client()
            .create_client_credentials(req)
            .await?;

        Ok(CreateClientResponse {
            client: RevocableClient::from(client),
            credentials: client_credentials.to_base64_blob(),
        })
    }

    #[instrument(skip_all, name = "(list-clients)")]
    pub async fn list_clients(&self) -> anyhow::Result<Vec<RevocableClient>> {
        // Only care about unrevoked and unexpired clients
        let req = GetRevocableClients { valid_only: true };
        let resp = self.inner.node_client().get_revocable_clients(req).await?;
        let clients = resp
            .clients
            .into_values()
            .map(RevocableClient::from)
            .collect();
        Ok(clients)
    }

    #[instrument(skip_all, name = "(update-client)")]
    pub async fn update_client(
        &self,
        req: UpdateClientRequest,
    ) -> anyhow::Result<()> {
        let req = UpdateClientRequestRs::try_from(req)?;
        let _resp = self
            .inner
            .node_client()
            .update_revocable_client(req)
            .await?;
        Ok(())
    }
}
