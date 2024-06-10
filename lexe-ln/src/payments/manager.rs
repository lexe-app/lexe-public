use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{bail, ensure, Context};
use bdk::TransactionDetails;
use common::{
    api::qs::UpdatePaymentNote,
    ln::{
        amount::Amount,
        hashes::LxTxid,
        payments::{
            LxPaymentHash, LxPaymentId, LxPaymentPreimage, PaymentStatus,
        },
    },
    notify,
    shutdown::ShutdownChannel,
    task::LxTask,
    test_event::TestEvent,
};
use lightning::{events::PaymentPurpose, ln::channelmanager::FailureCode};
use rust_decimal::Decimal;
use tokio::sync::Mutex;
use tracing::{debug, debug_span, error, info, instrument};

use crate::{
    esplora::{LexeEsplora, TxConfStatus},
    payments::{
        inbound::{InboundSpontaneousPayment, LxPaymentPurpose},
        onchain::OnchainReceive,
        Payment,
    },
    test_event::TestEventSender,
    traits::{LexeChannelManager, LexeInnerPersister, LexePersister},
    wallet::LexeWallet,
};

/// The interval at which we check our pending payments for expired invoices.
const INVOICE_EXPIRY_CHECK_INTERVAL: Duration = Duration::from_secs(120);
/// The interval at which we check our onchain payments for confirmations.
const ONCHAIN_PAYMENT_CHECK_INTERVAL: Duration = Duration::from_secs(120);

/// Annotates that a given [`Payment`] was returned by a `check_*` method which
/// successfully validated a proposed state transition. [`CheckedPayment`]s
/// should be persisted in order to transform into [`PersistedPayment`]s.
#[must_use]
pub struct CheckedPayment(pub Payment);

/// Annotates that a given [`Payment`] was successfully persisted.
/// [`PersistedPayment`]s should be committed to the local payments state.
#[must_use]
pub struct PersistedPayment(pub Payment);

/// The top-level, cloneable actor which exposes the main entrypoints for
/// various payment actions, including creating, updating, and finalizing
/// payments.
///
/// The primary responsibility of the [`PaymentsManager`] is to manage shared
/// access to the underlying payments state machine, and to coordinate callers,
/// the persister, and LDK to ensure that state updates are in sync, and that
/// there are no update / persist races.
#[derive(Clone)]
pub struct PaymentsManager<CM: LexeChannelManager<PS>, PS: LexePersister> {
    data: Arc<Mutex<PaymentsData>>,
    persister: PS,
    channel_manager: CM,
    test_event_tx: TestEventSender,
}

/// The main payments state machine, exposing private methods available only to
/// the [`PaymentsManager`].
///
/// Each state update consists of three stages:
///
/// 1) Check: We validate the proposed state transition, returning a
///    [`CheckedPayment`] if everything is OK. This is handled by the `check_*`
///    methods, which in turn delegate the heavy lifting to the corresponding
///    `check_*` methods available on each specific payment type.
/// 2) Persist: We persist the validated state transition, returning a
///    [`PersistedPayment`] if persistence succeeded. This is handled by the
///    [`create_payment`] and [`persist_payment`] methods.
/// 3) Commit: We commit the validated + persisted state transition to the local
///    state. This is done by [`PaymentsData::commit`].
///
/// To prevent update and persist races, a (Tokio) lock to the [`PaymentsData`]
/// struct (or at least the [`LxPaymentId`] of the payment) should be held
/// throughout the entirety of the state update, including the all of the check,
/// persist, and commit stages. TODO(max): If this turns out to be a performance
/// bottleneck, we should switch to per-payment or per-payment-type locks.
///
/// [`create_payment`]: crate::traits::LexeInnerPersister::create_payment
/// [`persist_payment`]: crate::traits::LexeInnerPersister::persist_payment
struct PaymentsData {
    pending: HashMap<LxPaymentId, Payment>,
    finalized: HashSet<LxPaymentId>,
}

impl<CM: LexeChannelManager<PS>, PS: LexePersister> PaymentsManager<CM, PS> {
    /// Instantiates a new [`PaymentsManager`] and spawns associated tasks.
    pub fn new(
        persister: PS,
        channel_manager: CM,
        esplora: Arc<LexeEsplora>,
        pending_payments: Vec<Payment>,
        finalized_payment_ids: Vec<LxPaymentId>,
        wallet: LexeWallet,
        onchain_recv_rx: notify::Receiver,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> (Self, [LxTask<()>; 3]) {
        let pending = pending_payments
            .into_iter()
            // Check that payments are indeed pending before adding to hashmap
            .filter_map(|payment| {
                let id = payment.id();
                let status = payment.status();

                if matches!(status, PaymentStatus::Pending) {
                    Some((id, payment))
                } else if cfg!(debug_assertions) {
                    panic!("Payment {id} should've been pending, was {status}");
                } else {
                    error!("Payment {id} should've been pending, was {status}");
                    None
                }
            })
            .collect::<HashMap<LxPaymentId, Payment>>();
        let finalized = finalized_payment_ids.into_iter().collect();

        let data = Arc::new(Mutex::new(PaymentsData { pending, finalized }));

        let myself = Self {
            data,
            persister,
            channel_manager,
            test_event_tx,
        };

        let payments_tasks = [
            myself.spawn_invoice_expiry_checker(shutdown.clone()),
            myself.spawn_onchain_confs_checker(esplora, shutdown.clone()),
            myself.spawn_onchain_recv_checker(
                wallet,
                onchain_recv_rx,
                shutdown,
            ),
        ];

        (myself, payments_tasks)
    }

    fn spawn_invoice_expiry_checker(
        &self,
        mut shutdown: ShutdownChannel,
    ) -> LxTask<()> {
        let payments_manager = self.clone();
        LxTask::spawn_named_with_span(
            "invoice expiry checker",
            debug_span!("(invoice-expiry-checker)"),
            async move {
                let mut check_timer =
                    tokio::time::interval(INVOICE_EXPIRY_CHECK_INTERVAL);

                loop {
                    tokio::select! {
                        _ = check_timer.tick() => {
                            if let Err(e) = payments_manager
                                .check_invoice_expiries()
                                .await {
                                error!("Error checking invoice expiries: {e:#}");
                            }
                        }
                        () = shutdown.recv() => break,
                    }
                }

                info!("Invoice expiry checker task shutting down");
            },
        )
    }

    fn spawn_onchain_confs_checker(
        &self,
        esplora: Arc<LexeEsplora>,
        mut shutdown: ShutdownChannel,
    ) -> LxTask<()> {
        let payments_manager = self.clone();

        LxTask::spawn_named_with_span(
            "onchain confs checker",
            debug_span!("(onchain-confs-checker)"),
            async move {
                let mut check_timer =
                    tokio::time::interval(ONCHAIN_PAYMENT_CHECK_INTERVAL);
                loop {
                    tokio::select! {
                        _ = check_timer.tick() => {
                            if let Err(e) = payments_manager
                                .check_onchain_confs(esplora.as_ref())
                                .await {
                                error!("Error checking onchain confs: {e:#}");
                            }
                        }
                        () = shutdown.recv() => break,
                    }
                }

                info!("Onchain confs checker task shutting down");
            },
        )
    }

    /// Spawns a task that calls `check_onchain_receives` when notified.
    ///
    /// The BDK sync task holds the `onchain_recv_tx` and sends a notification
    /// every time BDK sync completes.
    fn spawn_onchain_recv_checker(
        &self,
        wallet: LexeWallet,
        mut onchain_recv_rx: notify::Receiver,
        mut shutdown: ShutdownChannel,
    ) -> LxTask<()> {
        let payments_manager = self.clone();
        LxTask::spawn_named_with_span(
            "onchain receive checker",
            debug_span!("(onchain-recv-checker)"),
            async move {
                loop {
                    tokio::select! {
                        () = onchain_recv_rx.recv() => {
                            if let Err(e) = payments_manager
                                .check_onchain_receives(&wallet)
                                .await {
                                error!("Error checking onchain recvs: {e:#}");
                            }
                        }
                        () = shutdown.recv() => break,
                    }
                }

                info!("Onchain receive checker task shutting down");
            },
        )
    }

    /// Register a new, globally-unique payment.
    /// Errors if the payment already exists.
    #[instrument(skip_all, name = "(new-payment)")]
    pub async fn new_payment(&self, payment: Payment) -> anyhow::Result<()> {
        let id = payment.id();
        info!(%id, "Registering new payment");
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_new_payment(payment)
            .context("Error handling new payment")?;

        let persisted = self
            .persister
            .create_payment(checked)
            .await
            .context("Could not persist new payment")?;

        locked_data.commit(persisted);

        Ok(())
    }

    /// Returns true if we already have a payment with the given [`LxPaymentId`]
    /// registered.
    pub async fn contains_payment_id(&self, id: &LxPaymentId) -> bool {
        self.data.lock().await.contains_payment_id(id)
    }

    /// Attempt to update the personal note on a payment.
    #[instrument(skip_all, name = "(update-payment-note)")]
    pub async fn update_payment_note(
        &self,
        update: UpdatePaymentNote,
    ) -> anyhow::Result<()> {
        let id = update.index.id;
        info!(%id, "Updating payment note");
        let mut locked_data = self.data.lock().await;

        // If the payment was pending, get a clone of our local copy.
        // If the payment was finalized, we have to fetch a copy from the DB.
        let mut payment_clone = match locked_data.pending.get(&id) {
            Some(pending) => pending.clone(),
            None => {
                // Before fetching, quickly check that the payment exists.
                ensure!(
                    locked_data.finalized.contains(&id),
                    "Payment to be updated does not exist",
                );

                self.persister
                    .get_payment(update.index)
                    .await
                    .context("Could not fetch finalized payment")?
                    .context("Finalized payment was not found in DB")?
            }
        };

        // Update
        payment_clone.set_note(update.note);

        // Persist
        let persisted = self
            .persister
            .persist_payment(CheckedPayment(payment_clone))
            .await
            .context("Could not persist updated payment")?;

        // Commit
        locked_data.commit(persisted);

        debug!(%id, "Successfully updated payment note");
        Ok(())
    }

    /// Handles a [`PaymentClaimable`] event.
    ///
    /// [`PaymentClaimable`]: lightning::events::Event::PaymentClaimable
    #[instrument(skip_all, name = "(payment-claimable)")]
    pub async fn payment_claimable(
        &self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let amount = Amount::from_msat(amt_msat);
        info!(%amount, %hash, "Handling PaymentClaimable");
        let purpose = LxPaymentPurpose::try_from(purpose)
            // The conversion can only fail if the preimage is unknown.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    FailureCode::IncorrectOrUnknownPaymentDetails,
                )
            })?;

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_claimable(hash, amount, purpose)
            // If validation failed, permanently fail the HTLC.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    FailureCode::IncorrectOrUnknownPaymentDetails,
                )
            })
            .context("Error validating PaymentClaimable")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            // If persistence failed, fail the HTLC with a temporary error so
            // that the sender can retry at a loter point in time.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    FailureCode::TemporaryNodeFailure,
                )
            })
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        // Everything ok; claim the payment
        // TODO(max): `claim_funds` docs state that we must check that the
        // amount we received matches our expectation, relevant if
        // we're receiving payment for e.g. an order of some sort.
        // Otherwise, we will have given the sender a proof-of-payment
        // when they did not fulfill the full expected payment.
        // Implement this once it becomes relevant.
        self.channel_manager.claim_funds(purpose.preimage().into());

        // Q: What about if we handle a `PaymentClaimable` event, call
        // claim_funds, handle a `PaymentClaimed` event, then crash before the
        // channel manager is persisted? Wouldn't that mean that when we replay
        // the `PaymentClaimable` event upon restart, that the state transition
        // would be rejected because the `Payment` is persisted as already
        // `Completed`, when we actually need to call `claim_funds` again?
        //
        // A: `PaymentClaimable` will never appear in the same
        // `ChannelManager::pending_events` batch as the `PaymentClaimed` event,
        // since `claim_funds` generates `MessageSendEvent`s which the
        // `PeerManager` needs to handle before the payment is actually claimed
        // (source: claim_funds docs). After the event handler (which is what
        // calls this function) returns, the channel manager gets repersisted
        // (in the BGP). Thus, if a persisted `Payment` is already `Completed`,
        // then it must be true that the persisted channel manager is aware that
        // we have already called `claim_funds`, and thus it does not need to be
        // called again.

        info!("Handled PaymentClaimable");
        self.test_event_tx.send(TestEvent::PaymentClaimable);
        Ok(())
    }

    /// Handles a [`PaymentClaimed`] event.
    ///
    /// [`PaymentClaimed`]: lightning::events::Event::PaymentClaimed
    #[instrument(skip_all, name = "(payment-claimed)")]
    pub async fn payment_claimed(
        &self,
        hash: LxPaymentHash,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let amount = Amount::from_msat(amt_msat);
        info!(%amount, %hash, "Handling PaymentClaimed");
        let purpose = LxPaymentPurpose::try_from(purpose)?;

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_claimed(hash, amount, purpose)
            .context("Error validating PaymentClaimed")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        info!("Handled PaymentClaimed");
        self.test_event_tx.send(TestEvent::PaymentClaimed);
        Ok(())
    }

    /// Handles a [`PaymentSent`] event.
    ///
    /// [`PaymentSent`]: lightning::events::Event::PaymentSent
    #[instrument(skip_all, name = "(payment-sent)")]
    pub async fn payment_sent(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid_msat: Option<u64>,
    ) -> anyhow::Result<()> {
        let maybe_fees_paid = maybe_fees_paid_msat.map(Amount::from_msat);
        info!(%hash, ?maybe_fees_paid, "Handling PaymentSent");

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_sent(hash, preimage, maybe_fees_paid)
            .context("Error validating PaymentSent")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        info!("Handled PaymentSent");
        self.test_event_tx.send(TestEvent::PaymentSent);
        Ok(())
    }

    /// Registers that an outbound Lightning payment has failed. Should be
    /// called in response to a [`PaymentFailed`] event, or if the initial send
    /// in [`pay_invoice`] failed outright, resulting in no pending payments
    /// being registered with LDK (which means that no [`PaymentFailed`] or
    /// [`PaymentSent`] events will be emitted by LDK later).
    ///
    /// [`pay_invoice`]: crate::command::pay_invoice
    /// [`PaymentSent`]: lightning::events::Event::PaymentSent
    /// [`PaymentFailed`]: lightning::events::Event::PaymentFailed
    #[instrument(skip_all, name = "(payment-failed)")]
    pub async fn payment_failed(
        &self,
        hash: LxPaymentHash,
    ) -> anyhow::Result<()> {
        info!(%hash, "Handling PaymentFailed");

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_failed(hash)
            .context("Error validating PaymentFailed")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        info!("Handled PaymentFailed");
        Ok(())
    }

    /// Times out any pending inbound or outbound invoice payments whose
    /// invoices have expired. This function should be called regularly.
    #[instrument(skip_all, name = "(check-invoice-expiries)")]
    pub async fn check_invoice_expiries(&self) -> anyhow::Result<()> {
        debug!("Checking invoice expiries");

        // Call SystemTime::now() just once then pass it in everywhere else.
        let unix_duration = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time is before UNIX timestamp");

        // Check
        let mut locked_data = self.data.lock().await;
        let (all_checked, oip_hashes) = locked_data
            .check_invoice_expiries(unix_duration)
            .context("Error checking invoice expiries")?;

        // Abandon all newly expired outbound invoice payments.
        for oip_hash in oip_hashes {
            self.channel_manager.abandon_payment(oip_hash.into());
        }

        // Persist
        let all_persisted = self
            .persister
            .persist_payment_batch(all_checked)
            .await
            .context("Couldn't persist payment batch")?;

        // Commit
        for persisted in all_persisted {
            locked_data.commit(persisted);
        }

        debug!("Successfully checked invoice expiries");
        Ok(())
    }

    /// Register the successful broadcast of an onchain send tx.
    #[instrument(skip_all, name = "(onchain-send-broadcasted)")]
    pub async fn onchain_send_broadcasted(
        &self,
        id: &LxPaymentId,
        txid: &LxTxid,
    ) -> anyhow::Result<()> {
        debug!(%id, "Registering that an onchain send has been broadcasted");
        let mut locked_data = self.data.lock().await;

        ensure!(
            !locked_data.finalized.contains(id),
            "Onchain send was already finalized",
        );

        let pending = locked_data
            .pending
            .get(id)
            .context("Payment doesn't exist")?;

        // Check
        let checked = match pending {
            Payment::OnchainSend(os) => os
                .broadcasted(txid)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Invalid state transition")?,
            _ => bail!("Payment was not an onchain send"),
        };

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Persist failed")?;

        // Commit
        locked_data.commit(persisted);

        debug!("Successfully registered successful broadcast");
        Ok(())
    }

    /// Checks the confirmation status of our onchain payments.
    /// This function should be called regularly.
    #[instrument(skip_all, name = "(check-onchain-confs)")]
    pub async fn check_onchain_confs(
        &self,
        // TODO(max): Since these checks aren't security critical, and since
        // there may be lots of API calls, this esplora client should point to
        // Lexe's 'internal' instance.
        esplora: &LexeEsplora,
    ) -> anyhow::Result<()> {
        debug!("Checking onchain confs");

        // We drop the lock here so that off-chain payments can make progress
        // while we make multiple Esplora API calls. It's okay if a new onchain
        // tx is added before the lock is reacquired because the onchain confs
        // checker will update the new tx the next time its timer ticks.
        let payment_ids_pending_queries = {
            let locked_data = self.data.lock().await;

            // Construct a `(LxPaymentId, TxConfQuery)` for every pending
            // onchain payment.
            locked_data
                .pending
                .values()
                .filter_map(|p| match p {
                    Payment::OnchainSend(os) =>
                        Some((os.id(), os.to_tx_conf_query())),
                    Payment::OnchainReceive(or) =>
                        Some((or.id(), or.to_tx_conf_query())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };

        // Determine the conf statuses of all our pending payments.
        let pending_queries =
            payment_ids_pending_queries.iter().map(|(_, q)| q);
        let tx_conf_statuses = esplora
            .get_tx_conf_statuses(pending_queries)
            .await
            .context("Error while computing conf statuses")?;

        // Check
        let ids = payment_ids_pending_queries.iter().map(|(id, _)| id);
        let mut locked_data = self.data.lock().await;
        let all_checked = locked_data
            .check_onchain_confs(ids, tx_conf_statuses)
            .context("Invalid tx conf state transition")?;

        // Persist
        let all_persisted = self
            .persister
            .persist_payment_batch(all_checked)
            .await
            .context("Couldn't persist payment batch")?;

        // Commit
        for persisted in all_persisted {
            locked_data.commit(persisted);
        }

        debug!("Successfully checked onchain confs");
        Ok(())
    }

    /// Queries the [`bdk::Wallet`] to see if there are any onchain receives
    /// that the [`PaymentsManager`] doesn't yet know about. If so, the
    /// [`OnchainReceive`] is constructed and registered with the
    /// [`PaymentsManager`].
    ///
    /// This function should be called regularly.
    #[instrument(skip_all, name = "(check-onchain-receives)")]
    pub async fn check_onchain_receives(
        &self,
        wallet: &LexeWallet,
    ) -> anyhow::Result<()> {
        debug!("Checking for onchain receives");

        let include_raw = false;
        let txs = wallet.list_transactions(include_raw).await?;

        // Drop the lock here to prevent deadlocking when we call `new_payment`.
        let unseen_txs = {
            let locked_data = self.data.lock().await;
            txs.into_iter()
                // The tx is inbound if we own none of the inputs.
                .filter(|tx_details| tx_details.sent == 0)
                // Filter out txs we already know about
                .filter(|tx_details| {
                    let id = LxPaymentId::OnchainRecv(LxTxid(tx_details.txid));
                    !locked_data.pending.contains_key(&id)
                        && !locked_data.finalized.contains(&id)
                })
                .collect::<Vec<TransactionDetails>>()
        };

        let onchain_recv_futs = unseen_txs
            .iter()
            // Map each to a future which constructs the `OnchainReceive`
            .map(|tx_details| async {
                let include_raw = true;
                let raw_tx = wallet
                    .get_tx(&tx_details.txid, include_raw)
                    .await?
                    .context("Missing tx details")?
                    .transaction
                    .context("Raw tx was not included")?;

                let amount_sats = Decimal::from(tx_details.received);
                let amount = Amount::try_from_satoshis(amount_sats)
                    .context("Overflowed")?;

                let or = OnchainReceive::new(raw_tx, amount);

                Ok::<_, anyhow::Error>(or)
            });

        let onchain_recvs = futures::future::try_join_all(onchain_recv_futs)
            .await
            .context("Error constructing new `OnchainReceive`s")?;

        let register_futs = onchain_recvs
            .into_iter()
            .map(|or| self.new_payment(or.into()));

        for res in futures::future::join_all(register_futs).await {
            res.context("Failed to register new onchain receive")?;
        }

        debug!("Successfully checked for and registered new onchain receives");
        Ok(())
    }
}

impl PaymentsData {
    /// Commits a [`PersistedPayment`] to the local state.
    fn commit(&mut self, persisted: PersistedPayment) {
        let payment = persisted.0;
        let id = payment.id();

        if cfg!(debug_assertions) {
            payment.assert_invariants();
        }

        match payment.status() {
            PaymentStatus::Pending => {
                self.pending.insert(id, payment);
            }
            PaymentStatus::Completed | PaymentStatus::Failed => {
                self.pending.remove(&id);
                self.finalized.insert(id);
            }
        }
    }

    fn contains_payment_id(&self, id: &LxPaymentId) -> bool {
        self.pending.contains_key(id) || self.finalized.contains(id)
    }

    fn check_new_payment(
        &self,
        payment: Payment,
    ) -> anyhow::Result<CheckedPayment> {
        // Check that this payment is indeed unique.
        let id = payment.id();
        ensure!(
            !self.pending.contains_key(&id),
            "Payment already exists: pending"
        );
        ensure!(
            !self.finalized.contains(&id),
            "Payment already exists: finalized"
        );

        // Newly created payments should *always* be pending.
        debug_assert!(matches!(payment.status(), PaymentStatus::Pending));

        // Everything ok.
        Ok(CheckedPayment(payment))
    }

    fn check_payment_claimable(
        &self,
        hash: LxPaymentHash,
        amount: Amount,
        purpose: LxPaymentPurpose,
    ) -> anyhow::Result<CheckedPayment> {
        let id = LxPaymentId::from(hash);

        // The PaymentClaimable docs have a note that LDK will not stop an
        // inbound payment from being paid multiple times. We should fail the
        // payment in this case because:
        // - This messes up (or significantly complicates) our accounting
        // - This likely reflects an error on the receiver's part (reusing the
        //   same invoice for multiple payments, which would allow any nodes
        //   along the first payment path to steal subsequent payments)
        // - We should not allow payments to go through, in order to teach users
        //   that this is not an acceptable way to use lightning, because it is
        //   not safe. It is not hard to imagine users developing the
        //   misconception that it is safe to reuse invoices if duplicate
        //   payments actually do succeed.
        //
        // TODO(max): If LDK implements the regeneration of PaymentClaimable
        // events upon restart, we'll need a way to differentiate between these
        // regenerated events and duplicate payments to the same invoice.
        // https://discord.com/channels/915026692102316113/978829624635195422/1085427966986690570
        ensure!(
            !self.finalized.contains(&id),
            "Payment was a duplicate, or was already finalized"
        );

        let maybe_pending_payment = self.pending.get(&id);

        let checked = match (maybe_pending_payment, purpose) {
            (Some(pending_payment), purpose) => {
                // Pending payment exists; update it
                pending_payment
                    .check_payment_claimable(hash, amount, purpose)?
            }
            (None, LxPaymentPurpose::Spontaneous { preimage }) => {
                // We just got a new spontaneous payment!
                // Create the new payment.
                let isp =
                    InboundSpontaneousPayment::new(hash, preimage, amount);
                let payment = Payment::from(isp);

                // Validate the new payment.
                self.check_new_payment(payment)
                    .context("Error creating new spontaneous payment")?
            }
            (None, LxPaymentPurpose::Invoice { .. }) => {
                bail!("Tried to claim non-existent invoice payment")
            }
        };

        Ok(checked)
    }

    fn check_payment_claimed(
        &self,
        hash: LxPaymentHash,
        amount: Amount,
        purpose: LxPaymentPurpose,
    ) -> anyhow::Result<CheckedPayment> {
        let id = LxPaymentId::from(hash);

        ensure!(
            !self.finalized.contains(&id),
            "Payment was already finalized"
        );

        let pending_payment = self
            .pending
            .get(&id)
            .context("Pending payment does not exist")?;

        let checked = match (pending_payment, purpose) {
            (
                Payment::InboundInvoice(iip),
                LxPaymentPurpose::Invoice { preimage, secret },
            ) => iip
                .check_payment_claimed(hash, secret, preimage, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing inbound invoice payment")?,
            (
                Payment::InboundSpontaneous(isp),
                LxPaymentPurpose::Spontaneous { preimage },
            ) => isp
                .check_payment_claimed(hash, preimage, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing inbound spontaneous payment")?,
            _ => bail!("Not an inbound LN payment, or purpose didn't match"),
        };

        Ok(checked)
    }

    fn check_payment_sent(
        &self,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid: Option<Amount>,
    ) -> anyhow::Result<CheckedPayment> {
        let id = LxPaymentId::from(hash);

        ensure!(
            !self.finalized.contains(&id),
            "Payment was already finalized"
        );

        let pending_payment = self
            .pending
            .get(&id)
            .context("Pending payment does not exist")?;

        let checked = match pending_payment {
            Payment::OutboundInvoice(oip) => oip
                .check_payment_sent(hash, preimage, maybe_fees_paid)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error checking outbound invoice payment")?,
            Payment::OutboundSpontaneous(_) => todo!(),
            _ => bail!("Not an outbound Lightning payment"),
        };

        Ok(checked)
    }

    fn check_payment_failed(
        &self,
        hash: LxPaymentHash,
    ) -> anyhow::Result<CheckedPayment> {
        let id = LxPaymentId::from(hash);

        ensure!(
            !self.finalized.contains(&id),
            "Payment was already finalized"
        );

        let pending_payment = self
            .pending
            .get(&id)
            .context("Pending payment does not exist")?;

        let checked = match pending_payment {
            Payment::OutboundInvoice(oip) => oip
                .check_payment_failed(hash)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error checking outbound invoice payment")?,
            Payment::OutboundSpontaneous(_) => todo!(),
            _ => bail!("Not an outbound Lightning payment"),
        };

        Ok(checked)
    }

    /// Returns all expired invoice payments`*`, as well as the hashes of all
    /// outbound invoice payments which should be passed to [`abandon_payment`].
    ///
    /// `*` We don't return already-abandoning outbound invoice payments, since
    /// the work (persistence + [`abandon_payment`]) has already been done.
    ///
    /// [`abandon_payment`]: lightning::ln::channelmanager::ChannelManager::abandon_payment
    fn check_invoice_expiries(
        &self,
        // The current time expressed as a Duration since the unix epoch.
        unix_duration: Duration,
    ) -> anyhow::Result<(Vec<CheckedPayment>, Vec<LxPaymentHash>)> {
        let mut oip_hashes = Vec::new();
        let all_expired = self
            .pending
            .values()
            .filter_map(|payment| match payment {
                Payment::InboundInvoice(iip) => iip
                    .check_invoice_expiry(unix_duration)
                    .map(Payment::from)
                    .map(CheckedPayment),
                Payment::OutboundInvoice(oip) => oip
                    .check_invoice_expiry(unix_duration)
                    .inspect(|oip| oip_hashes.push(oip.hash))
                    .map(Payment::from)
                    .map(CheckedPayment),
                _ => None,
            })
            .collect::<Vec<_>>();

        Ok((all_expired, oip_hashes))
    }

    fn check_onchain_confs<'id>(
        &self,
        ids: impl Iterator<Item = &'id LxPaymentId>,
        conf_statuses: Vec<TxConfStatus>,
    ) -> anyhow::Result<Vec<CheckedPayment>> {
        ids.zip(conf_statuses)
            // Fetch the pending onchain payment by its id and call
            // `check_onchain_conf()` on it to validate the state transition.
            .map(|(id, conf_status)| {
                let payment = self
                    .pending
                    .get(id)
                    .context("Received conf status but payment was missing")?;
                let maybe_checked = match payment {
                    Payment::OnchainSend(os) => os
                        .check_onchain_conf(conf_status)
                        .map(|opt| opt.map(Payment::from).map(CheckedPayment))
                        .context("Error checking onchain send conf")?,
                    Payment::OnchainReceive(or) => or
                        .check_onchain_conf(conf_status)
                        .map(|opt| opt.map(Payment::from).map(CheckedPayment))
                        .context("Error checking onchain receive conf")?,
                    _ => bail!("Wasn't an onchain payment"),
                };
                Ok(maybe_checked)
            })
            // Filter `anyhow::Result<Option<T>>` to `anyhow::Result<T>`
            .filter_map(|maybe_checked| match maybe_checked {
                Ok(Some(checked)) => Some(Ok(checked)),
                Ok(None) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<anyhow::Result<Vec<CheckedPayment>>>()
            .context("Error while checking onchain confs in PaymentsData")
    }
}
