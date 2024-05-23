use std::{
    fmt::{self, Display},
    str::FromStr,
};

#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::ln::amount::Amount;

/// An enum used to send either (1) an explicit amount, or (2) the full
/// spendable balance.
///
/// This container type provides a nice JSON-serialization and is less
/// error-prone and more explicit than `Option<Amount>`.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
#[derive(Serialize, Deserialize)]
#[cfg_attr(any(test, feature = "test-utils"), derive(Arbitrary))]
pub enum AmountOrAll {
    #[serde(rename = "all")]
    All,

    #[serde(untagged)]
    Amount(Amount),
}

impl Display for AmountOrAll {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("all"),
            Self::Amount(amount) => Display::fmt(amount, f),
        }
    }
}

impl FromStr for AmountOrAll {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "all" {
            Ok(Self::All)
        } else {
            Amount::from_str(s).map(Self::Amount)
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::proptest;

    use super::*;

    #[test]
    fn test_fromstr_display_and_serde_equiv() {
        proptest!(|(amount: AmountOrAll)| {
            let str_serde = serde_json::to_string(&amount).unwrap();
            let str_fmt = amount.to_string();
            let str_fmt_json = format!("\"{str_fmt}\"");

            assert_eq!(str_serde, str_fmt_json);

            let deser_serde = serde_json::from_str::<AmountOrAll>(&str_fmt_json).unwrap();
            let deser_fromstr = AmountOrAll::from_str(&str_fmt).unwrap();

            assert_eq!(deser_serde, deser_fromstr);
        });
    }

    #[test]
    fn test_json() {
        #[derive(Debug, Serialize, Deserialize)]
        struct Dummy {
            amount: AmountOrAll,
        }

        let amount = Dummy {
            amount: AmountOrAll::Amount(Amount::from_sats_u32(123000)),
        };
        let all = Dummy {
            amount: AmountOrAll::All,
        };

        println!("{}", serde_json::to_string_pretty(&amount).unwrap());
        println!("{}", serde_json::to_string_pretty(&all).unwrap());

        let json1 = r#"{ "amount": "123000" }"#;
        let json2 = r#"{ "amount": "all" }"#;

        assert_eq!(
            amount.amount,
            serde_json::from_str::<Dummy>(json1).unwrap().amount,
        );
        assert_eq!(
            all.amount,
            serde_json::from_str::<Dummy>(json2).unwrap().amount
        );
    }
}
