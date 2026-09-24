//! Publishes payment updates to all of a wallet's waiters.
//!
//! Waiters subscribe by [`PaymentId`](crate::types::payment::PaymentId) (or
//! to all updates), then take turns running the poll loop: whichever waiter
//! holds the poll lock reads the next batch of updates and publishes each to
//! its subscribers. So any number of waiters cost a single poll loop, which
//! runs only while someone waits.

use std::collections::{HashMap, hash_map::Entry};

use lexe_std::backoff::Backoff;
use lexe_tokio::notify;
use tokio::sync::watch;
use tracing::warn;

use crate::types::{
    command::{
        GetPaymentByIdRequest, GetPaymentResponse, GetUpdatedPaymentsRequest,
        GetUpdatedPaymentsResponse,
    },
    payment::{Payment, PaymentId, PaymentUpdatedIndex},
};

pub(crate) struct PaymentsBus {
    /// Reads the cursor without the poll lock. See [`PollState::cursor_tx`].
    cursor_rx: watch::Receiver<Option<PaymentUpdatedIndex>>,
    /// Each watched payment's latest published state.
    payment_txs:
        std::sync::Mutex<HashMap<PaymentId, watch::Sender<Option<Payment>>>>,
    /// Held by the waiter running the next poll.
    poll_state: tokio::sync::Mutex<PollState>,
    /// Polls immediately, restarting the backoff.
    poll_now_tx: notify::Sender,
}

/// The wallet API methods that we need to publish payment updates.
pub(crate) trait WalletUpdatesApi {
    async fn get_updated_payments(
        &self,
        req: GetUpdatedPaymentsRequest,
    ) -> anyhow::Result<GetUpdatedPaymentsResponse>;

    async fn get_payment_by_id(
        &self,
        req: GetPaymentByIdRequest,
    ) -> anyhow::Result<GetPaymentResponse>;
}

/// The poll loop's state, owned by whichever waiter is polling.
struct PollState {
    backoff: Backoff,
    /// The latest update read so far.
    /// `None` while the wallet has no payments.
    cursor_tx: watch::Sender<Option<PaymentUpdatedIndex>>,
    poll_now_rx: notify::Receiver,
}

/// A waiter's subscription to one payment. Unsubscribes on drop.
struct PaymentSubscription<'a> {
    /// Fields drop in order, so `rx` is released before the guard runs.
    rx: watch::Receiver<Option<Payment>>,
    _guard: SubscriptionGuard<'a>,
}

/// Removes the payment's sender on drop if it has no receivers left.
struct SubscriptionGuard<'a> {
    bus: &'a PaymentsBus,
    id: PaymentId,
}

impl PaymentsBus {
    /// The poll delay after a new waiter or a payment update. Doubles up to
    /// [`MAX_WAIT_MS`](Self::MAX_WAIT_MS) while nothing changes.
    const INITIAL_WAIT_MS: u64 = 250;
    const MAX_WAIT_MS: u64 = 4_000;

    /// Starts the cursor at `latest_update`. Polls only read updates after it.
    pub(crate) fn new(latest_update: Option<PaymentUpdatedIndex>) -> Self {
        let (cursor_tx, cursor_rx) = watch::channel(latest_update);
        let (poll_now_tx, poll_now_rx) = notify::channel();
        Self {
            cursor_rx,
            payment_txs: std::sync::Mutex::new(HashMap::new()),
            poll_state: tokio::sync::Mutex::new(PollState {
                backoff: Backoff::new(Self::INITIAL_WAIT_MS, Self::MAX_WAIT_MS),
                cursor_tx,
                poll_now_rx,
            }),
            poll_now_tx,
        }
    }

    /// Wait until the payment's state satisfies `condition`, and return it.
    pub(crate) async fn wait_for_payment_condition(
        &self,
        api: &impl WalletUpdatesApi,
        id: PaymentId,
        mut condition: impl FnMut(&Payment) -> bool,
    ) -> anyhow::Result<Payment> {
        let mut subscription = self.subscribe(id);

        // Early return if the condition is already satisfied.
        let req = GetPaymentByIdRequest { id };
        if let Some(payment) = api.get_payment_by_id(req).await?.payment
            && condition(&payment)
        {
            return Ok(payment);
        }

        let payment = self
            .poll_until(api, &mut subscription.rx, |maybe_payment| {
                maybe_payment.as_ref().filter(|p| condition(p)).cloned()
            })
            .await;

        Ok(payment)
    }

    /// Wait until there is an update later than `start_index`.
    pub(crate) async fn wait_for_next_update(
        &self,
        api: &impl WalletUpdatesApi,
        start_index: Option<PaymentUpdatedIndex>,
    ) -> anyhow::Result<()> {
        let mut cursor_rx = self.cursor_rx.clone();

        self.poll_until(api, &mut cursor_rx, |cursor| {
            // `Some(_) > None`, so `start_index: None` matches any update.
            (*cursor > start_index).then_some(())
        })
        .await;

        Ok(())
    }

    fn subscribe(&self, id: PaymentId) -> PaymentSubscription<'_> {
        let rx = match self.payment_txs.lock().unwrap().entry(id) {
            Entry::Occupied(entry) => entry.get().subscribe(),
            Entry::Vacant(entry) =>
                entry.insert(watch::Sender::new(None)).subscribe(),
        };

        let _guard = SubscriptionGuard { bus: self, id };
        PaymentSubscription { rx, _guard }
    }

    /// Take turns running polls until `extract` finds its value in `rx`.
    async fn poll_until<T, U>(
        &self,
        api: &impl WalletUpdatesApi,
        rx: &mut watch::Receiver<T>,
        mut extract: impl FnMut(&T) -> Option<U>,
    ) -> U {
        self.poll_now_tx.send();

        loop {
            if let Some(value) = extract(&rx.borrow_and_update()) {
                return value;
            }

            tokio::select! {
                // A poll may have just published to `rx`, so check it before
                // taking the lock, otherwise we'd sit out another poll delay.
                biased;
                // Our sender outlives us, so this never errors.
                _ = rx.changed() => (),
                mut locked_poll_state = self.poll_state.lock() =>
                    self.poll_once(api, &mut locked_poll_state).await,
            }
        }
    }

    /// Wait out the poll delay, or until told to poll now, then publish new
    /// updates.
    async fn poll_once(
        &self,
        api: &impl WalletUpdatesApi,
        poll_state: &mut PollState,
    ) {
        let delay = poll_state.backoff.next_delay();
        let poll_now =
            tokio::time::timeout(delay, poll_state.poll_now_rx.recv()).await;
        if poll_now.is_ok() {
            poll_state.backoff.reset();
        }

        match self.publish_updates(api, poll_state).await {
            Ok(true) => poll_state.backoff.reset(),
            Ok(false) => (),
            Err(e) => warn!("Payment update poll failed: {e:#}"),
        }
    }

    /// Read the updates after the cursor, and publish each to its subscribers.
    /// Returns whether there were any.
    async fn publish_updates(
        &self,
        api: &impl WalletUpdatesApi,
        poll_state: &mut PollState,
    ) -> anyhow::Result<bool> {
        let cursor_tx = &poll_state.cursor_tx;
        let req = GetUpdatedPaymentsRequest {
            start_index: *cursor_tx.borrow(),
            limit: None,
        };
        let resp = api.get_updated_payments(req).await?;

        let Some(latest) = resp.updated_index else {
            return Ok(false);
        };

        let locked_payment_txs = self.payment_txs.lock().unwrap();
        for payment in resp.payments {
            if let Some(payment_tx) = locked_payment_txs.get(&payment.index.id)
            {
                payment_tx.send_replace(Some(payment));
            }
        }

        cursor_tx.send_replace(Some(latest));

        Ok(true)
    }
}

impl Drop for SubscriptionGuard<'_> {
    fn drop(&mut self) {
        let mut locked_payment_txs = self.bus.payment_txs.lock().unwrap();
        if let Entry::Occupied(entry) = locked_payment_txs.entry(self.id)
            && entry.get().receiver_count() == 0
        {
            entry.remove();
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        collections::BTreeMap,
        future::Future,
        ops::Bound,
        sync::{
            Arc, Mutex, OnceLock,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use lexe_api::types::payments::{BasicPaymentV2, PaymentStatus};
    use lexe_common::{test_utils::arbitrary, time::TimestampMs};
    use lexe_crypto::rng::FastRng;
    use lexe_tokio::task::LxTask;
    use proptest::arbitrary::any;
    use tokio::time::Instant;

    use super::*;

    /// A wallet whose payment updates are set by the test.
    #[derive(Default)]
    struct MockWallet {
        /// Each payment's latest state, keyed by its update index.
        payments: Mutex<BTreeMap<PaymentUpdatedIndex, Payment>>,
        /// When each poll happened, and its `start_index`.
        polls: Mutex<Vec<(Instant, Option<PaymentUpdatedIndex>)>>,
        /// The number of upcoming polls that fail.
        failures: AtomicUsize,
        /// An update applied right after the next `get_payment_by_id` read.
        update_after_read: Mutex<Option<Payment>>,
        /// An update applied right after the next poll.
        update_after_poll: Mutex<Option<Payment>>,
    }

    /// A [`PaymentsBus`] over a [`MockWallet`], with waiters run as
    /// tasks. Like the wallet, the bus is created on first use.
    struct TestBus {
        wallet: Arc<MockWallet>,
        bus: OnceLock<Arc<PaymentsBus>>,
    }

    impl MockWallet {
        /// Replace a payment's state, as the node does when it persists one.
        fn update(&self, payment: Payment) {
            let mut locked_payments = self.payments.lock().unwrap();
            locked_payments.retain(|_, p| p.index.id != payment.index.id);
            locked_payments.insert(payment.updated_index(), payment);
        }

        fn poll_times(&self) -> Vec<Instant> {
            let locked_polls = self.polls.lock().unwrap();
            locked_polls.iter().map(|(time, _)| *time).collect()
        }

        fn latest_updated_index(&self) -> Option<PaymentUpdatedIndex> {
            let locked_payments = self.payments.lock().unwrap();
            locked_payments.last_key_value().map(|(idx, _)| *idx)
        }
    }

    impl WalletUpdatesApi for MockWallet {
        async fn get_updated_payments(
            &self,
            req: GetUpdatedPaymentsRequest,
        ) -> anyhow::Result<GetUpdatedPaymentsResponse> {
            self.polls
                .lock()
                .unwrap()
                .push((Instant::now(), req.start_index));

            let fail = self
                .failures
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    n.checked_sub(1)
                })
                .is_ok();
            anyhow::ensure!(!fail, "Mock poll failure");

            let start =
                req.start_index.map_or(Bound::Unbounded, Bound::Excluded);
            let payments = self
                .payments
                .lock()
                .unwrap()
                .range((start, Bound::Unbounded))
                .take(req.limit.unwrap_or(usize::MAX))
                .map(|(_, p)| p.clone())
                .collect::<Vec<_>>();
            let updated_index = payments.last().map(Payment::updated_index);

            if let Some(update) = self.update_after_poll.lock().unwrap().take()
            {
                self.update(update);
            }

            Ok(GetUpdatedPaymentsResponse {
                payments,
                updated_index,
            })
        }

        async fn get_payment_by_id(
            &self,
            req: GetPaymentByIdRequest,
        ) -> anyhow::Result<GetPaymentResponse> {
            let payment = self
                .payments
                .lock()
                .unwrap()
                .values()
                .find(|p| p.index.id == req.id)
                .cloned();

            if let Some(update) = self.update_after_read.lock().unwrap().take()
            {
                self.update(update);
            }

            Ok(GetPaymentResponse { payment })
        }
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                wallet: Arc::default(),
                bus: OnceLock::new(),
            }
        }

        fn bus(&self) -> &Arc<PaymentsBus> {
            self.bus.get_or_init(|| {
                let latest_update = self.wallet.latest_updated_index();
                Arc::new(PaymentsBus::new(latest_update))
            })
        }

        /// Spawn a waiter for payment `n` to finalize.
        fn wait_finalized(&self, n: u64) -> LxTask<Payment> {
            self.spawn(|wallet, bus| async move {
                bus.wait_for_payment_condition(&*wallet, id(n), |p| {
                    p.status.is_finalized()
                })
                .await
                .unwrap()
            })
        }

        /// Spawn a waiter for the next update after `start_index`.
        fn wait_for_next_update(
            &self,
            start_index: Option<PaymentUpdatedIndex>,
        ) -> LxTask<()> {
            self.spawn(|wallet, bus| async move {
                bus.wait_for_next_update(&*wallet, start_index)
                    .await
                    .unwrap()
            })
        }

        fn spawn<F: Future + Send + 'static>(
            &self,
            wait: impl FnOnce(Arc<MockWallet>, Arc<PaymentsBus>) -> F,
        ) -> LxTask<F::Output>
        where
            F::Output: Send + 'static,
        {
            let (wallet, bus) =
                (Arc::clone(&self.wallet), Arc::clone(self.bus()));
            LxTask::spawn_unnamed(wait(wallet, bus))
        }

        fn num_subscriptions(&self) -> usize {
            self.bus().payment_txs.lock().unwrap().len()
        }

        /// The status last published for payment `n`.
        fn published_status(&self, n: u64) -> Option<PaymentStatus> {
            let locked_payment_txs = self.bus().payment_txs.lock().unwrap();
            let published = locked_payment_txs[&id(n)].borrow();
            published.as_ref().map(|p| p.status)
        }
    }

    /// Payment `n` at `status`, last updated at `updated_at_ms`.
    fn payment(n: u64, updated_at_ms: i64, status: PaymentStatus) -> Payment {
        let mut rng = FastRng::from_u64(n);
        let mut payment =
            arbitrary::gen_value(&mut rng, any::<BasicPaymentV2>());
        payment.status = status;
        payment.updated_at = TimestampMs::try_from(updated_at_ms).unwrap();
        Payment::from(payment)
    }

    fn id(n: u64) -> PaymentId {
        payment(n, 0, PaymentStatus::Pending).index.id
    }

    /// When a poll loop reset at `from` polls, up to (excluding) `until`.
    fn poll_schedule(from: Instant, until: Instant) -> Vec<Instant> {
        let mut backoff = Backoff::new(
            PaymentsBus::INITIAL_WAIT_MS,
            PaymentsBus::MAX_WAIT_MS,
        );
        let mut times = Vec::new();
        let mut time = from;
        while time < until {
            times.push(time);
            time += backoff.next_delay();
        }

        times
    }

    /// Sleep long enough for the next poll to happen, however backed off.
    /// Ends between polls, so no assertion races a poll.
    async fn next_poll() {
        tokio::time::sleep(Duration::from_millis(PaymentsBus::MAX_WAIT_MS + 1))
            .await
    }

    /// Await a waiter, failing rather than hanging if it never finishes.
    async fn finish<T>(waiter: LxTask<T>) -> T {
        tokio::time::timeout(Duration::from_secs(60), waiter)
            .await
            .expect("Waiter never finished")
            .unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn publishes_each_update_to_its_waiters() {
        let test = TestBus::new();
        let first = test.wait_finalized(1);
        let second = test.wait_finalized(1);
        let unrelated = test.wait_finalized(2);
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 2);

        test.wallet.update(payment(1, 10, PaymentStatus::Pending));
        next_poll().await;
        assert_eq!(test.published_status(1), Some(PaymentStatus::Pending));
        assert_eq!(test.published_status(2), None);
        assert!(!first.is_finished());

        test.wallet.update(payment(1, 20, PaymentStatus::Completed));
        for waiter in [first, second] {
            let payment = finish(waiter).await;
            assert_eq!(payment.status, PaymentStatus::Completed);
        }
        assert!(!unrelated.is_finished());
    }

    #[tokio::test(start_paused = true)]
    async fn waiters_share_one_poll_loop() {
        let test = TestBus::new();
        let start = Instant::now();
        let _waiters =
            (1..=5).map(|n| test.wait_finalized(n)).collect::<Vec<_>>();
        tokio::time::sleep(Duration::from_secs(30)).await;

        // One backoff schedule, not five. The waiters arriving during the
        // first poll trigger more polls at `start`, so dedup them.
        let mut times = test.wallet.poll_times();
        times.dedup();
        assert_eq!(times, poll_schedule(start, Instant::now()));
    }

    #[tokio::test(start_paused = true)]
    async fn stops_polling_once_nobody_waits() {
        let test = TestBus::new();
        let first = test.wait_finalized(1);
        let second = test.wait_finalized(2);
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 2);

        first.abort();
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 1);
        second.abort();
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 0);

        let num_polls = test.wallet.poll_times().len();
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert_eq!(test.wallet.poll_times().len(), num_polls);
    }

    #[tokio::test(start_paused = true)]
    async fn an_aborted_waiter_keeps_the_others_subscription() {
        let test = TestBus::new();
        let first = test.wait_finalized(1);
        let second = test.wait_finalized(1);
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 1);

        first.abort();
        next_poll().await;
        assert_eq!(test.num_subscriptions(), 1);

        test.wallet.update(payment(1, 10, PaymentStatus::Completed));
        let payment = finish(second).await;
        assert_eq!(payment.status, PaymentStatus::Completed);
        assert_eq!(test.num_subscriptions(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn starts_the_cursor_at_the_latest_update() {
        let test = TestBus::new();
        let history = payment(1, 10, PaymentStatus::Completed);
        let latest = history.updated_index();
        test.wallet.update(history);

        let _waiter = test.wait_finalized(2);
        next_poll().await;

        // The first poll reads only what came after the bus was created.
        assert_eq!(test.wallet.polls.lock().unwrap()[0].1, Some(latest));
    }

    #[tokio::test(start_paused = true)]
    async fn resumes_from_the_cursor_after_idling() {
        let test = TestBus::new();
        let waiter = test.wait_finalized(1);
        next_poll().await;
        let first = payment(1, 10, PaymentStatus::Completed);
        let first_index = first.updated_index();
        test.wallet.update(first);
        finish(waiter).await;

        // Nobody waits for a while, then a new waiter resumes tailing.
        tokio::time::sleep(Duration::from_secs(30)).await;
        let num_polls = test.wallet.poll_times().len();
        let waiter = test.wait_finalized(2);
        next_poll().await;
        test.wallet.update(payment(2, 20, PaymentStatus::Completed));
        finish(waiter).await;

        let resumed_at = test.wallet.polls.lock().unwrap()[num_polls].1;
        assert_eq!(resumed_at, Some(first_index));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_polling_backs_off_until_something_changes() {
        let test = TestBus::new();
        let start = Instant::now();
        let _waiter = test.wait_finalized(1);

        // Polls back off while nothing changes.
        tokio::time::sleep(Duration::from_secs(30)).await;
        let times = test.wallet.poll_times();
        assert_eq!(times, poll_schedule(start, Instant::now()));

        // A new waiter is polled for immediately, and restarts the backoff.
        let subscribed_at = Instant::now();
        let _other = test.wait_finalized(2);
        tokio::time::sleep(Duration::from_secs(30)).await;
        let times = test.wallet.poll_times();
        let expected = poll_schedule(subscribed_at, Instant::now());
        assert_eq!(times[times.len() - expected.len()..], expected);

        // An update restarts the backoff from the poll that saw it.
        test.wallet.update(payment(3, 10, PaymentStatus::Pending));
        let num_polls = test.wallet.poll_times().len();
        tokio::time::sleep(Duration::from_secs(5)).await;
        let times = test.wallet.poll_times();
        let saw_update = times[num_polls];
        let expected = poll_schedule(saw_update, Instant::now());
        assert_eq!(times[num_polls..], expected);
    }

    #[tokio::test(start_paused = true)]
    async fn poll_failures_do_not_stop_the_bus() {
        let test = TestBus::new();
        let waiter = test.wait_finalized(1);
        next_poll().await;

        test.wallet.failures.store(3, Ordering::Relaxed);
        test.wallet.update(payment(1, 10, PaymentStatus::Completed));
        let payment = finish(waiter).await;
        assert_eq!(payment.status, PaymentStatus::Completed);
        assert_eq!(test.wallet.failures.load(Ordering::Relaxed), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn returns_without_polling_if_already_satisfied() {
        let test = TestBus::new();
        test.wallet.update(payment(1, 10, PaymentStatus::Completed));

        let payment = finish(test.wait_finalized(1)).await;
        assert_eq!(payment.status, PaymentStatus::Completed);
        assert!(test.wallet.poll_times().is_empty());
        assert_eq!(test.num_subscriptions(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn catches_an_update_right_after_the_initial_read() {
        let test = TestBus::new();
        test.wallet.update(payment(1, 10, PaymentStatus::Pending));
        *test.wallet.update_after_read.lock().unwrap() =
            Some(payment(1, 20, PaymentStatus::Completed));

        let payment = finish(test.wait_finalized(1)).await;
        assert_eq!(payment.status, PaymentStatus::Completed);
    }

    /// A `None` cursor on a fresh wallet is a real position, so the wallet's
    /// first payment is published rather than skipped.
    #[tokio::test(start_paused = true)]
    async fn publishes_the_first_payment_of_a_fresh_wallet() {
        let test = TestBus::new();
        let first = test.wait_finalized(1);
        // Mid-way through the first waiter's poll delay.
        tokio::time::sleep(Duration::from_millis(
            PaymentsBus::INITIAL_WAIT_MS / 2,
        ))
        .await;

        // The first payment is created right after the next poll, which the
        // second waiter's arrival triggers.
        *test.wallet.update_after_poll.lock().unwrap() =
            Some(payment(1, 10, PaymentStatus::Completed));
        let _second = test.wait_finalized(2);

        let payment = finish(first).await;
        assert_eq!(payment.status, PaymentStatus::Completed);
    }

    #[tokio::test(start_paused = true)]
    async fn waits_for_the_next_update_after_an_index() {
        let test = TestBus::new();
        let old = payment(1, 10, PaymentStatus::Completed);
        let start_index = Some(old.updated_index());
        test.wallet.update(old);

        let waiter = test.wait_for_next_update(start_index);
        next_poll().await;
        assert!(!waiter.is_finished());

        test.wallet.update(payment(2, 20, PaymentStatus::Pending));
        finish(waiter).await;
    }
}
