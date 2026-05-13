#[cfg(any(test, feature = "test-utils"))]
use lexe_common::test_utils::arbitrary;
use lexe_common::{ppm, ppm::Ppm};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{arbitrary::Arbitrary, strategy::BoxedStrategy};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// Information about Lexe partners, like the revshare schedule.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct PartnersInfo {
    // NOTE(phlip9): To make a breaking change to the revshare schedule, it's
    // probably easiest to add a new "default-v2" `RevshareSchedule` and keep
    // the old schedule around for old nodes to use.
    revshare_schedules: Vec<RevshareSchedule>,
}

/// The partner revshare schedule maps the `total_partner_fee` to the partner's
/// share of the fees for payments that they facilitate.
///
/// The `total_partner_fee` is the _total_ proportional fee a partner wishes to
/// charge on payments they help facilitate.
///
/// See: <https://docs.lexe.tech/partner-fees/>.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq, Arbitrary))]
pub struct RevshareSchedule {
    /// The revshare schedule name. In the future we may have per-partner or
    /// per-volume-tier schedules, which will have different `name`s.
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "arbitrary::any_string()")
    )]
    name: String,

    /// An ordered list of revshare bands, from high to low priority.
    bands: Vec<Band>,
}

// NOTE(phlip9): the only two required fields are `lower/upper_bound` to
// support more easily making breaking changes to the revshare policy.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Eq, PartialEq))]
pub struct Band {
    /// The lower bound `total_partner_fee` (inclusive).
    #[serde(rename = "l")]
    lower_bound: Ppm,

    /// The upper bound `total_partner_fee` (exclusive).
    #[serde(rename = "u")]
    upper_bound: Ppm,

    /// Some bands may be disallowed and return an error.
    /// - ex: `[0, 5000) -> "partner fee is too low"`.
    #[serde(rename = "e", skip_serializing_if = "Option::is_none")]
    error: Option<String>,

    /// The partner's revshare for this `total_partner_fee`.
    #[serde(rename = "r", skip_serializing_if = "Option::is_none")]
    revshare: Option<Ppm>,
}

// --- impl PartnersInfo --- //

impl PartnersInfo {
    /// Return the partner's proportional revshare of the payment fee given
    /// the partner's total proportional fee. Returns `None` if there is no
    /// schedule with `schedule_name`.
    ///
    /// This total is the effective proportional fee after `partner_prop_fee`
    /// and `partner_base_fee` are both combined and taken into account, i.e.,
    /// `total_partner_fee := (partner_prop_fee * amount + partner_base_fee) /
    /// amount`.
    ///
    /// We don't currently support a `partner_base_fee` for amountless invoices,
    /// in which case `total_partner_fee := partner_prop_fee`.
    pub fn revshare_for_partner_fee(
        &self,
        schedule_name: &str,
        total_partner_fee: Ppm,
    ) -> Result<Option<Ppm>, String> {
        self.revshare_schedules
            .iter()
            .find(|s| s.name == schedule_name)
            .map(|schedule| {
                schedule.revshare_for_partner_fee(total_partner_fee)
            })
            .transpose()
    }

    /// The current partners info with the latest advertised revshare schedules.
    pub fn current() -> Self {
        Self {
            revshare_schedules: vec![RevshareSchedule::current()],
        }
    }

    /// Returns `true` if all `RevshareSchedule::is_covering`.
    #[cfg(test)]
    fn is_all_covering(&self) -> bool {
        self.revshare_schedules.iter().all(|rs| rs.is_covering())
    }
}

// --- impl RevshareSchedule --- //

impl RevshareSchedule {
    pub const DEFAULT_NAME: &str = "default-v1";

    /// The current advertised partner revshare schedule.
    fn current() -> Self {
        Self {
            name: Self::DEFAULT_NAME.to_owned(),
            bands: vec![
                Band {
                    lower_bound: ppm!(0.0%),
                    upper_bound: ppm!(0.5%),
                    error: Some(
                        "Total partner fee is too low, min: 5000 ppm"
                            .to_owned(),
                    ),
                    revshare: None,
                },
                Band {
                    lower_bound: ppm!(0.5%),
                    upper_bound: ppm!(1.0%),
                    error: None,
                    revshare: Some(ppm!(20.0%)),
                },
                Band {
                    lower_bound: ppm!(1.0%),
                    upper_bound: ppm!(3.0%),
                    error: None,
                    revshare: Some(ppm!(50.0%)),
                },
                Band {
                    lower_bound: ppm!(3.0%),
                    upper_bound: ppm!(10.0%),
                    error: None,
                    revshare: Some(ppm!(70.0%)),
                },
                Band {
                    lower_bound: ppm!(10.0%),
                    upper_bound: ppm!(50.0%),
                    error: None,
                    revshare: Some(ppm!(80.0%)),
                },
                Band {
                    lower_bound: ppm!(50.0%),
                    upper_bound: ppm!(100.0%),
                    error: Some(
                        "Total partner fee is too high, max: 500000 ppm"
                            .to_owned(),
                    ),
                    revshare: None,
                },
            ],
        }
    }

    /// Return the partner's proportional revshare of the payment fee given
    /// the total partner fee (in ppm).
    ///
    /// See: [`PartnersInfo::revshare_for_partner_fee`]
    fn revshare_for_partner_fee(
        &self,
        total_partner_fee: Ppm,
    ) -> Result<Ppm, String> {
        for band in &self.bands {
            // Check if in relevant band
            if total_partner_fee < band.lower_bound
                || total_partner_fee >= band.upper_bound
            {
                continue;
            }

            // Errors take precedence
            if let Some(err) = &band.error {
                return Err(err.clone());
            }

            if let Some(revshare) = band.revshare {
                return Ok(revshare);
            }
        }

        // Manually check this to improve the error message in this specific
        // edge case.
        if total_partner_fee == Ppm::MAX {
            return Err(
                "Total partner fee is too high, max: 500000 ppm".to_owned()
            );
        }

        let msg = "No matching revshare band found; this user node is \
                   probably too old";
        Err(msg.to_owned())
    }

    /// Returns `true` if the union of `[band.lower_bound, band.upper_bound)`
    /// covers the range `[0, 1)`.
    ///
    /// Useful as a sanity check.
    #[cfg(test)]
    fn is_covering(&self) -> bool {
        let mut ranges = self
            .bands
            .iter()
            .map(|b| (b.lower_bound, b.upper_bound))
            .collect::<Vec<_>>();

        ranges.sort_unstable_by_key(|&(lower_bound, _)| lower_bound);

        let mut covered_until = Ppm::ZERO;

        for (lower_bound, upper_bound) in ranges {
            // Ignore ranges that end before current coverage
            if upper_bound <= covered_until {
                continue;
            }

            // Found a gap before this range starts
            if lower_bound > covered_until {
                return false;
            }

            // Extend the covered area
            covered_until = upper_bound;

            // We've covered [0, 1)
            if covered_until >= Ppm::MAX {
                return true;
            }
        }

        false
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl Arbitrary for Band {
    type Strategy = BoxedStrategy<Self>;
    type Parameters = ();
    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        use lexe_common::ppm::Ppm;
        use proptest::{arbitrary::any, option, prelude::Strategy};

        let bound1 = any::<Ppm>();
        let bound2 = any::<Ppm>();
        let error = arbitrary::any_option_string();
        let revshare = option::of(any::<Ppm>());

        (bound1, bound2, error, revshare)
            .prop_map(|(bound1, bound2, error, revshare)| Self {
                lower_bound: if bound1 <= bound2 { bound1 } else { bound2 },
                upper_bound: if bound1 <= bound2 { bound2 } else { bound1 },
                error,
                revshare,
            })
            .boxed()
    }
}

#[cfg(test)]
mod test {
    use lexe_common::test_utils::roundtrip;
    use proptest::{arbitrary::any, test_runner::Config};

    use super::*;

    #[test]
    fn partners_info_roundtrip() {
        roundtrip::json_value_custom(
            any::<PartnersInfo>(),
            Config::with_cases(10),
        );
    }

    #[test]
    fn partners_info_deser_compat() {
        let input = r#"{
  "revshare_schedules": [
    {
      "name": "default-v1",
      "bands": [
        {
          "l": 0,
          "u": 5000,
          "e": "Total partner fee is too low, min: 5000 ppm"
        },
        {
          "l": 5000,
          "u": 10000,
          "r": 200000
        },
        {
          "l": 10000,
          "u": 30000,
          "r": 500000
        },
        {
          "l": 30000,
          "u": 100000,
          "r": 700000
        },
        {
          "l": 100000,
          "u": 500000,
          "r": 800000
        },
        {
          "l": 500000,
          "u": 1000000,
          "e": "Total partner fee is too high, max: 500000 ppm"
        }
      ]
    }
  ]
}"#;

        let p: PartnersInfo = serde_json::from_str(input).unwrap();
        let _ = serde_json::to_string(&p).unwrap();

        assert!(p.is_all_covering());
    }

    #[test]
    fn partners_info_deser_upgrade() {
        // Simulate an old node that ignores the new policy with a breaking
        // change.
        let input = r#"{
  "revshare_schedules": [
    {
      "name": "default-v2",
      "bands": [
        {
          "l": 0,
          "u": 500000,
          "foo": 123
        },
        {
          "l": 500000,
          "u": 1000000,
          "q": "123456.45"
        }
      ]
    },
    {
      "name": "default-v1",
      "bands": [
        {
          "l": 0,
          "u": 1000000,
          "r": 200000
        }
      ]
    }
  ]
}"#;

        let p: PartnersInfo = serde_json::from_str(input).unwrap();
        assert_eq!(
            p.revshare_for_partner_fee("default-v1", Ppm::ZERO).unwrap(),
            Some(ppm!(20.0%)),
        );
    }

    #[test]
    fn partners_info_current() {
        let p = PartnersInfo::current();

        let name = RevshareSchedule::DEFAULT_NAME;
        let ok = |prop_fee, revshare| {
            assert_eq!(
                p.revshare_for_partner_fee(name, prop_fee),
                Ok(Some(revshare))
            );
        };
        let err = |prop_fee, err: &str| {
            assert_eq!(
                p.revshare_for_partner_fee(name, prop_fee)
                    .as_ref()
                    .map_err(|e| e.as_str()),
                Err(err)
            );
        };

        err(ppm!(0.0000%), "Total partner fee is too low, min: 5000 ppm");
        err(ppm!(0.4999%), "Total partner fee is too low, min: 5000 ppm");

        ok(ppm!(0.5000%), ppm!(20.0%));
        ok(ppm!(0.9999%), ppm!(20.0%));

        ok(ppm!(1.0000%), ppm!(50.0%));
        ok(ppm!(2.9999%), ppm!(50.0%));

        ok(ppm!(3.0000%), ppm!(70.0%));
        ok(ppm!(9.9999%), ppm!(70.0%));

        ok(ppm!(10.0000%), ppm!(80.0%));
        ok(ppm!(49.9999%), ppm!(80.0%));

        err(
            ppm!(50.0000%),
            "Total partner fee is too high, max: 500000 ppm",
        );
        err(
            ppm!(99.9999%),
            "Total partner fee is too high, max: 500000 ppm",
        );
        err(
            ppm!(100.0000%),
            "Total partner fee is too high, max: 500000 ppm",
        );

        // nonexistent schedule -> None
        assert_eq!(
            None,
            p.revshare_for_partner_fee("foo", ppm!(0.5%)).unwrap()
        );
    }

    #[test]
    fn partners_info_upgrade() {
        let new_revshare = ppm!(50.0%);
        let old_revshare = ppm!(20.0%);

        let p = PartnersInfo {
            revshare_schedules: vec![
                RevshareSchedule {
                    name: "new".to_owned(),
                    bands: vec![Band {
                        lower_bound: Ppm::ZERO,
                        upper_bound: Ppm::MAX,
                        error: None,
                        revshare: Some(new_revshare),
                    }],
                },
                RevshareSchedule {
                    name: "old".to_owned(),
                    bands: vec![Band {
                        lower_bound: Ppm::ZERO,
                        upper_bound: Ppm::MAX,
                        error: None,
                        revshare: Some(old_revshare),
                    }],
                },
            ],
        };

        let ok = |name, prop_fee, revshare| {
            assert_eq!(
                p.revshare_for_partner_fee(name, prop_fee),
                Ok(Some(revshare))
            );
        };
        ok("new", ppm!(123), new_revshare);
        ok("old", ppm!(123), old_revshare);
    }
}
