use std::{borrow::Cow, collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context, anyhow, bail, ensure};
use bdk_wallet::KeychainKind;
use common::{
    api::test_event::TestEvent,
    ln::{amount::Amount, hashes::LxTxid},
    time::TimestampMs,
};
use lexe_api::{
    models::command::UpdatePaymentNote,
    types::payments::{
        LnClaimId, LxPaymentHash, LxPaymentId, LxPaymentPreimage, PaymentStatus,
    },
};
use lexe_tokio::{notify, notify_once::NotifyOnce, task::LxTask};
#[cfg(doc)]
use lightning::events::Event::PaymentFailed;
use lightning::{events::PaymentPurpose, ln::channelmanager::FailureCode};
use tokio::{
    sync::{Mutex, MutexGuard},
    time::Instant,
};
use tracing::{debug, error, info, info_span, instrument, warn};

use super::{inbound::InboundOfferReusablePayment, outbound::ExpireError};
use crate::{
    esplora::{LexeEsplora, TxConfStatus},
    payments::{
        Payment,
        inbound::{ClaimableError, InboundSpontaneousPayment, LnClaimCtx},
        onchain::OnchainReceive,
        outbound::LxOutboundPaymentFailure,
    },
    test_event::TestEventSender,
    traits::{LexeChannelManager, LexeInnerPersister, LexePersister},
    wallet::LexeWallet,
};

/// The interval at which we check our pending payments for expired
/// invoices/offers.
const PAYMENT_EXPIRY_CHECK_INTERVAL: Duration = Duration::from_secs(120);
/// The interval at which we check our onchain payments for confirmations.
const ONCHAIN_PAYMENT_CHECK_INTERVAL: Duration = Duration::from_secs(120);
const PAYMENT_EXPIRY_CHECK_DELAY: Duration = Duration::from_secs(1);
const ONCHAIN_PAYMENT_CHECK_DELAY: Duration = Duration::from_secs(2);

/// Annotates that a given [`Payment`] was returned by a `check_*` method which
/// successfully validated a proposed state transition. [`CheckedPayment`]s
/// should be persisted in order to transform into [`PersistedPayment`]s.
#[must_use]
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
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
#[cfg_attr(test, derive(Clone, Debug))]
struct PaymentsData {
    pending: HashMap<LxPaymentId, Payment>,
}

impl<CM: LexeChannelManager<PS>, PS: LexePersister> PaymentsManager<CM, PS> {
    /// Instantiates a new [`PaymentsManager`] and spawns associated tasks.
    pub fn new(
        persister: PS,
        channel_manager: CM,
        esplora: Arc<LexeEsplora>,
        pending_payments: Vec<Payment>,
        wallet: LexeWallet,
        onchain_recv_rx: notify::Receiver,
        test_event_tx: TestEventSender,
        shutdown: NotifyOnce,
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

        let data = Arc::new(Mutex::new(PaymentsData { pending }));

        let myself = Self {
            data,
            persister,
            channel_manager,
            test_event_tx,
        };

        let payments_tasks = [
            myself.spawn_payment_expiry_checker(shutdown.clone()),
            myself.spawn_onchain_confs_checker(esplora, shutdown.clone()),
            myself.spawn_onchain_recv_checker(
                wallet,
                onchain_recv_rx,
                shutdown,
            ),
        ];

        (myself, payments_tasks)
    }

    fn spawn_payment_expiry_checker(
        &self,
        mut shutdown: NotifyOnce,
    ) -> LxTask<()> {
        let payman = self.clone();
        LxTask::spawn_with_span(
            "payment expiry checker",
            info_span!("(payment-expiry-checker)"),
            async move {
                let mut check_timer = tokio::time::interval_at(
                    Instant::now() + PAYMENT_EXPIRY_CHECK_DELAY,
                    PAYMENT_EXPIRY_CHECK_INTERVAL,
                );

                loop {
                    tokio::select! {
                        _ = check_timer.tick() => (),
                        () = shutdown.recv() => break,
                    }

                    let check_result = tokio::select! {
                        res = payman.check_payment_expiries() => res,
                        () = shutdown.recv() => break,
                    };

                    if let Err(e) = check_result {
                        error!("Error checking payment expiries: {e:#}");
                    }
                }

                info!("Invoice payment checker task shutting down");
            },
        )
    }

    fn spawn_onchain_confs_checker(
        &self,
        esplora: Arc<LexeEsplora>,
        mut shutdown: NotifyOnce,
    ) -> LxTask<()> {
        let payman = self.clone();

        LxTask::spawn_with_span(
            "onchain confs checker",
            info_span!("(onchain-confs-checker)"),
            async move {
                let mut check_timer = tokio::time::interval_at(
                    Instant::now() + ONCHAIN_PAYMENT_CHECK_DELAY,
                    ONCHAIN_PAYMENT_CHECK_INTERVAL,
                );
                loop {
                    tokio::select! {
                        _ = check_timer.tick() => (),
                        () = shutdown.recv() => break,
                    }

                    let check_result = tokio::select! {
                        res = payman.check_onchain_confs(&esplora) => res,
                        () = shutdown.recv() => break,
                    };

                    if let Err(e) = check_result {
                        error!("Error checking onchain confs: {e:#}");
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
        mut shutdown: NotifyOnce,
    ) -> LxTask<()> {
        let payman = self.clone();
        LxTask::spawn_with_span(
            "onchain receive checker",
            info_span!("(onchain-recv-checker)"),
            async move {
                loop {
                    tokio::select! {
                        () = onchain_recv_rx.recv() => (),
                        () = shutdown.recv() => break,
                    }

                    let check_result = tokio::select! {
                        res = payman.check_onchain_receives(&wallet) => res,
                        () = shutdown.recv() => break,
                    };

                    if let Err(e) = check_result {
                        error!("Error checking onchain recvs: {e:#}");
                    }
                }

                info!("Onchain receive checker task shutting down");
            },
        )
    }

    /// Register a new, globally-unique payment.
    /// Errors if the payment already exists.
    // TODO(phlip9): might be clearer semantics if we assign the
    // new payment's `created_at` _inside_ the lock... this would make
    // payments properly append-only with a strictly increasing `created_at`
    #[instrument(skip_all, name = "(new-payment)")]
    pub async fn new_payment(&self, payment: Payment) -> anyhow::Result<()> {
        let mut locked_data = self.data.lock().await;
        self.new_payment_inner(&mut locked_data, payment).await
    }

    /// [`Self::new_payment`] with a pre-acquired lock.
    async fn new_payment_inner(
        &self,
        locked_data: &mut MutexGuard<'_, PaymentsData>,
        payment: Payment,
    ) -> anyhow::Result<()> {
        let id = payment.id();
        info!(%id, "Registering new payment");

        let existing_payment = self
            .get_cow_payment(locked_data, &id)
            .await
            .context("Could not get existing payment")?;
        if let Some(existing_payment) = existing_payment {
            let status = existing_payment.status();
            return Err(anyhow!("Payment already exists: {status}"));
        }

        let checked = CheckedPayment(payment);

        let persisted = self
            .persister
            .create_payment(checked)
            .await
            .context("Could not persist new payment")?;

        locked_data.commit(persisted);

        Ok(())
    }

    /// Get a [`Payment`] by its [`LxPaymentId`].
    /// Returns the in-memory payment if cached, fetches from the DB otherwise.
    pub async fn get_payment(
        &self,
        id: &LxPaymentId,
    ) -> anyhow::Result<Option<Payment>> {
        let locked_data = self.data.lock().await;
        self.get_cow_payment(&locked_data, id)
            .await
            .map(|maybe_payment| maybe_payment.map(|p| p.into_owned()))
    }

    /// Given a locked [`PaymentsData`], get either a reference to an in-memory
    /// payment or an owned payment fetched from the DB, if it exists.
    async fn get_cow_payment<'param>(
        &self,
        locked_data: &'param MutexGuard<'_, PaymentsData>,
        id: &LxPaymentId,
    ) -> anyhow::Result<Option<Cow<'param, Payment>>> {
        if let Some(payment) = locked_data.pending.get(id) {
            return Ok(Some(Cow::Borrowed(payment)));
        }

        // TODO(max): Maybe cache finalized payments that were fetched?
        // Then we could early return here as well.

        self.persister
            .get_payment_by_id(*id)
            .await
            .map(|maybe_payment| maybe_payment.map(Cow::Owned))
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
            None => self
                .persister
                .get_payment_by_index(update.index)
                .await
                .context("Could not fetch finalized payment")?
                .context("Finalized payment was not found in DB")?,
        };
        // TODO(max): Switch to get_cow_payment once the payment note is
        // separated from the core `Payment` type, o/w we might read stale data.
        // let mut payment_clone = self
        //     .get_cow_payment(&locked_data, &id)
        //     .await
        //     .context("Could not get payment")?
        //     .context("Payment does not exist")?
        //     .into_owned();

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
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    #[instrument(skip_all, name = "(payment-claimable)")]
    pub async fn payment_claimable(
        &self,
        purpose: PaymentPurpose,
        hash: LxPaymentHash,
        // TODO(phlip9): make non-Option once replaying events drain in prod
        claim_id: Option<LnClaimId>,
        amt_msat: u64,
    ) -> anyhow::Result<()> {
        // MUST call one of these before this function returns:
        // - `channel_manager.claim_funds*`
        // - `channel_manager.fail_htlc_backwards*`

        let amount = Amount::from_msat(amt_msat);
        info!(%amount, %hash, "Handling PaymentClaimable");

        // The conversion can only fail if the preimage is unknown.
        let claim_ctx =
            LnClaimCtx::new(purpose, hash, claim_id).inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash.into(),
                    FailureCode::IncorrectOrUnknownPaymentDetails,
                )
            })?;

        let preimage = claim_ctx.preimage();

        // NOTE: avoid touching the ChannelManager while holding the lock
        let handle_claimable = || async {
            let mut locked_data = self.data.lock().await;

            // The PaymentClaimable docs have a note that LDK will not stop an
            // inbound payment from being paid multiple times. We should fail
            // the payment in this case because:
            // - This messes up (or significantly complicates) our accounting
            // - This likely reflects an error on the receiver's part (reusing
            //   the same invoice for multiple payments, which would allow any
            //   nodes along the first payment path to steal later payments)
            // - We should not allow payments to go through, in order to teach
            //   users that this is not an acceptable way to use lightning,
            //   because it is not safe. It is not hard to imagine users
            //   developing the misconception that it is safe to reuse invoices
            //   if duplicate payments actually do succeed.
            let is_finalized = self
                .get_cow_payment(&locked_data, &claim_ctx.id())
                .await
                .context("Could not get payment")
                // Couldn't check if finalized; have to retry handling later.
                .map_err(ClaimableError::Replay)?
                .is_some_and(|p| p.status().is_finalized());
            if is_finalized {
                warn!(%amount, %hash, "Already finalized");
                // Clear these pending HTLCs (if they still exist) so they don't
                // stick around until expiration
                return Err(ClaimableError::FailBackHtlcsTheirFault);
            }

            // Check
            let checked =
                // `check_payment_claimable` precondition: must not be finalized
                locked_data.check_payment_claimable(claim_ctx, amount)?;

            // Persist
            let persisted = self
                .persister
                .persist_payment(checked)
                .await
                .map_err(ClaimableError::Persist)?;

            // Commit
            locked_data.commit(persisted);

            Ok::<(), ClaimableError>(())
        };

        // Callback into the channel manager
        let hash = hash.into();
        match handle_claimable().await {
            Ok(()) => {
                // Everything ok; claim the payment.
                self.channel_manager.claim_funds(preimage.into());
                self.test_event_tx.send(TestEvent::PaymentClaimable);
            }
            Err(ClaimableError::IgnoreAndReclaim) => {
                // Maybe we crashed before channel manager could persist;
                // re-claim the payment.
                self.channel_manager.claim_funds(preimage.into());
            }
            Err(ClaimableError::Replay(e)) => {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash,
                    FailureCode::IncorrectOrUnknownPaymentDetails,
                );
                return Err(e);
            }
            Err(ClaimableError::Persist(e)) => {
                warn!("Failed to persist payment after claimable: {e:#}");
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash,
                    FailureCode::TemporaryNodeFailure,
                );
            }
            Err(ClaimableError::FailBackHtlcsTheirFault) => {
                self.channel_manager.fail_htlc_backwards_with_reason(
                    &hash,
                    FailureCode::IncorrectOrUnknownPaymentDetails,
                );
            }
        }

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
        Ok(())
    }

    /// Handles a [`PaymentClaimed`] event.
    ///
    /// [`PaymentClaimed`]: lightning::events::Event::PaymentClaimed
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
    #[instrument(skip_all, name = "(payment-claimed)")]
    pub async fn payment_claimed(
        &self,
        purpose: PaymentPurpose,
        hash: LxPaymentHash,
        claim_id: Option<LnClaimId>,
        amt_msat: u64,
    ) -> anyhow::Result<()> {
        let amount = Amount::from_msat(amt_msat);
        info!(%amount, %hash, "Handling PaymentClaimed");
        let claim_ctx = LnClaimCtx::new(purpose, hash, claim_id)?;

        let mut locked_data = self.data.lock().await;

        // check_payment_claimed precondition: Must not be finalized
        let is_finalized = self
            .get_cow_payment(&locked_data, &claim_ctx.id())
            .await
            .context("Could not get payment")?
            .is_some_and(|p| p.status().is_finalized());
        if is_finalized {
            warn!(%amount, %hash, "Already finalized");
            // Idempotency: If the payment was already finalized,
            //              we don't need to do anything.
            return Ok(());
        }

        // Check
        let checked = locked_data
            .check_payment_claimed(claim_ctx, amount)
            .context("Error validating PaymentClaimed")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        // TODO(phlip9): test event is not the right approach for observing
        // a payment's status.
        self.test_event_tx.send(TestEvent::PaymentClaimed);

        Ok(())
    }

    /// Handles an `EventHandler` -> [`PaymentSent`] event (replayable).
    ///
    /// [`PaymentSent`]: lightning::events::Event::PaymentSent
    // Event sources:
    // - `EventHandler` -> `Event::PaymentSent` (replayable)
    #[instrument(skip_all, name = "(payment-sent)", fields(%hash), err)]
    pub async fn payment_sent(
        &self,
        id: LxPaymentId,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid_msat: Option<u64>,
    ) -> anyhow::Result<()> {
        let maybe_fees_paid = maybe_fees_paid_msat.map(Amount::from_msat);
        info!(?maybe_fees_paid, "Handling PaymentSent");

        let mut locked_data = self.data.lock().await;

        // check_payment_sent precondition: Must not be finalized
        let is_finalized = self
            .get_cow_payment(&locked_data, &id)
            .await
            .context("Could not get payment")?
            .is_some_and(|p| p.status().is_finalized());
        if is_finalized {
            warn!(%hash, "Already finalized");
            // Idempotency: If the payment was already finalized,
            //              we don't need to do anything.
            return Ok(());
        }

        // Check
        let checked = locked_data
            .check_payment_sent(id, hash, preimage, maybe_fees_paid)
            .context("Error validating PaymentSent")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        // TODO(phlip9): test event is not the right approach for observing
        // a payment's status.
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
    // Event sources:
    // - `EventHandler` -> `Event::PaymentFailed` (replayable)
    // - `pay_invoice` API
    #[instrument(skip_all, name = "(payment-failed)", fields(?failure, %id), err)]
    pub async fn payment_failed(
        &self,
        id: LxPaymentId,
        // TODO(phlip9): Option<LxPaymentHash>,
        failure: LxOutboundPaymentFailure,
    ) -> anyhow::Result<()> {
        warn!("handling PaymentFailed");

        let mut locked_data = self.data.lock().await;

        // check_payment_failed precondition: Must not be finalized
        let is_finalized = self
            .get_cow_payment(&locked_data, &id)
            .await
            .context("Could not get payment")?
            .is_some_and(|p| p.status().is_finalized());
        if is_finalized {
            warn!(%id, "Already finalized");
            // Idempotency: If the payment was already finalized,
            //              we don't need to do anything.
            return Ok(());
        }

        // Check
        let checked = locked_data
            .check_payment_failed(id, failure)
            .context("Error validating PaymentFailed")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

        // Commit
        locked_data.commit(persisted);

        // TODO(phlip9): test event is not the right approach for observing
        // a payment's status.
        self.test_event_tx.send(TestEvent::PaymentFailed);

        Ok(())
    }

    /// Times out any pending inbound or outbound payments that have expired.
    /// This function should be called regularly.
    //
    // Event sources:
    // - `PaymentsManager::spawn_payment_expiry_checker` task
    #[instrument(skip_all, name = "(check-payment-expiries)")]
    pub async fn check_payment_expiries(&self) -> anyhow::Result<()> {
        debug!("Checking payment expiries");

        // NOTE: avoid touching the ChannelManager while holding the lock
        let ops_to_abandon = {
            // Check
            let mut locked_data = self.data.lock().await;

            // Call TimestampMs::now() just once then pass it in everywhere.
            let now = TimestampMs::now();

            let (all_checked, ops_to_abandon) = locked_data
                .check_payment_expiries(now)
                .context("Error checking payment expiries")?;

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

            ops_to_abandon
        };

        // Abandon all expired outbound payments.
        // We'll also abandon any abandoning payments to handle the case where
        // we crash after persisting above, but before the channel manager
        // persists.
        for payment_id in ops_to_abandon {
            self.channel_manager.abandon_payment(payment_id);
        }

        debug!("Successfully checked payment expiries");
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

        let payment = self
            .get_cow_payment(&locked_data, id)
            .await
            .context("Could not get payment")?
            .context("Payment does not exist")?;

        // TODO(phlip9): races with sync after broadcast
        ensure!(
            !payment.status().is_finalized(),
            "Onchain send was already finalized",
        );
        // From here, we know the payment is pending.

        // Check
        let checked = match payment.as_ref() {
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

    /// Queries the [`bdk_wallet::Wallet`] to see if there are any onchain
    /// receives that the [`PaymentsManager`] doesn't yet know about. If so,
    /// the [`OnchainReceive`] is constructed and registered with the
    /// [`PaymentsManager`].
    ///
    /// This function should be called regularly.
    #[instrument(skip_all, name = "(check-onchain-receives)")]
    pub async fn check_onchain_receives(
        &self,
        wallet: &LexeWallet,
    ) -> anyhow::Result<()> {
        debug!("Checking for onchain receives");
        let mut locked_data = self.data.lock().await;

        // List the txids of UTXOs owned by us (from BDK's perspective).
        let unspent_txids = {
            let locked_wallet = wallet.read();
            locked_wallet
                .list_unspent()
                // Only register payments to the external descriptor, so there
                // aren't entries for channel closes / splices / etc.
                // TODO(max): We actually want to remove this in the future, so
                // that we can track the sweeping of funds from closed channels;
                // OnchainRecv would include channel sweeps and anything that
                // spends to the BDK wallet. We should match the channel sweep
                // credit with the channel close debit, but channel opens /
                // closes need to be tracked separately.
                .filter(|o| matches!(o.keychain, KeychainKind::External))
                .map(|local_output| local_output.outpoint.txid)
                .collect::<Vec<_>>()
        };

        // Filter out txids we've already seen (already registered).
        let unseen_txids = {
            let mut unseen = Vec::new();
            for txid in &unspent_txids {
                let id = LxPaymentId::OnchainRecv(LxTxid(*txid));
                let is_unseen_txid = self
                    .get_cow_payment(&locked_data, &id)
                    .await
                    .with_context(|| *txid)
                    .context("Could not check whether txid already exists")?
                    .is_none();
                if is_unseen_txid {
                    unseen.push(*txid);
                }
            }
            unseen
        };

        // Construct new `OnchainReceive`s for each unseen txid.
        let onchain_recvs = {
            let locked_wallet = wallet.read();
            unseen_txids
                .into_iter()
                .map(|txid| {
                    let canonical_tx = locked_wallet
                        .get_tx(txid)
                        .context("Missing full tx for owned output")?;
                    let raw_tx = canonical_tx.tx_node.tx;
                    let (_, received) =
                        locked_wallet.sent_and_received(&raw_tx);
                    let amount =
                        Amount::try_from(received).context("Overflowed")?;
                    Ok(OnchainReceive::new(raw_tx, amount))
                })
                .collect::<anyhow::Result<Vec<OnchainReceive>>>()?
        };

        // Register all of the new onchain receives.
        for or in onchain_recvs {
            self.new_payment_inner(&mut locked_data, or.into())
                .await
                .context("Failed to register new onchain receive")?;
        }
        debug!("Successfully checked for and registered new onchain receives");

        // The PaymentsData lock is finally dropped here.
        Ok(())
    }
}

impl PaymentsData {
    /// Commits a [`PersistedPayment`] to the local state.
    fn commit(&mut self, persisted: PersistedPayment) {
        let payment = persisted.0;
        let id = payment.id();

        payment.debug_assert_invariants();
        self.debug_assert_invariants();

        match payment.status() {
            PaymentStatus::Pending => {
                self.pending.insert(id, payment);
            }
            PaymentStatus::Completed | PaymentStatus::Failed => {
                self.pending.remove(&id);
            }
        }

        self.debug_assert_invariants();
    }

    /// Assert invariants about the internal state of the [`PaymentsData`] when
    /// `cfg!(debug_assertions)` is enabled. This is a no-op in production.
    fn debug_assert_invariants(&self) {
        if cfg!(not(debug_assertions)) {
            return;
        }

        for (id, payment) in &self.pending {
            assert_eq!(payment.id(), *id);
            assert_eq!(payment.status(), PaymentStatus::Pending);
            payment.debug_assert_invariants();
        }
    }

    /// Precondition: Payment must not be finalized (Completed | Failed).
    ///               It's OK for the payment to not exist (new payment).
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimable` (replayable)
    fn check_payment_claimable(
        &self,
        claim_ctx: LnClaimCtx,
        amount: Amount,
    ) -> Result<CheckedPayment, ClaimableError> {
        let id = claim_ctx.id();

        let maybe_pending_payment = self.pending.get(&id);

        match maybe_pending_payment {
            // Pending payment exists; update it
            Some(pending_payment) =>
                pending_payment.check_payment_claimable(claim_ctx, amount),
            None => match claim_ctx {
                LnClaimCtx::Bolt11Invoice { .. } =>
                    Err(ClaimableError::Replay(anyhow!(
                        "Tried to claim non-existent inbound invoice payment"
                    ))),
                LnClaimCtx::Bolt12Offer(ctx) => {
                    let now = TimestampMs::now();
                    let iorp =
                        InboundOfferReusablePayment::new(ctx, amount, now);
                    let payment = Payment::from(iorp);
                    Ok(CheckedPayment(payment))
                }
                LnClaimCtx::Spontaneous {
                    hash,
                    preimage,
                    claim_id: _,
                } => {
                    // We just got a new spontaneous payment!
                    // Create the new payment.
                    let isp =
                        InboundSpontaneousPayment::new(hash, preimage, amount);
                    let payment = Payment::from(isp);
                    Ok(CheckedPayment(payment))
                }
            },
        }
    }

    /// Precondition: Payment must not be finalized (Completed | Failed).
    // Event sources:
    // - `EventHandler` -> `Event::PaymentClaimed` (replayable)
    fn check_payment_claimed(
        &self,
        claim_ctx: LnClaimCtx,
        amount: Amount,
    ) -> anyhow::Result<CheckedPayment> {
        let id = claim_ctx.id();

        let pending_payment = self
            .pending
            .get(&id)
            .context("Pending payment does not exist")?;

        let checked = match (pending_payment, claim_ctx) {
            (
                Payment::InboundInvoice(iip),
                LnClaimCtx::Bolt11Invoice {
                    preimage,
                    hash,
                    secret,
                    claim_id: _,
                },
            ) => iip
                .check_payment_claimed(hash, secret, preimage, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing inbound invoice payment")?,
            (
                Payment::InboundOfferReusable(iorp),
                LnClaimCtx::Bolt12Offer(ctx),
            ) => iorp
                .check_payment_claimed(ctx, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing reusable inbound offer payment")?,
            (
                Payment::InboundSpontaneous(isp),
                LnClaimCtx::Spontaneous {
                    preimage,
                    hash,
                    claim_id: _,
                },
            ) => isp
                .check_payment_claimed(hash, preimage, amount)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error finalizing inbound spontaneous payment")?,
            // TODO(phlip9): impl BOLT 12 refunds
            _ => bail!(
                "Not an inbound LN payment, or claim context didn't match"
            ),
        };

        Ok(checked)
    }

    /// Precondition: Payment must not be finalized (Completed | Failed).
    // Event sources:
    // - `EventHandler` -> `Event::PaymentSent` (replayable)
    fn check_payment_sent(
        &self,
        id: LxPaymentId,
        hash: LxPaymentHash,
        preimage: LxPaymentPreimage,
        maybe_fees_paid: Option<Amount>,
    ) -> anyhow::Result<CheckedPayment> {
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
            Payment::OutboundOffer(oop) => oop
                .check_payment_sent(hash, preimage, maybe_fees_paid)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error checking outbound offer payment")?,
            Payment::OutboundSpontaneous(_) => todo!(),
            _ => bail!("Not an outbound Lightning payment"),
        };

        Ok(checked)
    }

    /// Precondition: Payment must not be finalized (Completed | Failed).
    // Event sources:
    // - `EventHandler` -> `Event::PaymentFailed` (replayable)
    // - `pay_invoice` API
    fn check_payment_failed(
        &self,
        id: LxPaymentId,
        failure: LxOutboundPaymentFailure,
    ) -> anyhow::Result<CheckedPayment> {
        let pending_payment = self
            .pending
            .get(&id)
            .context("Pending payment does not exist")?;

        let checked = match pending_payment {
            Payment::OutboundInvoice(oip) => oip
                .check_payment_failed(id, failure)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error checking outbound invoice payment")?,
            Payment::OutboundOffer(oop) => oop
                .check_payment_failed(failure)
                .map(Payment::from)
                .map(CheckedPayment)
                .context("Error checking outbound offer payment")?,
            Payment::OutboundSpontaneous(_) => todo!(),
            _ => bail!("Not an outbound Lightning payment"),
        };

        Ok(checked)
    }

    /// Returns all _newly_ expired payments and the hashes of all outbound
    /// payments which should be passed to [`abandon_payment`].
    ///
    /// [`abandon_payment`]: lightning::ln::channelmanager::ChannelManager::abandon_payment
    //
    // Event sources:
    // - `PaymentsManager::spawn_payment_expiry_checker` task
    fn check_payment_expiries(
        &self,
        now: TimestampMs,
    ) -> anyhow::Result<(
        Vec<CheckedPayment>,
        Vec<lightning::ln::channelmanager::PaymentId>,
    )> {
        let mut ops_to_abandon = Vec::new();
        let all_expired = self
            .pending
            .values()
            .filter_map(|payment| match payment {
                // Precondition: payment is not finalized (Completed | Failed).
                Payment::InboundInvoice(iip) => iip
                    .check_invoice_expiry(now)
                    .map(Payment::from)
                    .map(CheckedPayment),
                Payment::OutboundInvoice(oip) => {
                    match oip.check_invoice_expiry(now) {
                        Ok(oip) => {
                            ops_to_abandon.push(oip.ldk_id());
                            Some(CheckedPayment(Payment::from(oip)))
                        }
                        Err(ExpireError::Ignore) => None,
                        Err(ExpireError::IgnoreAndAbandon) => {
                            ops_to_abandon.push(oip.ldk_id());
                            None
                        }
                    }
                }
                Payment::OutboundOffer(oop) => {
                    match oop.check_offer_expiry(now) {
                        Ok(oop) => {
                            ops_to_abandon.push(oop.ldk_id());
                            Some(CheckedPayment(Payment::from(oop)))
                        }
                        Err(ExpireError::Ignore) => None,
                        Err(ExpireError::IgnoreAndAbandon) => {
                            ops_to_abandon.push(oop.ldk_id());
                            None
                        }
                    }
                }
                _ => None,
            })
            .collect::<Vec<_>>();

        Ok((all_expired, ops_to_abandon))
    }

    // Event sources:
    // - `PaymentsManager::spawn_onchain_confs_checker` task
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

#[cfg(test)]
mod arb {
    use proptest::{
        arbitrary::{Arbitrary, any},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for PaymentsData {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            proptest::collection::vec(any::<Payment>(), 0..10)
                .prop_map(PaymentsData::from_vec)
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::ByteArray;
    use proptest::{
        arbitrary::{any, any_with},
        prop_assert_eq, proptest,
    };

    use super::*;
    use crate::payments::{
        inbound::{
            InboundInvoicePayment, InboundOfferReusablePayment, OfferClaimCtx,
        },
        outbound::{
            OutboundInvoicePayment, OutboundInvoicePaymentStatus,
            OutboundOfferPayment, arb::OipParams,
        },
    };

    impl PaymentsData {
        pub(super) fn from_vec(mut payments: Vec<Payment>) -> Self {
            // Remove duplicates
            payments.sort_unstable_by_key(Payment::id);
            payments.dedup_by_key(|p| p.id());

            let pending = HashMap::default();

            // Insert all payments
            let mut out = Self { pending };
            for payment in payments {
                out.insert_payment(payment);
            }

            // Make sure we didn't lose any payments somehow
            out.debug_assert_invariants();

            out
        }

        /// Insert a payment into `PaymentsData` without running through the
        /// full state machine. Only pending payments are inserted.
        fn insert_payment(&mut self, payment: Payment) {
            let id = payment.id();
            if payment.status().is_pending() {
                self.pending.insert(id, payment);
            }
        }

        /// Forcibly insert a payment into the `PaymentsData`, removing any
        /// existing payment with the same ID.
        fn force_insert_payment(&mut self, payment: Payment) {
            let id = payment.id();
            self.pending.remove(&id);
            self.insert_payment(payment);
            self.debug_assert_invariants();
        }
    }

    impl CheckedPayment {
        /// Assert that a `CheckedPayment` was persisted without actually
        /// persisting.
        fn persisted(self) -> PersistedPayment {
            PersistedPayment(self.0)
        }
    }

    #[test]
    fn prop_inbound_spontaneous_payment_idempotency() {
        let pending_only = true;
        proptest!(|(
            mut data in any::<PaymentsData>(),
            // check_payment_claimable precondition: Must not be finalized
            //   check_payment_claimed precondition: Must not be finalized
            isp in any_with::<InboundSpontaneousPayment>(pending_only),
            // currently does nothing for spontaneous payments, but could catch
            // an unintended change.
            claim_id in any::<Option<LnClaimId>>(),
        )| {
            let payment = Payment::InboundSpontaneous(isp.clone());
            data.force_insert_payment(payment.clone());

            let amount = isp.amount;
            let claim_ctx = LnClaimCtx::Spontaneous {
                preimage: isp.preimage,
                hash: isp.hash,
                claim_id,
            };

            // NOTE: New payment duplicate check moved to manager/DB layer.

            let _ = data
                .check_payment_claimable(
                    claim_ctx.clone(),
                    amount,
                )
                .inspect_err(|err| assert!(!err.is_replay()));

            let _ = data.check_payment_claimed(claim_ctx, amount)
                .unwrap();
        });
    }

    #[test]
    fn prop_inbound_invoice_payment_idempotency() {
        let pending_only = true;
        proptest!(|(
            mut data in any::<PaymentsData>(),
            // check_payment_claimable precondition: Must not be finalized
            //   check_payment_claimed precondition: Must not be finalized
            iip in any_with::<InboundInvoicePayment>(pending_only),
            recvd_amount in any::<Amount>(),
            claim_id in any::<Option<Option<LnClaimId>>>(),
        )| {
            let payment = Payment::InboundInvoice(iip.clone());
            data.force_insert_payment(payment.clone());

            let recvd_amount = iip.recvd_amount.unwrap_or(recvd_amount);

            // Support 3 cases:
            // 1. same claim_id as the payment
            // 2. different claim_id from the payment
            // 3. pre- node-v0.7.0 with no claim_id
            let claim_id = claim_id.unwrap_or(iip.claim_id);

            let claim_ctx = LnClaimCtx::Bolt11Invoice {
                preimage: iip.preimage,
                hash: iip.hash,
                secret: iip.secret,
                claim_id,
            };

            // NOTE: New payment duplicate check moved to manager/DB layer.

            let _ = data
                .check_payment_claimable(
                    claim_ctx.clone(),
                    recvd_amount,
                )
                .inspect_err(|err| assert!(!err.is_replay()));

            let _ = data.check_payment_claimed(claim_ctx, recvd_amount)
                .unwrap();

            data.check_payment_expiries(TimestampMs::MAX).unwrap();
        });
    }

    #[test]
    fn prop_inbound_offer_reuse_payment_idempotency() {
        let pending_only = true;
        proptest!(|(
            mut data in any::<PaymentsData>(),
            // check_payment_claimable precondition: Must not be finalized
            //   check_payment_claimed precondition: Must not be finalized
            iorp in any_with::<InboundOfferReusablePayment>(pending_only),
        )| {
            let payment = Payment::InboundOfferReusable(iorp.clone());
            data.force_insert_payment(payment.clone());

            let claim_ctx = LnClaimCtx::Bolt12Offer(OfferClaimCtx {
                preimage: iorp.preimage,
                claim_id: iorp.claim_id,
                offer_id: iorp.offer_id,
                quantity: iorp.quantity,
                payer_note: iorp.payer_note,
                payer_name: iorp.payer_name,
            });

            // NOTE: New payment duplicate check moved to manager/DB layer.

            let _ = data
                .check_payment_claimable(
                    claim_ctx.clone(),
                    iorp.amount,
                )
                .inspect_err(|err| assert!(!err.is_replay()));

            let _ = data.check_payment_claimed(claim_ctx, iorp.amount)
                .unwrap();
        });
    }

    #[test]
    fn prop_outbound_invoice_payment_idempotency() {
        let preimage = LxPaymentPreimage::from_array([0x42; 32]);
        proptest!(|(
            mut data in any::<PaymentsData>(),
            //   check_payment_sent precondition: Must not be finalized
            // check_payment_failed precondition: Must not be finalized
            oip in any_with::<OutboundInvoicePayment>(OipParams {
                payment_preimage: Some(preimage),
                pending_only: true,
            }),
            failure in any::<LxOutboundPaymentFailure>(),
        )| {
            let payment = Payment::OutboundInvoice(oip.clone());
            let id = payment.id();
            data.force_insert_payment(payment);

            let _ = data.check_payment_sent(id, oip.hash, preimage, Some(oip.fees))
                .unwrap();
            let _ = data.check_payment_failed(id, failure).unwrap();
            data.check_payment_expiries(TimestampMs::MAX).unwrap();
        });
    }

    #[test]
    fn prop_outbound_offer_payment_idempotency() {
        let pending_only = true;
        proptest!(|(
            mut data in any::<PaymentsData>(),
            //   check_payment_sent precondition: Must not be finalized
            // check_payment_failed precondition: Must not be finalized
            oop in any_with::<OutboundOfferPayment>(pending_only),
            preimage in any::<LxPaymentPreimage>(),
            fees in any::<Amount>(),
            failure in any::<LxOutboundPaymentFailure>(),
        )| {
            let payment = Payment::OutboundOffer(oop.clone());
            let id = payment.id();
            data.force_insert_payment(payment);

            let hash = preimage.compute_hash();
            let _ = data.check_payment_sent(id, hash, preimage, Some(fees))
                .unwrap();
            let _ = data.check_payment_failed(id, failure).unwrap();
            data.check_payment_expiries(TimestampMs::MAX).unwrap();
        });
    }

    #[test]
    fn prop_outbound_invoice_payment() {
        use OutboundInvoicePaymentStatus::*;

        let preimage = LxPaymentPreimage::from_array([0x42; 32]);
        proptest!(|(
            oip in any_with::<OutboundInvoicePayment>(OipParams {
                payment_preimage: Some(preimage),
                //   check_payment_sent precondition: Must not be finalized
                // check_payment_failed precondition: Must not be finalized
                pending_only: true,
            }),
            failure in any::<LxOutboundPaymentFailure>(),
        )| {

            let hash = oip.hash;
            let fees = oip.fees;
            let status = oip.status;

            let payment = Payment::OutboundInvoice(oip.clone());
            let id = payment.id();
            let data = PaymentsData::from_vec(vec![payment.clone()]);

            // NOTE: New payment duplicate check moved to manager/DB layer.

            // (_, PaymentSent event) -> _
            let checked = data
                .check_payment_sent(id, hash, preimage, Some(fees))
                .unwrap();
            prop_assert_eq!(PaymentStatus::Completed, checked.0.status());
            data.clone().commit(checked.persisted());

            // (_, PaymentFailed event) -> _
            let checked = data.check_payment_failed(id, failure)
                .unwrap();
            prop_assert_eq!(PaymentStatus::Failed, checked.0.status());
            data.clone().commit(checked.persisted());

            // (_, Invoice expires) -> _
            let (mut checked_payments, _ids) = data
                .check_payment_expiries(TimestampMs::MAX)
                .unwrap();
            match status {
                Pending => {
                    assert_eq!(1, checked_payments.len());
                    let checked = checked_payments.pop().unwrap();
                    match &checked.0 {
                        Payment::OutboundInvoice(oip) =>
                            prop_assert_eq!(Abandoning, oip.status),
                        _ => unreachable!(),
                    }
                    data.clone().commit(checked.persisted());
                }
                Abandoning =>
                    prop_assert_eq!(0, checked_payments.len()),
                _ => unreachable!("pending_only was set to true"),
            }

            // [Idempotency]
            // (_, Invoice not expired) -> do nothing
            let expires_at = oip.invoice.saturating_expires_at();
            let (checked_payments, _ids) = data
                .check_payment_expiries(expires_at)
                .unwrap();
            prop_assert_eq!(0, checked_payments.len());
        });
    }
}
