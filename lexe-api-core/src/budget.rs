//! Spending budgets for clients.
//!
//! A [`Budget`] is a ledger of a client's payments, checked against a
//! [`BudgetPolicy`]. It is not the source of truth for payment state: the
//! payments manager owns every budget and updates it in lockstep with the
//! payments it tracks, under the payments lock, so this module holds only the
//! ledger and its pure operations.
//!
//! [`Budget`]: crate::budget::Budget
//! [`BudgetPolicy`]: crate::budget::BudgetPolicy

use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Display},
    num::NonZeroU8,
    ops::{Bound, RangeBounds},
    str::FromStr,
};

use anyhow::{Context, anyhow, ensure};
use chrono::{
    Datelike, FixedOffset, Months, NaiveDate, NaiveTime, TimeDelta, TimeZone,
    Utc,
};
#[cfg(any(test, feature = "test-utils"))]
use lexe_common::test_utils::arbitrary;
use lexe_common::{
    api::fiat_rates::{FiatRates, IsoCurrencyCode},
    ln::amount::Amount,
    time::TimestampMs,
};
use lexe_crypto::ed25519;
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use tracing::warn;

use self::models::{
    BudgetCurrency, BudgetInfo, BudgetParams, BudgetResetInterval,
};
use crate::{
    models::nwc::NostrPk,
    types::{payments::PaymentCreatedIndex, retries::Retries},
    vfs::{BUDGETS_DIR, Vfs, VfsDirectory, VfsFile, VfsFileId},
};

/// Request and response types for client budgets.
pub mod models;

/// A collection of client budgets, keyed by client public key.
///
/// Persisted to the VFS under the [`BUDGETS_DIR`] directory. Each budget is
/// stored as a separate file, using [`ClientPk`] as the filename.
#[derive(Clone, Debug)]
pub struct ClientBudgets(HashMap<ClientPk, Budget>);

/// An NWC or revocable client public key, serialized as `<prefix>_<pk>`.
///
/// NWC prefix: `n`. Revocable client prefix: `r`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(SerializeDisplay, DeserializeFromStr)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum ClientPk {
    Revocable(ed25519::PublicKey),
    Nwc(NostrPk),
}

/// A spending budget enforced for a client.
///
/// The budget is configured using [`BudgetPolicy`].
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
pub struct Budget {
    /// The policy this budget enforces. [`None`] means no spending limit.
    //
    // Expect a `policy` field on deserialization, so that a missing `policy`
    // field isn't interpreted as "no spending limit".
    #[serde(deserialize_with = "Option::deserialize")]
    policy: Option<BudgetPolicy>,

    /// The fiat currency a [`BudgetAmount::Fiat`] limit is denominated in.
    /// Fixed at budget creation.
    // TODO(nicole): store fiat rates historically and re-populate on change
    fiat_currency: IsoCurrencyCode,

    /// A map of all payments sent under this budget.
    payments: BTreeMap<PaymentCreatedIndex, BudgetPayment>,
}

/// A payment counted against the budget.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq))]
struct BudgetPayment {
    /// The amount sent plus fees.
    total: Amount,
    /// The Bitcoin price quoted in the budget's fiat currency when the
    /// payment was registered.
    btc_price: Decimal,
    status: BudgetPaymentStatus,
}

/// A budgeted payment's status. All but [`Failed`] payments count against the
/// limit.
///
/// [`Failed`]: Self::Failed
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetPaymentStatus {
    /// The payment hasn't finalized, so its total may still change.
    Pending,
    /// The payment completed.
    Completed,
    /// The payment failed, but its signed onchain tx can still confirm.
    SpendableFailed,
    /// The payment failed and can never complete.
    Failed,
}

/// The types of budgets that can be configured for a client.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum BudgetPolicy {
    /// The configuration for a "total" budget, which enforces a single spending
    /// limit across all of the client's payments.
    Total {
        /// The maximum total value the client may spend, including fees.
        limit: BudgetAmount,
    },
    /// The configuration for a budget whose spending resets on a regular
    /// schedule, e.g. on the 15th of every month.
    Windowed {
        /// The maximum value the client may spend between resets, including
        /// fees.
        limit: BudgetAmount,
        /// The calendar unit in which the time between resets is measured.
        unit: CalendarUnit,
        /// The number of `unit`s between resets, e.g. 1 month or 14 days.
        n: NonZeroU8,
        /// A reset time. Other resets occur every `n` `unit`s before and
        /// after it, e.g. every month on the 15th at 13:30.
        ///
        /// Only fixed UTC offsets are supported, since regional timezones
        /// would need the heavy `chrono_tz` dependency.
        ///
        /// Serialized using RFC 3339, so the UTC offset must use whole
        /// minutes.
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_fixed_offset_datetime()")
        )]
        reset_anchor: chrono::DateTime<FixedOffset>,
    },
}

/// A bitcoin or fiat amount used to describe a budget.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum BudgetAmount {
    /// A bitcoin amount.
    Bitcoin(Amount),
    /// A fiat amount. The currency used is determined by the budget's
    /// associated fiat currency.
    Fiat(
        #[cfg_attr(
            any(test, feature = "test-utils"),
            proptest(strategy = "arbitrary::any_nonneg_decimal()")
        )]
        Decimal,
    ),
}

/// A unit of calendar time.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum CalendarUnit {
    Day,
    /// Starts on Sunday.
    Week,
    Month,
    Year,
}

/// Annotates that a client's [`Budget`] was successfully persisted.
/// [`PersistedBudget`]s should be committed with [`ClientBudgets::commit`].
#[must_use]
pub struct PersistedBudget {
    pub budget: Budget,
    pub client_pk: ClientPk,
}

/// Look up a client's [`BudgetPolicy`].
//
// Required only to decouple the shared `VerifiedClientAuthorization` from the
// `PaymentsManager` which owns client budgets.
pub trait GetClientBudgetPolicy {
    /// The client's budget policy. [`None`] means no spending limit.
    fn budget_policy(
        &self,
        client_pk: &ClientPk,
    ) -> impl Future<Output = anyhow::Result<Option<BudgetPolicy>>> + Send;
}

impl ClientBudgets {
    #[cfg(any(test, feature = "test-utils"))]
    pub fn empty() -> Self {
        Self(HashMap::new())
    }

    /// Read all client budgets from the VFS, returning an empty collection if
    /// the VFS does not contain any budgets yet.
    pub async fn read(vfs: &(impl Vfs + Send + Sync)) -> anyhow::Result<Self> {
        let dir = VfsDirectory::new(BUDGETS_DIR);
        let client_budgets = vfs
            .read_dir_json::<Budget>(&dir)
            .await?
            .into_iter()
            .map(|(file_id, budget)| {
                let client_pk = ClientPk::from_str(&file_id.filename)
                    .with_context(|| file_id.filename.clone())
                    .context("Couldn't parse client pk from filename")?;
                Ok((client_pk, budget))
            })
            .collect::<anyhow::Result<_>>()
            .context("Error while reading client budgets")?;
        Ok(Self(client_budgets))
    }

    /// Get a read-only reference to a client's budget, if it exists.
    ///
    /// [`get_mut`](HashMap::get_mut) is disallowed. Instead, [`clone`](Clone)
    /// the budget, mutate the clone, [`persist`](Self::persist) it, then
    /// [`commit`](Self::commit) it.
    pub fn get(&self, client_pk: &ClientPk) -> Option<&Budget> {
        self.0.get(client_pk)
    }

    /// Iterate over every client's budget.
    pub fn iter(&self) -> impl Iterator<Item = (&ClientPk, &Budget)> {
        self.0.iter()
    }

    /// Persist a client's budget to the VFS. Does not update memory; call
    /// [`commit`](Self::commit) once everything the update depends on has
    /// been persisted too.
    pub async fn persist(
        vfs: &(impl Vfs + Send + Sync + ?Sized),
        client_pk: ClientPk,
        budget: Budget,
    ) -> anyhow::Result<PersistedBudget> {
        let file = Self::encrypt(vfs, client_pk, &budget);
        let retries = Retries::from_count(0);
        vfs.persist_file(file, retries).await?;
        Ok(PersistedBudget { budget, client_pk })
    }

    /// Encrypt a client's budget into its [`VfsFile`] without persisting it.
    /// Useful if a budget needs to be persisted alongside other files
    /// atomically.
    pub fn encrypt(
        vfs: &(impl Vfs + ?Sized),
        client_pk: ClientPk,
        budget: &Budget,
    ) -> VfsFile {
        vfs.encrypt_json(Self::file_id(&client_pk), budget)
    }

    /// The [`VfsFileId`] of a client's budget.
    pub fn file_id(client_pk: &ClientPk) -> VfsFileId {
        VfsFileId::new(BUDGETS_DIR, client_pk.to_string())
    }

    /// Commit a persisted budget to memory.
    pub fn commit(&mut self, persisted: PersistedBudget) {
        let PersistedBudget { budget, client_pk } = persisted;
        self.0.insert(client_pk, budget);
    }

    /// Remove a client's budget from memory, once its file has been deleted.
    pub fn commit_removal(&mut self, client_pk: &ClientPk) {
        self.0.remove(client_pk);
    }
}

impl Budget {
    /// Create a fresh budget from a policy, with no spending recorded yet.
    pub fn new(
        policy: Option<BudgetPolicy>,
        fiat_currency: IsoCurrencyCode,
    ) -> anyhow::Result<Self> {
        if let Some(policy) = &policy {
            policy.validate()?;
        }
        Ok(Self {
            policy,
            fiat_currency,
            payments: BTreeMap::new(),
        })
    }

    /// A snapshot of this budget as of `at`. [`None`] means no spending
    /// limit.
    pub fn info(&self, at: TimestampMs) -> Option<BudgetInfo> {
        let policy = self.policy?;

        // `BudgetInfo` reports the policy using `BudgetParams`'s fields.
        // Destructure to get a compile error when a new param is added,
        // so we can add it to `BudgetInfo`.
        let BudgetParams {
            limit,
            currency,
            reset_interval,
            interval_count,
            utc_offset_mins,
        } = policy.params(self.fiat_currency);
        let (spendable, _) = helpers::flatten_budget_amount(
            self.spendable(at)?,
            self.fiat_currency,
        );
        let next_reset = match policy {
            BudgetPolicy::Total { .. } => None,
            BudgetPolicy::Windowed {
                unit,
                n,
                reset_anchor,
                ..
            } => helpers::window_containing(at, unit, n, reset_anchor).1,
        };

        // A policy always has a limit and currency.
        Some(BudgetInfo {
            limit: limit?,
            currency: currency?,
            reset_interval,
            interval_count,
            utc_offset_mins,
            spendable,
            next_reset,
        })
    }

    /// The amount spendable as of `at`, in the policy's denomination.
    ///
    /// [`None`] if the budget has no policy, i.e. no spending limit.
    ///
    /// Counts every payment against the budget except
    /// [`BudgetPaymentStatus::Failed`] ones.
    pub fn spendable(&self, at: TimestampMs) -> Option<BudgetAmount> {
        let policy = self.policy?;
        let budgeted_payments = self
            .payments
            .iter()
            .filter(|(_, bp)| bp.status != BudgetPaymentStatus::Failed);

        // Get all the payments that count against the same limit that a
        // payment created at `at` would count against, then
        // `calculate_spendable` by subtracting the payment amounts against
        // limit.
        match policy {
            BudgetPolicy::Total { limit } => {
                let payments = budgeted_payments.map(|(_, bp)| bp);
                let spendable = helpers::calculate_spendable(limit, payments);
                Some(spendable)
            }
            BudgetPolicy::Windowed {
                limit,
                unit,
                n,
                reset_anchor,
            } => {
                let (start, end) =
                    helpers::window_containing(at, unit, n, reset_anchor);
                let window = (
                    start.map_or(Bound::Unbounded, Bound::Included),
                    end.map_or(Bound::Unbounded, Bound::Excluded),
                );
                let in_window = budgeted_payments
                    .filter(|(index, _)| window.contains(&index.created_at))
                    .map(|(_, bp)| bp);
                let spendable = helpers::calculate_spendable(limit, in_window);
                Some(spendable)
            }
        }
    }

    /// Get this budget's policy. `None` means unbounded spending policy.
    pub fn policy(&self) -> Option<BudgetPolicy> {
        self.policy
    }

    /// The currency a [`BudgetAmount::Fiat`] limit is denominated in.
    pub fn fiat_currency(&self) -> IsoCurrencyCode {
        self.fiat_currency
    }

    /// The Bitcoin price quoted in this budget's fiat currency, e.g. `80000`
    /// for 80,000 EUR per BTC, looked up in `fiat_rates`.
    pub fn btc_price(&self, fiat_rates: &FiatRates) -> anyhow::Result<Decimal> {
        let currency = self.fiat_currency;
        let price = fiat_rates
            .rates
            .get(&currency)
            .with_context(|| format!("No BTC/{currency} rate"))?;
        Decimal::try_from(price.0)
            .map_err(|_| anyhow!("Invalid BTC/{currency} rate"))
    }

    /// Check `total`, a payment's amount plus fees, against the
    /// [`Self::spendable`] as of `at`.
    ///
    /// `btc_price` is the Bitcoin price quoted in the budget's fiat currency,
    /// e.g. `80000` for 80,000 EUR per BTC.
    pub fn check_spendable(
        &self,
        total: Amount,
        btc_price: Decimal,
        at: TimestampMs,
    ) -> anyhow::Result<()> {
        let Some(spendable) = self.spendable(at) else {
            return Ok(());
        };

        match spendable {
            BudgetAmount::Bitcoin(spendable) => ensure!(
                total <= spendable,
                "Payment amount with fees ({total} sats) exceeds \
                 the remaining budget of {spendable} sats"
            ),
            BudgetAmount::Fiat(spendable) => {
                let total_fiat = total.btc() * btc_price;
                let currency = self.fiat_currency;
                ensure!(
                    total_fiat <= spendable,
                    "Payment amount with fees ({total_fiat} {currency}) \
                     exceeds the remaining budget of {spendable} {currency}"
                )
            }
        }

        Ok(())
    }

    /// Reconfigure a budget to a new [`BudgetPolicy`], preserving payment
    /// history.
    //
    // NOTE: Each payment's `btc_price` must be quoted in the budget's fiat
    // currency. We don't yet have historical fiat rates, so we disallow
    // changing the fiat currency.
    pub fn reconfigure(
        &mut self,
        new: Option<BudgetPolicy>,
    ) -> anyhow::Result<()> {
        if let Some(policy) = &new {
            policy.validate()?;
        }
        self.policy = new;
        Ok(())
    }

    /// Register a new payment costing `total`, its amount plus fees, valued
    /// at `fiat_rates`. If the total fits the spendable amount as of the
    /// payment's creation, it counts against the budget until the next
    /// reset.
    ///
    /// Fails if the payment is already registered.
    pub fn register_new_payment(
        &mut self,
        index: PaymentCreatedIndex,
        total: Amount,
        fiat_rates: &FiatRates,
    ) -> anyhow::Result<()> {
        ensure!(
            !self.payments.contains_key(&index),
            "Payment {index} is already registered in the budget"
        );

        let btc_price = self.btc_price(fiat_rates)?;
        self.check_spendable(total, btc_price, index.created_at)?;

        let payment = BudgetPayment {
            total,
            btc_price,
            status: BudgetPaymentStatus::Pending,
        };
        self.payments.insert(index, payment);
        Ok(())
    }

    /// Update a registered, pending payment's `total`, e.g. after re-routing
    /// it for a retry.
    ///
    /// Fails, leaving the budget untouched, if the payment isn't registered
    /// and pending, or if it would no longer fit; the caller must not send it.
    pub fn update_pending_payment(
        &mut self,
        index: PaymentCreatedIndex,
        total: Amount,
    ) -> anyhow::Result<()> {
        let current = *self
            .payments
            .get(&index)
            .with_context(|| format!("Budget does not track {index}"))?;
        ensure!(
            current.status == BudgetPaymentStatus::Pending,
            "Payment {index} is already finalized"
        );

        // The spendable amount already accounts for the current total, so only
        // the increase needs to fit.
        let total_increase = total.saturating_sub(current.total);
        if total_increase > Amount::ZERO {
            self.check_spendable(
                total_increase,
                current.btc_price,
                index.created_at,
            )
            .context("The payment's increased total no longer fits")?;
        }

        self.payments
            .insert(index, BudgetPayment { total, ..current });
        Ok(())
    }

    /// Finalize a payment in the budget, updating its status to one of:
    /// - [`BudgetPaymentStatus::Completed`]
    /// - [`BudgetPaymentStatus::SpendableFailed`]
    /// - [`BudgetPaymentStatus::Failed`], for which the payment no longer
    ///   counts against the limit
    ///
    /// Fails if the budget doesn't track the payment, or if `finalized_status`
    /// is [`BudgetPaymentStatus::Pending`]. Idempotent.
    pub fn finalize_payment(
        &mut self,
        index: PaymentCreatedIndex,
        total: Amount,
        finalized_status: BudgetPaymentStatus,
    ) -> anyhow::Result<()> {
        ensure!(
            finalized_status != BudgetPaymentStatus::Pending,
            "Tried to finalize payment {index}, but its status is pending"
        );
        let payment = self
            .payments
            .get_mut(&index)
            .with_context(|| format!("Budget does not track {index}"))?;
        if payment.total != total {
            // The payments manager registers an outbound offer payment before
            // LDK routes the payment, so the final fees may differ from those
            // the budget recorded. Thus, we expect this warning to fire for
            // outbound offer payments.
            // TODO(nicole): Handle BOLT 12 invoices manually, so the preflight
            // can report the fees accurately, and we can error on a mismatch.
            let id = &index.id;
            let recorded = payment.total;
            warn!(
                "Budgeted payment {id} total changed without a budget check: \
                 {recorded} -> {total}"
            );
        }
        *payment = BudgetPayment {
            total,
            status: finalized_status,
            ..*payment
        };
        Ok(())
    }
}

impl BudgetPayment {
    /// The payment total denominated in the budget's fiat currency.
    fn total_fiat(&self) -> Decimal {
        self.total.btc() * self.btc_price
    }
}

impl BudgetPolicy {
    /// Errors if the policy is invalid.
    fn validate(&self) -> anyhow::Result<()> {
        let Self::Windowed { reset_anchor, .. } = self else {
            return Ok(());
        };

        // Serialization drops any seconds from the UTC offset.
        let offset_secs = reset_anchor.offset().local_minus_utc();
        ensure!(
            offset_secs % 60 == 0,
            "reset_anchor's UTC offset isn't in whole minutes: {offset_secs}s"
        );
        Ok(())
    }

    /// This policy as [`BudgetParams`], given the budget's `fiat_currency`.
    /// Inverse of [`BudgetParams::policy`].
    fn params(self, fiat_currency: IsoCurrencyCode) -> BudgetParams {
        match self {
            Self::Total { limit } => {
                let (limit, currency) =
                    helpers::flatten_budget_amount(limit, fiat_currency);
                BudgetParams {
                    limit: Some(limit),
                    currency: Some(currency),
                    reset_interval: None,
                    interval_count: None,
                    utc_offset_mins: None,
                }
            }
            Self::Windowed {
                limit,
                unit,
                n,
                reset_anchor,
            } => {
                let (limit, currency) =
                    helpers::flatten_budget_amount(limit, fiat_currency);
                BudgetParams {
                    limit: Some(limit),
                    currency: Some(currency),
                    reset_interval: Some(BudgetResetInterval::from(unit)),
                    interval_count: Some(n),
                    utc_offset_mins: Some(
                        reset_anchor.offset().local_minus_utc() / 60,
                    ),
                }
            }
        }
    }
}

impl CalendarUnit {
    /// The next unit start after `at`, in `at`'s UTC offset.
    fn next_start(
        self,
        at: chrono::DateTime<FixedOffset>,
    ) -> Option<chrono::DateTime<FixedOffset>> {
        let next_index = self.index(at.date_naive()).checked_add(1)?;
        let start = self.first_day(next_index)?.and_time(NaiveTime::MIN);
        at.timezone().from_local_datetime(&start).single()
    }

    /// `at` plus `count` units, which may be negative. A month or year which
    /// lands past the end of a shorter month clamps to its last day, e.g.
    /// Jan 31 plus a month is Feb 28.
    fn add(
        self,
        count: i32,
        at: chrono::DateTime<FixedOffset>,
    ) -> Option<chrono::DateTime<FixedOffset>> {
        let add_months = |months: i32| {
            let abs_months = Months::new(months.unsigned_abs());
            if months >= 0 {
                at.checked_add_months(abs_months)
            } else {
                at.checked_sub_months(abs_months)
            }
        };
        match self {
            Self::Day =>
                at.checked_add_signed(TimeDelta::try_days(i64::from(count))?),
            Self::Week =>
                at.checked_add_signed(TimeDelta::try_weeks(i64::from(count))?),
            Self::Month => add_months(count),
            Self::Year => add_months(count.checked_mul(12)?),
        }
    }

    /// Numbers calendar units consecutively from an arbitrary origin, and
    /// finds the index of the unit containing `date`.
    ///
    /// Indices are under 10^8 in magnitude: day indices, the largest, span
    /// [`NaiveDate::MIN`] to [`NaiveDate::MAX`], about ±9.6 × 10^7 days.
    fn index(self, date: NaiveDate) -> i32 {
        match self {
            // We count Jan 1, 1 as the day at index 1.
            Self::Day => date.num_days_from_ce(),
            // We count Dec 31, 0 to Jan 6, 1 inclusive as the week at index 0.
            // Day 1 CE is a Monday.
            Self::Week => date.num_days_from_ce().div_euclid(7),
            // We count January of year 0 as the month at index 0.
            Self::Month => date.year() * 12 + date.month0() as i32,
            // We count year 0 as the year at index 0.
            Self::Year => date.year(),
        }
    }

    /// The date of the start of the calendar unit numbered `index`, per
    /// [`Self::index`].
    fn first_day(self, index: i32) -> Option<NaiveDate> {
        match self {
            Self::Day => NaiveDate::from_num_days_from_ce_opt(index),
            Self::Week =>
                NaiveDate::from_num_days_from_ce_opt(index.checked_mul(7)?),
            Self::Month => {
                let year = index.div_euclid(12);
                let month = index.rem_euclid(12) as u32 + 1;
                NaiveDate::from_ymd_opt(year, month, 1)
            }
            Self::Year => NaiveDate::from_ymd_opt(index, 1, 1),
        }
    }
}

// --- impl ClientPk --- //

impl ClientPk {
    const PREFIX_REVOCABLE: &'static str = "r";
    const PREFIX_NWC: &'static str = "n";

    fn prefix(&self) -> &'static str {
        match self {
            ClientPk::Revocable(_) => Self::PREFIX_REVOCABLE,
            ClientPk::Nwc(_) => Self::PREFIX_NWC,
        }
    }
}

/// `<prefix>_<pk>`
impl Display for ClientPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = self.prefix();
        match self {
            ClientPk::Revocable(pk) => write!(f, "{prefix}_{pk}"),
            ClientPk::Nwc(pk) => write!(f, "{prefix}_{pk}"),
        }
    }
}

/// `<prefix>_<pk>`
impl FromStr for ClientPk {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (prefix, pk_str) = s.split_once('_').ok_or_else(|| {
            anyhow!("Expected ClientPk in the format <prefix>_<pk>, got: {s}")
        })?;
        match prefix {
            Self::PREFIX_REVOCABLE => {
                let pk = ed25519::PublicKey::from_str(pk_str)?;
                Ok(ClientPk::Revocable(pk))
            }
            Self::PREFIX_NWC => {
                let pk = NostrPk::from_str(pk_str)?;
                Ok(ClientPk::Nwc(pk))
            }
            _ => Err(anyhow!("Unknown ClientPk prefix: {prefix}")),
        }
    }
}

// --- helpers --- //

mod helpers {
    use super::*;

    /// Returns the `[start, end)` of the [`BudgetPolicy::Windowed`] window
    /// spanning `n` `unit`s and containing `at`, where `start` is a whole
    /// number of windows from `reset_anchor`, in the same UTC offset.
    ///
    /// February has no 31st, so a Jan 31 monthly anchor starts a window on
    /// Feb 28. Each window start is computed from the anchor rather than the
    /// previous start, so the next is Mar 31, not Mar 28.
    ///
    /// A bound is [`None`] if it can't be computed or represented, e.g. a
    /// `start` before the epoch.
    pub(super) fn window_containing(
        at: TimestampMs,
        unit: CalendarUnit,
        n: NonZeroU8,
        reset_anchor: chrono::DateTime<FixedOffset>,
    ) -> (Option<TimestampMs>, Option<TimestampMs>) {
        let n = i32::from(n.get());
        let maybe_at =
            chrono::DateTime::<Utc>::from_timestamp_millis(at.to_i64());
        let at = match maybe_at {
            Some(at) => at.with_timezone(&reset_anchor.timezone()),
            None => return (None, None),
        };

        // Suppose `unit` is a month and the anchor is Jan 15, 2026, 13:30.
        // Then a window starts every month on the 15th at 13:30, or if
        // `n > 1`, every `n`th month before or after Jan 2026.

        // First find the start of `at`'s window as if `n` were 1, in whole
        // units from the anchor.
        let anchor_index = unit.index(reset_anchor.date_naive());
        // Indices are under 10^8 in magnitude, so this can't overflow.
        let mut units_from_anchor = unit.index(at.date_naive()) - anchor_index;
        if let Some(anchor_offset_in_same_unit) =
            unit.add(units_from_anchor, reset_anchor)
            && at < anchor_offset_in_same_unit
        {
            units_from_anchor -= 1;
        }

        // Then count back to the latest window start for the true `n`, which
        // is a multiple of `n` units from the anchor. The window ends at the
        // next multiple.
        let window_start_units =
            units_from_anchor - units_from_anchor.rem_euclid(n);
        let window_end_units = window_start_units + n;
        let to_timestamp = |units: i32| {
            let dt = unit.add(units, reset_anchor)?;
            TimestampMs::try_from(dt.timestamp_millis()).ok()
        };
        (
            to_timestamp(window_start_units),
            to_timestamp(window_end_units),
        )
    }

    /// Given a set of payments to count against a limit, calculate the
    /// spendable amount in the same denomination as the limit.
    pub(super) fn calculate_spendable<'a>(
        limit: BudgetAmount,
        payments: impl Iterator<Item = &'a BudgetPayment>,
    ) -> BudgetAmount {
        match limit {
            BudgetAmount::Bitcoin(limit) => {
                let spent = payments
                    .fold(Amount::ZERO, |acc, bp| acc.saturating_add(bp.total));
                BudgetAmount::Bitcoin(limit.saturating_sub(spent))
            }
            BudgetAmount::Fiat(limit) => {
                let spent = payments.fold(Decimal::ZERO, |acc, bp| {
                    acc.saturating_add(bp.total_fiat())
                });
                BudgetAmount::Fiat(
                    limit.saturating_sub(spent).max(Decimal::ZERO),
                )
            }
        }
    }

    /// Flattens `amount` into its [`models`] representation: a [`Decimal`] with
    /// a [`BudgetCurrency`].
    ///
    /// [`models`]: crate::budget::models
    pub(super) fn flatten_budget_amount(
        amount: BudgetAmount,
        fiat_currency: IsoCurrencyCode,
    ) -> (Decimal, BudgetCurrency) {
        match amount {
            BudgetAmount::Bitcoin(amount) =>
                (amount.btc(), BudgetCurrency::Bitcoin),
            BudgetAmount::Fiat(value) =>
                (value, BudgetCurrency::Fiat(fiat_currency)),
        }
    }
}

#[cfg(test)]
mod test {
    use lexe_common::{
        api::fiat_rates::FiatBtcPrice,
        sat,
        test_utils::{roundtrip, snapshot},
    };
    use proptest::{prop_assert, prop_assert_eq, proptest};

    use super::*;
    use crate::types::payments::{ClientPaymentId, PaymentId};

    // -- Helpers -- //

    /// The index of onchain send `id`, created at `created_at_secs`.
    fn onchain_payment_index(
        created_at_secs: u32,
        id: u8,
    ) -> PaymentCreatedIndex {
        PaymentCreatedIndex {
            created_at: TimestampMs::from_secs_u32(created_at_secs),
            id: PaymentId::OnchainSend(ClientPaymentId([id; 32])),
        }
    }

    // -- Design guarantees -- //

    /// A fiat limit values each payment at the BTC price when it was
    /// registered, so a later price change doesn't revalue it.
    #[test]
    fn fiat_payments_keep_their_price() {
        let now = TimestampMs::from_secs_u32(1_000);
        let rates_at_usd_btc_price = |usd_btc_price: f64| FiatRates {
            timestamp_ms: now,
            rates: BTreeMap::from([(
                IsoCurrencyCode::USD,
                FiatBtcPrice(usd_btc_price),
            )]),
        };
        let policy = BudgetPolicy::Total {
            limit: BudgetAmount::Fiat(Decimal::ONE),
        };
        let usd = IsoCurrencyCode::USD;
        let mut budget = Budget::new(Some(policy), usd).unwrap();

        // 500 sats at $100k/BTC is $0.50.
        let rates = rates_at_usd_btc_price(100_000.0);
        budget
            .register_new_payment(
                onchain_payment_index(1, 1),
                sat!(500),
                &rates,
            )
            .unwrap();
        let unspent = BudgetAmount::Fiat(Decimal::new(50, 2));
        assert_eq!(budget.spendable(now), Some(unspent));

        // 200 sats at $200k/BTC is $0.40, while the first payment stays $0.50.
        let rates = rates_at_usd_btc_price(200_000.0);
        budget
            .register_new_payment(
                onchain_payment_index(2, 2),
                sat!(200),
                &rates,
            )
            .unwrap();
        let unspent = BudgetAmount::Fiat(Decimal::new(10, 2));
        assert_eq!(budget.spendable(now), Some(unspent));

        // 100 sats at $200k/BTC is $0.20, which no longer fits.
        let result = budget.register_new_payment(
            onchain_payment_index(3, 3),
            sat!(100),
            &rates,
        );
        assert!(result.is_err());
    }

    /// Since `Budget { policy: None, .. }` means "no spending limit", the
    /// serialization should distinguish between a missing `policy` field and a
    /// no-limit policy. Ensure the field is always written, and required on
    /// read.
    #[test]
    fn budget_json_requires_policy_field() {
        let budget = Budget::new(None, IsoCurrencyCode::USD).unwrap();
        let json = serde_json::to_value(&budget).unwrap();
        assert_eq!(json["policy"], serde_json::Value::Null);
        assert!(serde_json::from_value::<Budget>(json.clone()).is_ok());

        let mut missing = json;
        missing.as_object_mut().unwrap().remove("policy");
        assert!(serde_json::from_value::<Budget>(missing).is_err());
    }

    // -- Code correctness tests -- //

    /// Tests [`CalendarUnit::index`] and [`CalendarUnit::first_day`]:
    /// - Units are numbered consecutively.
    /// - Every date lies between the start of its unit and of the next.
    /// - Weeks start on Sunday, months on the 1st, and years on January 1.
    #[test]
    fn calendar_unit_index_first_day() {
        use CalendarUnit::*;

        // Spans about ±2,700 years, including years before the common era.
        proptest!(|(days in -1_000_000_i32..1_000_000)| {
            let date = NaiveDate::from_num_days_from_ce_opt(days).unwrap();
            for unit in [Day, Week, Month, Year] {
                // `date` lies in the unit starting at `first_day(index)`,
                // which ends where the next index's unit starts.
                let index = unit.index(date);
                let start = unit.first_day(index).unwrap();
                let next_start = unit.first_day(index + 1).unwrap();
                prop_assert!(start <= date && date < next_start);
                prop_assert_eq!(unit.index(start), index);
                prop_assert_eq!(unit.index(next_start), index + 1);

                // Each unit starts on its calendar boundary.
                let aligned = match unit {
                    Day => start == date,
                    Week => start.weekday() == chrono::Weekday::Sun,
                    Month => start.day() == 1,
                    Year => start.ordinal() == 1,
                };
                prop_assert!(aligned, "{unit:?} starts on {start}");
            }
        });
    }

    /// Tests [`helpers::window_containing`]:
    /// - A window includes its start but not its end.
    /// - Windows start every `n` units from the anchor, before it as well as
    ///   after it. So with a daily 06:30 anchor, 03:00 is in the window which
    ///   began at 06:30 the day before.
    /// - Windows follow the anchor's UTC offset.
    /// - Months and years follow the calendar. A reset past the end of a
    ///   shorter month clamps to its last day, without drifting, e.g. a Jan 31
    ///   monthly anchor resets on Feb 28, then Mar 31.
    #[test]
    fn window_containing() {
        use CalendarUnit::*;

        let ts = |rfc3339: &str| {
            let dt = chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap();
            TimestampMs::try_from(dt.timestamp_millis()).unwrap()
        };
        // Asserts that the window of `n` `unit`s anchored at `anchor` which
        // contains `at` is `[start, end)`.
        let assert_window =
            |at: &str,
             unit: CalendarUnit,
             n: u8,
             anchor: &str,
             [start, end]: [&str; 2]| {
                let n = NonZeroU8::new(n).unwrap();
                let anchor =
                    chrono::DateTime::parse_from_rfc3339(anchor).unwrap();
                let window =
                    helpers::window_containing(ts(at), unit, n, anchor);
                assert_eq!(
                    window,
                    (Some(ts(start)), Some(ts(end))),
                    "{unit:?} x{n} from {anchor}, at {at}"
                );
            };

        // A window includes its start but not its end, so a time on the
        // boundary between two windows is in the later one.
        let anchor = "2026-01-01T06:30:00Z";
        let window = ["2026-03-16T06:30:00Z", "2026-03-17T06:30:00Z"];
        assert_window("2026-03-16T06:30:00Z", Day, 1, anchor, window);
        // A time after midnight but before 06:30 is in the previous window.
        let window = ["2026-03-14T06:30:00Z", "2026-03-15T06:30:00Z"];
        assert_window("2026-03-15T03:00:00Z", Day, 1, anchor, window);

        // Windows follow the anchor's UTC offset.
        let anchor = "2026-01-01T06:30:00+09:00";
        let window = ["2026-01-02T06:30:00+09:00", "2026-01-03T06:30:00+09:00"];
        assert_window("2026-01-02T08:00:00+09:00", Day, 1, anchor, window);

        // A weekly Wednesday 09:15 anchor resets every Wednesday at 09:15.
        let anchor = "2026-01-07T09:15:00Z";
        let window = ["2026-01-14T09:15:00Z", "2026-01-21T09:15:00Z"];
        assert_window("2026-01-20T12:00:00Z", Week, 1, anchor, window);

        // A 2023-12-15 anchor every 5 months resets on ..., 2023-02-15,
        // 2023-07-15, 2023-12-15, 2024-05-15, 2024-10-15, ...
        let anchor = "2023-12-15T11:30:00Z";
        let window = ["2024-05-15T11:30:00Z", "2024-10-15T11:30:00Z"];
        assert_window("2024-06-20T15:24:00Z", Month, 5, anchor, window);
        let window = ["2023-12-15T11:30:00Z", "2024-05-15T11:30:00Z"];
        assert_window("2024-05-12T15:24:00Z", Month, 5, anchor, window);
        let window = ["2023-02-15T11:30:00Z", "2023-07-15T11:30:00Z"];
        assert_window("2023-05-20T15:24:00Z", Month, 5, anchor, window);

        // A Jan 31 anchor's February reset clamps to Feb 28. Each reset is
        // computed from the anchor, so the window starting from Feb 28 ends on
        // Mar 31, not Mar 28.
        let anchor = "2026-01-31T08:45:00Z";
        let window = ["2026-02-28T08:45:00Z", "2026-03-31T08:45:00Z"];
        assert_window("2026-02-28T12:00:00Z", Month, 1, anchor, window);
        // Counting back from a future anchor clamps the same way.
        let anchor = "2026-03-31T08:45:00Z";
        let window = ["2026-01-31T08:45:00Z", "2026-02-28T08:45:00Z"];
        assert_window("2026-02-15T00:00:00Z", Month, 1, anchor, window);

        // A Feb 29 anchor resets on Feb 28 in a common year.
        let anchor = "2028-02-29T12:05:00Z";
        let window = ["2029-02-28T12:05:00Z", "2030-02-28T12:05:00Z"];
        assert_window("2029-06-01T00:00:00Z", Year, 1, anchor, window);
    }

    /// Tests [`Budget::update_pending_payment`]:
    /// - A pending payment's total can rise only if it still fits the spendable
    ///   amount. Otherwise, the update is rejected, leaving the budget
    ///   untouched.
    /// - Lowering the total releases the difference.
    /// - A finalized payment's total can't be updated.
    #[test]
    fn updated_pending_payment_must_fit() {
        let rates = FiatRates::dummy();
        let now = TimestampMs::from_secs_u32(1_000);
        let policy = BudgetPolicy::Total {
            limit: BudgetAmount::Bitcoin(sat!(1_000)),
        };
        let fiat_currency = IsoCurrencyCode::USD;
        let mut budget = Budget::new(Some(policy), fiat_currency).unwrap();
        let payment = onchain_payment_index(1, 1);
        budget
            .register_new_payment(payment, sat!(400), &rates)
            .unwrap();

        budget.update_pending_payment(payment, sat!(500)).unwrap();
        let unspent = BudgetAmount::Bitcoin(sat!(500));
        assert_eq!(budget.spendable(now), Some(unspent));

        let before = budget.clone();
        let result = budget.update_pending_payment(payment, sat!(1_001));
        assert!(result.is_err());
        assert_eq!(budget, before);

        budget.update_pending_payment(payment, sat!(300)).unwrap();
        let unspent = BudgetAmount::Bitcoin(sat!(700));
        assert_eq!(budget.spendable(now), Some(unspent));

        let status = BudgetPaymentStatus::Completed;
        budget.finalize_payment(payment, sat!(300), status).unwrap();
        let result = budget.update_pending_payment(payment, sat!(200));
        assert!(result.is_err());
    }

    /// A windowed budget counts a payment only in the window it was created
    /// in. Once the window resets, the full limit is spendable again, while
    /// the payment stays tracked.
    #[test]
    fn windowed_budget_resets() {
        let ts = |rfc3339: &str| {
            let dt = chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap();
            TimestampMs::try_from(dt.timestamp_millis()).unwrap()
        };
        let rates = FiatRates::dummy();
        let limit = sat!(1_000);
        let policy = BudgetPolicy::Windowed {
            limit: BudgetAmount::Bitcoin(limit),
            unit: CalendarUnit::Day,
            n: NonZeroU8::MIN,
            reset_anchor: chrono::DateTime::parse_from_rfc3339(
                "2026-01-01T00:00:00+00:00",
            )
            .unwrap(),
        };
        let fiat_currency = IsoCurrencyCode::USD;
        let mut budget = Budget::new(Some(policy), fiat_currency).unwrap();

        // A payment at noon counts against that day's window.
        let payment = PaymentCreatedIndex {
            created_at: ts("2026-01-01T12:00:00+00:00"),
            id: PaymentId::OnchainSend(ClientPaymentId([1; 32])),
        };
        budget
            .register_new_payment(payment, sat!(600), &rates)
            .unwrap();
        let unspent = BudgetAmount::Bitcoin(sat!(400));
        assert_eq!(budget.spendable(payment.created_at), Some(unspent));

        // From the next midnight on, the full limit is spendable again.
        let next_day = ts("2026-01-02T00:00:00+00:00");
        let full = BudgetAmount::Bitcoin(limit);
        assert_eq!(budget.spendable(next_day), Some(full));

        // The payment is still tracked and pending, so its window still
        // counts it.
        budget.update_pending_payment(payment, sat!(600)).unwrap();
        assert_eq!(budget.spendable(payment.created_at), Some(unspent));
    }

    // -- Roundtrip tests -- //

    #[test]
    fn budget_policy_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<BudgetPolicy>();
    }

    #[test]
    fn client_pk_roundtrip() {
        roundtrip::json_string_roundtrip_proptest::<ClientPk>();
        roundtrip::fromstr_display_roundtrip_proptest::<ClientPk>();
    }

    // -- Serialization compat tests -- //

    #[test]
    fn budget_deser_compat() {
        let inputs = r#"
--- Windowed fiat limit, with a payment in each status
{"policy":{"windowed":{"limit":{"fiat":"20.00"},"unit":"month","n":1,"reset_anchor":"2026-01-01T00:00:00-08:00"}},"fiat_currency":"USD","payments":{"0000000000000001000-os_0101010101010101010101010101010101010101010101010101010101010101":{"total":"1000","btc_price":"100000","status":"pending"},"0000000000000002000-os_0202020202020202020202020202020202020202020202020202020202020202":{"total":"2000","btc_price":"100000","status":"completed"},"0000000000000003000-os_0303030303030303030303030303030303030303030303030303030303030303":{"total":"3000","btc_price":"100000","status":"spendable_failed"},"0000000000000004000-os_0404040404040404040404040404040404040404040404040404040404040404":{"total":"4000","btc_price":"100000","status":"failed"}}}
--- Total bitcoin limit
{"policy":{"total":{"limit":{"bitcoin":"1000"}}},"fiat_currency":"EUR","payments":{"0000000000000001000-os_0101010101010101010101010101010101010101010101010101010101010101":{"total":"500","btc_price":"62965.97545915064","status":"completed"}}}
--- No spending limit
{"policy":null,"fiat_currency":"USD","payments":{}}
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let value1: Budget = serde_json::from_str(input).unwrap();
            let output = serde_json::to_string(&value1).unwrap();
            let value2: Budget = serde_json::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }

    #[test]
    fn client_pk_deser_compat() {
        let inputs = r#"
--- revocable
r_003690453dac3e6c29db4e930c80f797fe0a05bc43ed2d9cff2a62fb7407d3e0
--- nwc
n_3045b3cf002d40b2ae2e2f0f4b3d657cad3d2d8995988fb78ce488d9ec7d8f30
"#;
        for input in snapshot::parse_sample_data(inputs) {
            let value1 = ClientPk::from_str(input).unwrap();
            let output = value1.to_string();
            let value2 = ClientPk::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }
}
