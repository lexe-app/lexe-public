//! Request and response types for client budgets.
//!
//! - [`BudgetParams`]: specifies a new client's [`BudgetPolicy`].
//! - [`UpdateBudgetParams`]: specifies changes to a client's policy.
//! - [`BudgetInfo`]: reports a client's [`Budget`]. Contains every
//!   [`BudgetParams`] field, plus derived fields such as `spendable` and
//!   `next_reset`.
//!
//! All are flat, which makes them easier to evolve compatibly.
//!
//! The [`BudgetParams`] fields depend on each other:
//!
//! ```text
//! limit + currency
//! └── reset_interval
//!     ├── interval_count  (required)
//!     └── utc_offset_mins (required)
//! ```
//!
//! - `limit` and `currency` are always set, changed, and cleared together.
//! - A child may be set only if its parent is.
//! - A required child must be set whenever its parent is.
//!
//! In an update, a required child must be updated whenever its parent is, and
//! every child must be cleared whenever its parent is. For example,
//! [`UpdateBudgetParams::CLEAR_BUDGET`] clears every param, rather than just
//! the `limit`.
//!
//! [`UpdateBudgetParams`]: crate::budget::models::UpdateBudgetParams
//! [`UpdateBudgetParams::CLEAR_BUDGET`]:
//!     crate::budget::models::UpdateBudgetParams::CLEAR_BUDGET

use std::{fmt, num::NonZeroU8, str::FromStr};

use anyhow::{Context, anyhow, bail, ensure};
use chrono::{FixedOffset, Utc};
#[cfg(any(test, feature = "test-utils"))]
use lexe_common::test_utils::arbitrary;
use lexe_common::{
    api::fiat_rates::IsoCurrencyCode, ln::amount::Amount, time::TimestampMs,
};
use lexe_serde::optopt::{self, none};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_with::DeserializeFromStr;

use super::{BudgetAmount, BudgetPolicy, CalendarUnit};

/// The parameters to configure a new client's [`BudgetPolicy`]. All fields
/// unset means no spending limit.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct BudgetParams {
    /// The client's spending limit, including fees, in `currency` (BTC, not
    /// sats, for bitcoin). The limit can reset at regular intervals, as set by
    /// `reset_interval`.
    ///
    /// Must be set together with `currency`.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "proptest::option::of(arbitrary::any_decimal())")
    )]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<Decimal>,
    /// The currency of `limit`.
    ///
    /// Must be set together with `limit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<BudgetCurrency>,
    /// How often the limit resets, e.g. `monthly`. Resets happen at the start
    /// of each day, week, month, or year. For example, a `weekly` limit
    /// resets on Sunday. Unset means the limit never resets.
    ///
    /// May be set only if `limit` is. If set, `interval_count` and
    /// `utc_offset_mins` must be set too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_interval: Option<BudgetResetInterval>,
    /// The number of days, weeks, months, or years between resets, whichever
    /// calendar unit `reset_interval` specifies.
    ///
    /// The next reset will start at the beginning of the next calendar unit.
    /// For example, a `weekly` budget with an `interval_count` of 2 resets
    /// every other week, starting next Sunday.
    ///
    /// May be set only if `reset_interval` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_count: Option<NonZeroU8>,
    /// The budget's timezone, as a UTC offset in minutes, e.g. -300 for
    /// UTC-05:00. Resets follow this timezone's calendar, since Monday in one
    /// timezone can already be Tuesday in another.
    ///
    /// May be set only if `reset_interval` is.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_offset_mins: Option<i32>,
}

/// Changes to a client's [`BudgetPolicy`]. For each field, [`None`] leaves it
/// unchanged and `Some(None)` clears it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct UpdateBudgetParams {
    /// Set or clear [`BudgetParams::limit`].
    ///
    /// Must be changed together with `currency`.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "proptest::option::of(proptest::option::of(\
                             arbitrary::any_decimal()))")
    )]
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub limit: Option<Option<Decimal>>,
    /// Set or clear [`BudgetParams::currency`].
    ///
    /// Must be changed together with `limit`.
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub currency: Option<Option<BudgetCurrency>>,
    /// Set or clear [`BudgetParams::reset_interval`]. Changing it restarts the
    /// schedule from the next calendar unit start.
    ///
    /// Changing it requires changing `interval_count` and `utc_offset_mins`
    /// too.
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub reset_interval: Option<Option<BudgetResetInterval>>,
    /// Set or clear [`BudgetParams::interval_count`]. Changing it restarts the
    /// schedule from the next calendar unit start.
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub interval_count: Option<Option<NonZeroU8>>,
    /// Set or clear [`BudgetParams::utc_offset_mins`]. Changing it restarts
    /// the schedule from the next calendar unit start.
    #[serde(default, skip_serializing_if = "none", with = "optopt")]
    pub utc_offset_mins: Option<Option<i32>>,
}

/// A snapshot of a client's budget.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub struct BudgetInfo {
    /// The client's spending limit, including fees, in `currency` (BTC, not
    /// sats, for bitcoin). The limit can reset at regular intervals, as set by
    /// `reset_interval`.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_decimal()")
    )]
    pub limit: Decimal,
    /// The currency of `limit` and `spendable`.
    pub currency: BudgetCurrency,
    /// How often the limit resets. [`None`] means it never resets.
    pub reset_interval: Option<BudgetResetInterval>,
    /// The number of days, weeks, months, or years between resets, whichever
    /// `reset_interval` specifies.
    ///
    /// Set together with `reset_interval`.
    pub interval_count: Option<NonZeroU8>,
    /// The budget's timezone, as a UTC offset in minutes.
    ///
    /// Set together with `reset_interval`.
    pub utc_offset_mins: Option<i32>,
    /// How much the client can still spend, as of the snapshot, in `currency`
    /// (BTC, not sats, for bitcoin).
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_decimal()")
    )]
    pub spendable: Decimal,
    /// When the limit next resets, as of the snapshot. [`None`] if it never
    /// resets.
    pub next_reset: Option<TimestampMs>,
}

/// The currency of a budget's limit: bitcoin, or a fiat currency.
///
/// Serialized as an ISO 4217 code, e.g. `"USD"`, with `"BTC"` for bitcoin.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[derive(DeserializeFromStr)]
pub enum BudgetCurrency {
    Bitcoin,
    /// Never [`IsoCurrencyCode::BTC`], which parses as [`Self::Bitcoin`].
    Fiat(IsoCurrencyCode),
}

/// How often a budget's limit resets.
//
// NOTE: This is exposed in the Rust SDK.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum BudgetResetInterval {
    Daily,
    /// Weeks start on Sunday.
    Weekly,
    Monthly,
    Yearly,
}

impl BudgetParams {
    /// Creates an unlimited budget, which doesn't enforce any spending limits.
    pub const UNLIMITED: Self = Self {
        limit: None,
        currency: None,
        reset_interval: None,
        interval_count: None,
        utc_offset_mins: None,
    };

    /// Creates a budget which can only spend up to a total of `limit`, across
    /// all payments ever made.
    pub const fn new_total_budget(
        limit: Decimal,
        currency: BudgetCurrency,
    ) -> Self {
        Self {
            limit: Some(limit),
            currency: Some(currency),
            reset_interval: None,
            interval_count: None,
            utc_offset_mins: None,
        }
    }

    /// The policy these params describe as of `now`, given the budget's
    /// `current_policy`. [`None`] means no spending limit.
    ///
    /// Errors if the params are invalid.
    pub fn policy(
        &self,
        current_policy: Option<&BudgetPolicy>,
        now: TimestampMs,
    ) -> anyhow::Result<Option<BudgetPolicy>> {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the policy.
        let &Self {
            limit: maybe_limit,
            currency: maybe_currency,
            reset_interval: maybe_reset_interval,
            interval_count: maybe_interval_count,
            utc_offset_mins: maybe_utc_offset_mins,
        } = self;

        let Some(limit) = maybe_limit else {
            let set_fields = helpers::join_if_true(&[
                ("currency", maybe_currency.is_some()),
                ("reset_interval", maybe_reset_interval.is_some()),
                ("interval_count", maybe_interval_count.is_some()),
                ("utc_offset_mins", maybe_utc_offset_mins.is_some()),
            ]);
            if let Some(fields) = set_fields {
                bail!("Must set limit when setting {fields}");
            }
            return Ok(None);
        };
        ensure!(
            !limit.is_sign_negative(),
            "limit can't be negative: {limit}"
        );
        let currency =
            maybe_currency.context("Must set currency when setting limit")?;
        let policy_limit = match currency {
            BudgetCurrency::Bitcoin => {
                let btc =
                    Amount::try_from_btc(limit).context("Invalid BTC limit")?;
                BudgetAmount::Bitcoin(btc)
            }
            BudgetCurrency::Fiat(_) => BudgetAmount::Fiat(limit),
        };

        let Some(reset_interval) = maybe_reset_interval else {
            let set_fields = helpers::join_if_true(&[
                ("interval_count", maybe_interval_count.is_some()),
                ("utc_offset_mins", maybe_utc_offset_mins.is_some()),
            ]);
            if let Some(fields) = set_fields {
                bail!("Must set reset_interval when setting {fields}");
            }
            return Ok(Some(BudgetPolicy::Total {
                limit: policy_limit,
            }));
        };
        let unit = CalendarUnit::from(reset_interval);
        let n = maybe_interval_count.context(
            "Must set interval_count when setting reset_interval, e.g. to 1",
        )?;
        let utc_offset_mins = maybe_utc_offset_mins.context(
            "Must set utc_offset_mins when setting reset_interval, e.g. to 0 \
             for UTC",
        )?;
        let utc_offset = utc_offset_mins
            .checked_mul(60)
            .and_then(FixedOffset::east_opt)
            .with_context(|| {
                format!(
                    "utc_offset_mins must be within ±1439 minutes, but got \
                     {utc_offset_mins}"
                )
            })?;

        // Keep the current reset anchor if the unit, interval count, and UTC
        // offset are unchanged. Otherwise, anchor at the next unit's start.
        let unchanged_anchor = match current_policy {
            Some(&BudgetPolicy::Windowed {
                unit: current_unit,
                n: current_n,
                reset_anchor,
                ..
            }) => {
                let is_unchanged = current_unit == unit
                    && current_n == n
                    && *reset_anchor.offset() == utc_offset;
                is_unchanged.then_some(reset_anchor)
            }
            _ => None,
        };
        let reset_anchor = match unchanged_anchor {
            Some(reset_anchor) => reset_anchor,
            None => {
                let now = chrono::DateTime::<Utc>::from_timestamp_millis(
                    now.to_i64(),
                )
                .context("Invalid timestamp")?
                .with_timezone(&utc_offset);
                unit.next_start(now)
                    .context("Couldn't compute the budget's reset time")?
            }
        };
        Ok(Some(BudgetPolicy::Windowed {
            limit: policy_limit,
            unit,
            n,
            reset_anchor,
        }))
    }
}

// Used for #[serde(default)].
impl Default for BudgetParams {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

impl UpdateBudgetParams {
    /// Clears every param, removing the client's spending limit.
    pub const CLEAR_BUDGET: Self = Self {
        limit: Some(None),
        currency: Some(None),
        reset_interval: Some(None),
        interval_count: Some(None),
        utc_offset_mins: Some(None),
    };

    /// `true` if this update changes nothing.
    pub fn is_noop(&self) -> bool {
        let Self {
            limit,
            currency,
            reset_interval,
            interval_count,
            utc_offset_mins,
        } = self;
        limit.is_none()
            && currency.is_none()
            && reset_interval.is_none()
            && interval_count.is_none()
            && utc_offset_mins.is_none()
    }

    /// `current_policy` with this update applied as of `now`, given the
    /// budget's `fiat_currency`. [`None`] means no spending limit.
    pub fn apply(
        self,
        current_policy: Option<&BudgetPolicy>,
        fiat_currency: IsoCurrencyCode,
        now: TimestampMs,
    ) -> anyhow::Result<Option<BudgetPolicy>> {
        self.validate()?;
        if let Some(Some(BudgetCurrency::Fiat(limit_currency))) = self.currency
        {
            ensure!(
                limit_currency == fiat_currency,
                "currency must be BTC or {fiat_currency}, but got \
                 {limit_currency}"
            );
        }

        let current = current_policy
            .map(|policy| policy.params(fiat_currency))
            .unwrap_or(BudgetParams::UNLIMITED);

        // Destructure to get a compile error when a new field is added,
        // reminding us to merge it below.
        let Self {
            limit,
            currency,
            reset_interval,
            interval_count,
            utc_offset_mins,
        } = self;
        let merged = BudgetParams {
            limit: limit.unwrap_or(current.limit),
            currency: currency.unwrap_or(current.currency),
            reset_interval: reset_interval.unwrap_or(current.reset_interval),
            interval_count: interval_count.unwrap_or(current.interval_count),
            utc_offset_mins: utc_offset_mins.unwrap_or(current.utc_offset_mins),
        };

        merged.policy(current_policy, now)
    }

    /// Errors if this update breaks the dependency rules in the
    /// [module docs](crate::budget::models).
    fn validate(&self) -> anyhow::Result<()> {
        // Destructure to get a compile error when a new field is added,
        // reminding us to include it in the dependency rules.
        let Self {
            limit,
            currency,
            reset_interval,
            interval_count,
            utc_offset_mins,
        } = self;

        // `limit` and `currency` are always set, changed, and cleared together.
        match (limit, currency) {
            (None, None)
            | (Some(Some(_)), Some(Some(_)))
            | (Some(None), Some(None)) => (),
            _ => bail!(
                "Must set limit and currency together, or clear them together"
            ),
        }

        // Updating a param requires updating its required children.
        if let Some(Some(_)) = reset_interval {
            let missing_fields = helpers::join_if_true(&[
                ("interval_count", interval_count.is_none()),
                ("utc_offset_mins", utc_offset_mins.is_none()),
            ]);
            if let Some(fields) = missing_fields {
                bail!("Must update {fields} when updating reset_interval");
            }
        }

        // Clearing a param requires clearing its children.
        if let Some(None) = limit {
            let uncleared_fields = helpers::join_if_true(&[
                ("reset_interval", !matches!(reset_interval, Some(None))),
                ("interval_count", !matches!(interval_count, Some(None))),
                ("utc_offset_mins", !matches!(utc_offset_mins, Some(None))),
            ]);
            if let Some(fields) = uncleared_fields {
                bail!("Must clear {fields} when clearing limit");
            }
        } else if let Some(None) = reset_interval {
            let uncleared_fields = helpers::join_if_true(&[
                ("interval_count", !matches!(interval_count, Some(None))),
                ("utc_offset_mins", !matches!(utc_offset_mins, Some(None))),
            ]);
            if let Some(fields) = uncleared_fields {
                bail!("Must clear {fields} when clearing reset_interval");
            }
        }
        Ok(())
    }
}

impl BudgetCurrency {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Bitcoin => IsoCurrencyCode::BTC.as_str(),
            Self::Fiat(currency) => currency.as_str(),
        }
    }
}

impl Serialize for BudgetCurrency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.as_str().serialize(serializer)
    }
}

impl fmt::Display for BudgetCurrency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BudgetCurrency {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        IsoCurrencyCode::from_str(s).map(Self::from).map_err(|e| {
            anyhow!(
                "Expected a currency code like \"BTC\" or \"USD\", \
                 but got {s:?}: {e}"
            )
        })
    }
}

impl From<IsoCurrencyCode> for BudgetCurrency {
    fn from(code: IsoCurrencyCode) -> Self {
        if code == IsoCurrencyCode::BTC {
            Self::Bitcoin
        } else {
            Self::Fiat(code)
        }
    }
}

impl From<BudgetResetInterval> for CalendarUnit {
    fn from(reset_interval: BudgetResetInterval) -> Self {
        match reset_interval {
            BudgetResetInterval::Daily => Self::Day,
            BudgetResetInterval::Weekly => Self::Week,
            BudgetResetInterval::Monthly => Self::Month,
            BudgetResetInterval::Yearly => Self::Year,
        }
    }
}

impl From<CalendarUnit> for BudgetResetInterval {
    fn from(unit: CalendarUnit) -> Self {
        match unit {
            CalendarUnit::Day => Self::Daily,
            CalendarUnit::Week => Self::Weekly,
            CalendarUnit::Month => Self::Monthly,
            CalendarUnit::Year => Self::Yearly,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{Arbitrary, any},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for BudgetCurrency {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<IsoCurrencyCode>().prop_map(Self::from).boxed()
        }
    }
}

mod helpers {
    /// Joins the names whose flag is true into a list, e.g. "a and b".
    /// [`None`] if no flag is true.
    pub(super) fn join_if_true(names: &[(&str, bool)]) -> Option<String> {
        let names = names
            .iter()
            .filter(|(_, flag)| *flag)
            .map(|(name, _)| *name)
            .collect::<Vec<_>>();
        match names.as_slice() {
            [] => None,
            [name] => Some((*name).to_owned()),
            [init @ .., last] =>
                Some(format!("{} and {last}", init.join(", "))),
        }
    }
}

#[cfg(test)]
mod test {
    use lexe_common::{dec, test_utils::roundtrip};
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    const BTC: BudgetCurrency = BudgetCurrency::Bitcoin;
    const USD: BudgetCurrency = BudgetCurrency::Fiat(IsoCurrencyCode::USD);

    // -- Helpers -- //

    /// Create a [`BudgetParams`], with fields in declaration order.
    fn p(
        limit: impl Into<Option<Decimal>>,
        currency: impl Into<Option<BudgetCurrency>>,
        reset_interval: impl Into<Option<BudgetResetInterval>>,
        interval_count: impl Into<Option<u8>>,
        utc_offset_mins: impl Into<Option<i32>>,
    ) -> BudgetParams {
        BudgetParams {
            limit: limit.into(),
            currency: currency.into(),
            reset_interval: reset_interval.into(),
            interval_count: interval_count
                .into()
                .map(|n| NonZeroU8::new(n).unwrap()),
            utc_offset_mins: utc_offset_mins.into(),
        }
    }

    /// Create an [`UpdateBudgetParams`], with fields in declaration order.
    fn u(
        limit: UpdateField<Decimal>,
        currency: UpdateField<BudgetCurrency>,
        reset_interval: UpdateField<BudgetResetInterval>,
        interval_count: UpdateField<u8>,
        utc_offset_mins: UpdateField<i32>,
    ) -> UpdateBudgetParams {
        fn into_optopt<T>(field: UpdateField<T>) -> Option<Option<T>> {
            match field {
                UpdateField::Keep => None,
                UpdateField::Clear => Some(None),
                UpdateField::Set(value) => Some(Some(value)),
            }
        }
        UpdateBudgetParams {
            limit: into_optopt(limit),
            currency: into_optopt(currency),
            reset_interval: into_optopt(reset_interval),
            interval_count: into_optopt(interval_count)
                .map(|count| count.map(|n| NonZeroU8::new(n).unwrap())),
            utc_offset_mins: into_optopt(utc_offset_mins),
        }
    }

    /// One field of an [`UpdateBudgetParams`].
    enum UpdateField<T> {
        Keep,   // `None`
        Clear,  // `Some(None)`
        Set(T), // `Some(Some(T))`
    }

    fn at(rfc3339: &str) -> TimestampMs {
        let at = chrono::DateTime::parse_from_rfc3339(rfc3339).unwrap();
        TimestampMs::try_from(at.timestamp_millis()).unwrap()
    }

    /// A windowed policy's `reset_anchor`, in RFC 3339 to compare offsets too.
    fn anchor(policy: &BudgetPolicy) -> String {
        match policy {
            BudgetPolicy::Windowed { reset_anchor, .. } =>
                reset_anchor.to_rfc3339(),
            BudgetPolicy::Total { .. } => panic!("Expected a windowed policy"),
        }
    }

    // -- Design guarantees -- //

    /// [`BudgetParams::policy`] and [`UpdateBudgetParams::apply`] reject params
    /// which break the dependency rules in the module docs. A child set without
    /// its parent, a parent set without a required child, or a parent cleared
    /// without its children, is rejected.
    #[test]
    fn dependency_rules_are_enforced() {
        use BudgetResetInterval::*;
        use UpdateField::*;
        let fiat_currency = IsoCurrencyCode::USD;
        let now = at("2026-03-01T03:00:00Z");

        #[rustfmt::skip]
        let invalid_params = [
            // limit    curr. reset  count offset error
            (p(None,    BTC,  None,  None, None), "Must set limit"),
            (p(dec!(1), None, None,  None, None), "Must set currency"),
            (p(dec!(1), BTC,  None,  2,    None), "Must set reset_interval"),
            (p(dec!(1), BTC,  None,  None, 60),   "Must set reset_interval"),
            (p(dec!(1), BTC,  Daily, None, 0),    "Must set interval_count"),
            (p(dec!(1), BTC,  Daily, 1,    None), "Must set utc_offset_mins"),
        ];
        for (params, expected_err) in invalid_params {
            let no_current_policy = None;
            let err = params
                .policy(no_current_policy, now)
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected_err), "{params:?}: {err}");
        }

        let every_2_weeks = p(dec!(5), USD, Weekly, 2, -300);
        let no_current_policy = None;
        let current_policy = every_2_weeks
            .policy(no_current_policy, now)
            .unwrap()
            .unwrap();
        #[rustfmt::skip]
        let invalid_updates = [
            // limit          currency   reset         count offset error
            (u(Set(dec!(10)), Keep,      Keep,         Keep, Keep), "Must set limit and currency together"),
            (u(Set(dec!(10)), Clear,     Keep,         Keep, Keep), "Must set limit and currency together"),
            (u(Keep,          Keep,      Set(Monthly), Keep, Keep), "Must update interval_count and utc_offset_mins"),
            (u(Clear,         Clear,     Keep,         Keep, Keep), "Must clear reset_interval, interval_count and utc_offset_mins"),
            (u(Keep,          Keep,      Clear,        Keep, Keep), "Must clear interval_count and utc_offset_mins"),
        ];
        for (update, expected_err) in invalid_updates {
            let err = update
                .clone()
                .apply(Some(&current_policy), fiat_currency, now)
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected_err), "{update:?}: {err}");
        }
    }

    // -- Code correctness tests -- //

    /// Tests [`BudgetParams::policy`]'s reset anchor:
    /// - A new schedule anchors at the start of the next period, in the
    ///   budget's UTC offset.
    /// - Changing only the limit keeps the anchor.
    /// - Changing the reset interval, interval count, or UTC offset re-anchors
    ///   at the next period start.
    #[test]
    fn policy_reset_anchor() {
        use BudgetResetInterval::*;
        let every_5_months = p(dec!(1), BTC, Monthly, 5, -300);
        let now = at("2026-02-28T22:00:00-05:00");
        let no_current_policy = None;
        let current_policy = every_5_months
            .policy(no_current_policy, now)
            .unwrap()
            .unwrap();
        assert_eq!(anchor(&current_policy), "2026-03-01T00:00:00-05:00");

        // (every_5_months with one param changed, resulting anchor)
        #[rustfmt::skip]
        let changed = [
            (p(dec!(2), BTC,  Monthly, 5, -300), "2026-03-01T00:00:00-05:00"),
            (p(dec!(1), BTC,  Weekly,  5, -300), "2026-05-17T00:00:00-05:00"),
            (p(dec!(1), BTC,  Monthly, 1, -300), "2026-06-01T00:00:00-05:00"),
            (p(dec!(1), BTC,  Monthly, 5, 0),    "2026-06-01T00:00:00+00:00"),
        ];
        // A Thursday.
        let later = at("2026-05-14T19:00:00-05:00");
        for (params, expected_anchor) in changed {
            let new_policy = params
                .policy(Some(&current_policy), later)
                .unwrap()
                .unwrap();
            assert_eq!(anchor(&new_policy), expected_anchor, "{params:?}");
        }
    }

    /// [`BudgetPolicy::params`] inverts [`BudgetParams::policy`], given the
    /// current policy.
    #[test]
    fn policy_params_roundtrip() {
        proptest!(|(
            policy: BudgetPolicy,
            fiat_currency: IsoCurrencyCode,
            now: TimestampMs,
        )| {
            let params = policy.params(fiat_currency);
            let roundtripped = params.policy(Some(&policy), now).unwrap();
            prop_assert_eq!(roundtripped, Some(policy));
        });
    }

    // -- Roundtrip proptests -- //

    #[test]
    fn budget_currency_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<BudgetCurrency>();
        roundtrip::fromstr_json_string_equiv::<BudgetCurrency>();
    }

    #[test]
    fn budget_params_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<BudgetParams>();
    }

    #[test]
    fn update_budget_params_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<UpdateBudgetParams>();
    }

    #[test]
    fn budget_info_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<BudgetInfo>();
    }
}
