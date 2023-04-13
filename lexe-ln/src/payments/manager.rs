use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, ensure, Context};
use common::ln::payments::{
    LxPaymentHash, LxPaymentId, LxPaymentPreimage, PaymentStatus,
};
use common::shutdown::ShutdownChannel;
use common::task::LxTask;
use lightning::ln::channelmanager::FailureCode;
use lightning::util::events::PaymentPurpose;
use tokio::sync::Mutex;
use tracing::{debug, debug_span, error, info, instrument};

use crate::payments::inbound::{InboundSpontaneousPayment, LxPaymentPurpose};
use crate::payments::Payment;
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};

/// The interval at which we check our pending payments for expired invoices.
const INVOICE_EXPIRY_CHECK_INTERVAL: Duration = Duration::from_secs(120);

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
    /// Instantiates a new [`PaymentsManager`] and spawns a task that
    /// periodically checks for expired invoices.
    pub fn new(
        persister: PS,
        channel_manager: CM,
        pending_payments: Vec<Payment>,
        finalized_payment_ids: Vec<LxPaymentId>,
        test_event_tx: TestEventSender,
        shutdown: ShutdownChannel,
    ) -> (Self, LxTask<()>) {
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

        let invoice_expiry_checker_task =
            myself.spawn_invoice_expiry_checker(shutdown);

        (myself, invoice_expiry_checker_task)
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

    /// Register a new, globally-unique payment.
    /// Errors if the payment already exists.
    pub async fn new_payment(
        &self,
        payment: impl Into<Payment>,
    ) -> anyhow::Result<()> {
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_new_payment(payment.into())
            .context("Error handling new payment")?;

        let persisted = self
            .persister
            .create_payment(checked)
            .await
            .context("Could not persist new payment")?;

        locked_data.commit(persisted);

        Ok(())
    }

    /// Handles a [`PaymentClaimable`] event.
    ///
    /// [`PaymentClaimable`]: lightning::util::events::Event::PaymentClaimable
    #[instrument(skip_all, name = "(payment-claimable)")]
    pub async fn payment_claimable(
        &self,
        hash: impl Into<LxPaymentHash>,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let hash = hash.into();
        info!(%amt_msat, %hash, "Handling PaymentClaimable");
        let purpose = LxPaymentPurpose::try_from(purpose)
            // The conversion can only fail if the preimage is unknown.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    &FailureCode::IncorrectOrUnknownPaymentDetails,
                )
            })?;

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_claimable(hash, amt_msat, purpose)
            // If validation failed, permanently fail the HTLC.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    &FailureCode::IncorrectOrUnknownPaymentDetails,
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
                    &FailureCode::TemporaryNodeFailure,
                )
            })
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        // Everything ok; claim the payment
        // TODO(max): `claim_funds` docs state that we must check that the
        // amt_msat we received matches our expectation, relevant if
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
    /// [`PaymentClaimed`]: lightning::util::events::Event::PaymentClaimed
    #[instrument(skip_all, name = "(payment-claimed)")]
    pub async fn payment_claimed(
        &self,
        hash: impl Into<LxPaymentHash>,
        amt_msat: u64,
        purpose: PaymentPurpose,
    ) -> anyhow::Result<()> {
        let hash = hash.into();
        info!(%amt_msat, %hash, "Handling PaymentClaimed");
        let purpose = LxPaymentPurpose::try_from(purpose)?;

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_claimed(hash, amt_msat, purpose)
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
    /// [`PaymentSent`]: lightning::util::events::Event::PaymentSent
    #[instrument(skip_all, name = "(payment-sent)")]
    pub async fn payment_sent(
        &self,
        hash: impl Into<LxPaymentHash>,
        preimage: impl Into<LxPaymentPreimage>,
        maybe_fees_paid_msat: Option<u64>,
    ) -> anyhow::Result<()> {
        let hash = hash.into();
        info!(%hash, ?maybe_fees_paid_msat, "Handling PaymentSent");

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_sent(hash, preimage.into(), maybe_fees_paid_msat)
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
    /// [`PaymentSent`] events will not be emitted by LDK later).
    ///
    /// [`pay_invoice`]: crate::command::pay_invoice
    /// [`PaymentSent`]: lightning::util::events::Event::PaymentSent
    /// [`PaymentFailed`]: lightning::util::events::Event::PaymentFailed
    #[instrument(skip_all, name = "(payment-failed)")]
    pub async fn payment_failed(
        &self,
        hash: impl Into<LxPaymentHash>,
    ) -> anyhow::Result<()> {
        let hash = hash.into();
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
        let all_checked = locked_data
            .check_invoice_expiries(unix_duration)
            .context("Error checking invoice expiries")?;

        // Persist
        // TODO(max): We could implement a batch persist endpoint for this, but
        // is it really worth it just for invoice expiries?
        let persist_futs = all_checked
            .into_iter()
            .map(|checked| self.persister.persist_payment(checked))
            .collect::<Vec<_>>();
        let all_persisted = futures::future::join_all(persist_futs)
            .await
            .into_iter()
            .collect::<anyhow::Result<Vec<PersistedPayment>>>()
            .context("Failed to persist timed out payments")?;

        // Commit
        for persisted in all_persisted {
            locked_data.commit(persisted);
        }

        debug!("Successfully checked invoice expiries");
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
        amt_msat: u64,
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
                    .check_payment_claimable(hash, amt_msat, purpose)?
            }
            (None, LxPaymentPurpose::Spontaneous { preimage }) => {
                // We just got a new spontaneous payment!
                // Create the new payment.
                let isp =
                    InboundSpontaneousPayment::new(hash, preimage, amt_msat);
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
        amt_msat: u64,
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
                .check_payment_claimed(hash, secret, preimage, amt_msat)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing inbound invoice payment")?,
            (
                Payment::InboundSpontaneous(isp),
                LxPaymentPurpose::Spontaneous { preimage },
            ) => isp
                .check_payment_claimed(hash, preimage, amt_msat)
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
        maybe_fees_paid_msat: Option<u64>,
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
                .check_payment_sent(hash, preimage, maybe_fees_paid_msat)
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

    fn check_invoice_expiries(
        &self,
        // The current time expressed as a Duration since the unix epoch.
        unix_duration: Duration,
    ) -> anyhow::Result<Vec<CheckedPayment>> {
        let mut all_inbound = Vec::new();
        let mut all_outbound = Vec::new();

        for payment in self.pending.values() {
            match payment {
                Payment::InboundInvoice(iip) => all_inbound.push(iip),
                Payment::OutboundInvoice(oip) => all_outbound.push(oip),
                _ => (),
            }
        }

        let expired_inbound = all_inbound
            .into_iter()
            .filter_map(|iip| iip.check_invoice_expiry(unix_duration))
            .map(Payment::from)
            .map(CheckedPayment);
        let expired_outbound = all_outbound
            .into_iter()
            .filter_map(|oip| oip.check_invoice_expiry(unix_duration))
            .map(Payment::from)
            .map(CheckedPayment);

        let all_expired =
            expired_inbound.chain(expired_outbound).collect::<Vec<_>>();

        Ok(all_expired)
    }
}
