use anyhow::Context;
use common::{
    api::{
        revocable_clients::{
            CreateRevocableClientRequest as CreateRevocableClientRequestRs,
            GetRevocableClients, UpdateClientRequest as UpdateClientRequestRs,
        },
        user::UserPk,
    },
    env::DeployEnv,
    ln::amount::Amount,
    rng::SysRng,
};
use flutter_rust_bridge::RustOpaqueNom;
use hex::FromHex;
use lexe_api::{
    def::{AppGatewayApi, AppNodeRunApi},
    models::command::{
        GetAddressResponse, OpenChannelRequest as OpenChannelRequestRs,
        PayInvoiceRequest as PayInvoiceRequestRs,
        PayOfferRequest as PayOfferRequestRs,
        PayOnchainRequest as PayOnchainRequestRs,
        PreflightOpenChannelRequest as PreflightOpenChannelRequestRs,
        UpdatePaymentNote as UpdatePaymentNoteRs,
    },
    types::{
        Empty,
        lnurl::LnurlPayRequest as LnurlPayRequestRs,
        payments::{LxPaymentId, PaymentCreatedIndex as PaymentCreatedIndexRs},
        username::{
            Username as UsernameRs, UsernameStruct as UsernameStructRs,
        },
    },
};
use tracing::instrument;

pub(crate) use crate::{
    app::App, app_data::AppDataRs, db::WritebackDb as WritebackDbRs,
    settings::SettingsRs,
};
use crate::{
    app::AppConfig,
    ffi::{
        api::{
            CloseChannelRequest, CreateClientRequest, CreateClientResponse,
            CreateInvoiceRequest, CreateInvoiceResponse, CreateOfferRequest,
            CreateOfferResponse, FiatRates, ListChannelsResponse, NodeInfo,
            OpenChannelRequest, OpenChannelResponse, PayInvoiceRequest,
            PayInvoiceResponse, PayOfferRequest, PayOfferResponse,
            PayOnchainRequest, PayOnchainResponse, PaymentAddress,
            PreflightCloseChannelRequest, PreflightCloseChannelResponse,
            PreflightOpenChannelRequest, PreflightOpenChannelResponse,
            PreflightPayInvoiceRequest, PreflightPayInvoiceResponse,
            PreflightPayOfferRequest, PreflightPayOfferResponse,
            PreflightPayOnchainRequest, PreflightPayOnchainResponse,
            UpdateClientRequest, UpdatePaymentNote,
        },
        app_data::AppDataDb,
        settings::SettingsDb,
        types::{
            AppUserInfo, BackupInfo, Config, GDriveSignupCredentials, Invoice,
            LnurlPayRequest, Network, Payment, PaymentCreatedIndex,
            PaymentMethod, RevocableClient, RootSeed, ShortPayment, Username,
        },
    },
    types::GDriveSignupCredentials as GDriveSignupCredentialsRs,
};

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

    pub async fn provision(&self) -> anyhow::Result<()> {
        self.inner.provision().await
    }

    pub async fn restore(
        config: Config,
        google_auth_code: Option<String>,
        root_seed: RootSeed,
    ) -> anyhow::Result<AppHandle> {
        // Ignored in local dev.
        //
        // Single-use `serverAuthCode` from Google OAuth 2 consent flow, used by
        // the enclave to get access+refresh tokens.
        let google_auth_code = match DeployEnv::from(config.deploy_env) {
            DeployEnv::Dev => None,
            DeployEnv::Prod | DeployEnv::Staging => google_auth_code,
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
        root_seed: RootSeed,
        gdrive_signup_creds: Option<GDriveSignupCredentials>,
        signup_code: Option<String>,
        partner: Option<String>,
    ) -> anyhow::Result<AppHandle> {
        let config = AppConfig::from(config);

        // When user signs up with active Google Drive backup, this is the
        // backup password + single-use `serverAuthCode` from Google OAuth 2
        // consent flow, used by the enclave to get access+refresh tokens.
        let gdrive_signup_creds = gdrive_signup_creds.and_then(|c| {
            match config.deploy_env {
                // Ignored in local dev.
                DeployEnv::Dev => None,
                // TODO(phlip9): don't know why frb keeps trying to add a
                // conversion for the Rust-only type...
                DeployEnv::Prod | DeployEnv::Staging =>
                    Some(GDriveSignupCredentialsRs::from(c)),
            }
        });

        let partner = partner
            .map(|p| UserPk::from_hex(&p))
            .transpose()
            .context("Failed to parse partner id")?;

        App::signup(
            &mut SysRng::new(),
            config,
            &root_seed.inner,
            gdrive_signup_creds,
            signup_code,
            partner,
        )
        .await
        .context("Failed to generate and signup new wallet")
        .map(Self::new)
    }

    /// flutter_rust_bridge:sync
    pub fn settings_db(&self) -> SettingsDb {
        SettingsDb::new(self.inner.settings_db())
    }

    /// flutter_rust_bridge:sync
    pub fn app_db(&self) -> AppDataDb {
        AppDataDb::new(self.inner.app_db())
    }

    /// flutter_rust_bridge:sync
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
            .node_client()?
            .node_info()
            .await
            .map(NodeInfo::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(list-channels)")]
    pub async fn list_channels(&self) -> anyhow::Result<ListChannelsResponse> {
        self.inner
            .node_client()?
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
            .node_client()?
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
            .node_client()?
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
            .node_client()?
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
            .node_client()?
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
            .node_client()?
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
            .node_client()?
            .preflight_pay_onchain(req.try_into()?)
            .await
            .map(PreflightPayOnchainResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(get-address)")]
    pub async fn get_address(&self) -> anyhow::Result<String> {
        self.inner
            .node_client()?
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
            .node_client()?
            .create_invoice(req.try_into()?)
            .await
            // TODO(phlip9): return new PaymentCreatedIndex
            .map(CreateInvoiceResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-pay-invoice)")]
    pub async fn preflight_pay_invoice(
        &self,
        req: PreflightPayInvoiceRequest,
    ) -> anyhow::Result<PreflightPayInvoiceResponse> {
        self.inner
            .node_client()?
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
            .node_client()?
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
            .node_client()?
            .create_offer(req.try_into()?)
            .await
            .map(CreateOfferResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(preflight-pay-offer)")]
    pub async fn preflight_pay_offer(
        &self,
        req: PreflightPayOfferRequest,
    ) -> anyhow::Result<PreflightPayOfferResponse> {
        self.inner
            .node_client()?
            .preflight_pay_offer(req.try_into()?)
            .await
            .map(PreflightPayOfferResponse::from)
            .map_err(anyhow::Error::new)
    }

    #[instrument(skip_all, name = "(pay-offer)")]
    pub async fn pay_offer(
        &self,
        req: PayOfferRequest,
    ) -> anyhow::Result<PayOfferResponse> {
        let req = PayOfferRequestRs::try_from(req)?;
        let id = LxPaymentId::OfferSend(req.cid);
        self.inner
            .node_client()?
            .pay_offer(req)
            .await
            .map(|resp| PayOfferResponse::from_id_and_response(id, resp))
            .map_err(anyhow::Error::new)
    }

    /// Delete both the local payment state and the on-disk payment db.
    pub fn delete_payment_db(&self) -> anyhow::Result<()> {
        let mut db_lock = self.inner.payments_db().lock().unwrap();
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

    /// flutter_rust_bridge:sync
    pub fn get_payment_by_created_index(
        &self,
        created_idx: PaymentCreatedIndex,
    ) -> Option<Payment> {
        let created_idx = PaymentCreatedIndexRs::try_from(created_idx).ok()?;
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_payment_by_created_index(&created_idx)
            .map(Payment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_short_payment_by_scroll_index(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPayment> {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_payment_by_scroll_idx(scroll_idx)
            .map(ShortPayment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_pending_short_payment_by_scroll_index(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPayment> {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_pending_payment_by_scroll_idx(scroll_idx)
            .map(ShortPayment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_finalized_short_payment_by_scroll_index(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPayment> {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_finalized_payment_by_scroll_idx(scroll_idx)
            .map(ShortPayment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_pending_not_junk_short_payment_by_scroll_index(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPayment> {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_pending_not_junk_payment_by_scroll_idx(scroll_idx)
            .map(ShortPayment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_finalized_not_junk_short_payment_by_scroll_index(
        &self,
        scroll_idx: usize,
    ) -> Option<ShortPayment> {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock
            .state()
            .get_finalized_not_junk_payment_by_scroll_idx(scroll_idx)
            .map(ShortPayment::from)
    }

    /// flutter_rust_bridge:sync
    pub fn get_num_payments(&self) -> usize {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock.state().num_payments()
    }

    /// flutter_rust_bridge:sync
    pub fn get_num_pending_payments(&self) -> usize {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock.state().num_pending()
    }

    /// flutter_rust_bridge:sync
    pub fn get_num_finalized_payments(&self) -> usize {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock.state().num_finalized()
    }

    /// flutter_rust_bridge:sync
    pub fn get_num_pending_not_junk_payments(&self) -> usize {
        let db_lock = self.inner.payments_db().lock().unwrap();
        db_lock.state().num_pending_not_junk()
    }

    /// flutter_rust_bridge:sync
    pub fn get_num_finalized_not_junk_payments(&self) -> usize {
        let db_lock = self.inner.payments_db().lock().unwrap();
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
            .node_client()?
            .update_payment_note(req.clone())
            .await
            .map(|Empty {}| ())
            .map_err(anyhow::Error::new)?;
        // Update local store after
        self.inner
            .payments_db()
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
            .node_client()?
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
        let resp = self.inner.node_client()?.get_revocable_clients(req).await?;
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
            .node_client()?
            .update_revocable_client(req)
            .await?;
        Ok(())
    }

    #[instrument(skip_all, name = "(list-broadcasted-txs)")]
    pub async fn list_broadcasted_txs(&self) -> anyhow::Result<String> {
        let resp = self
            .inner
            .node_client()?
            .list_broadcasted_txs()
            .await
            .map_err(anyhow::Error::new)?;
        serde_json::to_string_pretty(&resp)
            .context("Failed to serialize broadcasted txs")
    }

    #[instrument(skip_all, name = "(backup-info)")]
    pub async fn backup_info(&self) -> anyhow::Result<BackupInfo> {
        let resp = self.inner.node_client()?.backup_info().await?;
        let backup_info = BackupInfo::from(resp);
        Ok(backup_info)
    }

    /// Resolve a (possible) [`PaymentUri`] string that we just
    /// scanned/pasted into the best [`PaymentMethod`] for us to pay.
    ///
    /// [`PaymentUri`]: payment_uri::PaymentUri
    #[instrument(skip_all, name = "(resolve-best)")]
    pub async fn resolve_best(
        &self,
        network: Network,
        uri_str: String,
    ) -> anyhow::Result<PaymentMethod> {
        let payment_uri = payment_uri::PaymentUri::parse(&uri_str)
            .context("Unrecognized payment code")?;

        payment_uri::resolve_best(
            self.inner.bip353_client(),
            self.inner.lnurl_client(),
            network.into(),
            payment_uri,
        )
        .await
        .map(PaymentMethod::from)
    }

    /// Resolve a [`LnurlPayRequest`] that we just received + the amount in
    /// msats. After resolving, we can use the [`Invoice`] to pay the
    /// invoice.
    pub async fn resolve_lnurl_pay_request(
        &self,
        req: LnurlPayRequest,
        amount_msats: u64,
    ) -> anyhow::Result<Invoice> {
        let pay_req = LnurlPayRequestRs::from(req);

        let lx_invoice = self
            .inner
            .lnurl_client()
            .resolve_pay_request(&pay_req, Amount::from_msat(amount_msats))
            .await?;
        Ok(Invoice::from(lx_invoice))
    }

    /// Get the [`PaymentAddress`] for the user and if it is updatable.
    pub async fn get_payment_address(&self) -> anyhow::Result<PaymentAddress> {
        let resp = self.inner.node_client()?.get_payment_address().await?;
        PaymentAddress::try_from(resp)
    }

    pub async fn update_payment_address(
        &self,
        username: Username,
    ) -> anyhow::Result<PaymentAddress> {
        let req = UsernameStructRs {
            username: UsernameRs::try_from(username)?,
        };
        let resp = self
            .inner
            .node_client()?
            .update_payment_address(req)
            .await?;
        PaymentAddress::try_from(resp)
    }
}
