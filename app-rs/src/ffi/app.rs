use anyhow::Context;
use common::{
    api::{
        command::{
            PayInvoiceRequest as PayInvoiceRequestRs,
            PayOnchainRequest as PayOnchainRequestRs,
        },
        def::{AppGatewayApi, AppNodeRunApi},
        qs::UpdatePaymentNote as UpdatePaymentNoteRs,
        Empty,
    },
    ln::payments::PaymentIndex as PaymentIndexRs,
    rng::SysRng,
};
use flutter_rust_bridge::{frb, RustOpaqueNom};

pub(crate) use crate::app::App;
use crate::ffi::{
    api::{
        CreateInvoiceRequest, CreateInvoiceResponse, FiatRates, NodeInfo,
        PayInvoiceRequest, PayInvoiceResponse, PayOnchainRequest,
        PayOnchainResponse, PreflightPayInvoiceRequest,
        PreflightPayInvoiceResponse, PreflightPayOnchainRequest,
        PreflightPayOnchainResponse, UpdatePaymentNote,
    },
    types::{
        Config, Payment, PaymentIndex, ShortPayment, ShortPaymentAndIndex,
    },
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

    pub async fn restore(
        config: Config,
        seed_phrase: String,
    ) -> anyhow::Result<AppHandle> {
        App::restore(config.into(), seed_phrase)
            .await
            .context("Failed to restore from seed phrase")
            .map(Self::new)
    }

    pub async fn signup(
        config: Config,
        google_auth_code: String,
        password: String,
    ) -> anyhow::Result<AppHandle> {
        App::signup(
            &mut SysRng::new(),
            config.into(),
            google_auth_code,
            password,
        )
        .await
        .context("Failed to generate and signup new wallet")
        .map(Self::new)
    }

    pub async fn node_info(&self) -> anyhow::Result<NodeInfo> {
        self.inner
            .node_client()
            .node_info()
            .await
            .map(NodeInfo::from)
            .map_err(anyhow::Error::new)
    }

    pub async fn fiat_rates(&self) -> anyhow::Result<FiatRates> {
        self.inner
            .gateway_client()
            .get_fiat_rates()
            .await
            .map(FiatRates::from)
            .map_err(anyhow::Error::new)
    }

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

    pub async fn get_address(&self) -> anyhow::Result<String> {
        self.inner
            .node_client()
            .get_address()
            .await
            .map(|addr| addr.to_string())
            .map_err(anyhow::Error::new)
    }

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
}
