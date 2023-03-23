use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{bail, ensure, Context};
use lightning::util::events::PaymentPurpose;
use tokio::sync::Mutex;
use tracing::{info, instrument};

use crate::payments::inbound::{InboundSpontaneousPayment, LxPaymentPurpose};
use crate::payments::{LxPaymentHash, LxPaymentId, Payment, PaymentStatus};
use crate::test_event::{TestEvent, TestEventSender};
use crate::traits::{LexeChannelManager, LexePersister};

/// Annotates that a given [`Payment`] was returned by a `check_*` method which
/// successfully validated a proposed state transition. [`CheckedPayment`]s
/// should be persisted in order to transform into [`PersistedPayment`]s.
#[must_use]
pub struct CheckedPayment(pub Payment);

/// Annotates that a given [`Payment`] was successfully persisted, i.e. it was
/// returned by the [`persist_payment`] method. [`PersistedPayment`]s should be
/// committed to the local payments state.
///
/// [`persist_payment`]: crate::traits::LexeInnerPersister::persist_payment
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
///    [`persist_payment`] method.
/// 3) Commit: We commit the validated + persisted state transition to the local
///    state. This is done by [`PaymentsData::commit`].
///
/// To prevent update and persist races, a (Tokio) lock to the [`PaymentsData`]
/// struct (or at least the [`LxPaymentId`] of the payment) should be held
/// throughout the entirety of the state update, including the all of the check,
/// persist, and commit stages. TODO(max): If this turns out to be a performance
/// bottleneck, we should switch to per-payment or per-payment-type locks.
///
/// [`persist_payment`]: crate::traits::LexeInnerPersister::persist_payment
struct PaymentsData {
    pending: HashMap<LxPaymentId, Payment>,
    finalized: HashSet<LxPaymentId>,
}

impl<CM: LexeChannelManager<PS>, PS: LexePersister> PaymentsManager<CM, PS> {
    pub fn new(
        persister: PS,
        channel_manager: CM,
        test_event_tx: TestEventSender,
    ) -> Self {
        // TODO(max): Take initial data in parameters
        let data = Arc::new(Mutex::new(PaymentsData {
            pending: HashMap::new(),
            finalized: HashSet::new(),
        }));

        Self {
            data,
            persister,
            channel_manager,
            test_event_tx,
        }
    }

    /// Register a new, globally-unique payment.
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
            .persist_payment(checked)
            .await
            .context("Could not persist payment")?;

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
        let purpose = LxPaymentPurpose::try_from(purpose)?;

        // Check
        let mut locked_data = self.data.lock().await;
        let checked = locked_data
            .check_payment_claimable(hash, amt_msat, purpose)
            // If validation failed, fail the HTLC.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards(&hash.into())
            })
            .context("Error validating PaymentClaimable")?;

        // Persist
        let persisted = self
            .persister
            .persist_payment(checked)
            .await
            // If persistence failed, fail the HTLC.
            .inspect_err(|_| {
                self.channel_manager.fail_htlc_backwards(&hash.into())
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
            "Payment was was already finalized"
        );

        let checked = self
            .pending
            .get(&id)
            .context("Claimed payment does not exist")?
            .check_payment_claimed(hash, amt_msat, purpose)?;

        Ok(checked)
    }
}
