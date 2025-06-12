use std::{
    cmp::Ordering,
    fmt::{self, Display},
    str::FromStr,
};

use bitcoin::{hashes::Hash as _, Txid};
use serde::{Deserialize, Serialize};

/// Almost exactly [`bitcoin::Txid`], but fixes the inconsistency between the
/// string-serialized and unserialized orderings caused by bitcoin sha256d hash
/// types being displayed in reverse hex order (thanks Satoshi!). Also provides
/// an `Arbitrary` impl. When neither of these are required, it is perfectly
/// fine (and equivalent) to use [`bitcoin::Txid`] directly.
///
/// To ensure that we don't accidentally display a non-reversed hash to a Lexe
/// user, we still display using [`Txid`]'s provided reverse hex impl, but we
/// override the [`Ord`] implementation to be consistent with the user-facing
/// lexicographic ordering.
///
/// See [`bitcoin::hashes::Hash::DISPLAY_BACKWARD`] or the `hash_newtype!`
/// definition of [`Txid`] for more info.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct LxTxid(pub Txid);

impl Display for LxTxid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Txid::fmt(&self.0, f)
    }
}

impl FromStr for LxTxid {
    type Err = bitcoin::hex::HexToArrayError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Txid::from_str(s).map(Self)
    }
}

impl Ord for LxTxid {
    fn cmp(&self, other: &Self) -> Ordering {
        // Compare the two hashes byte by byte, starting with the least
        // significant byte (i.e. in reverse order), returning as soon as we
        // find a pair of bytes that are not equal, returning Ordering::Equal if
        // all of the bytes were equal.
        self.0
            .as_raw_hash()
            .as_byte_array()
            .iter()
            .rev()
            .cmp(other.0.as_byte_array().iter().rev())
    }
}

impl PartialOrd for LxTxid {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(any(test, feature = "test-utils"))]
mod arbitrary_impl {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for LxTxid {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            // Excluding .no_shrink() makes it easier to debug
            any::<[u8; 32]>()
                .prop_map(Txid::from_byte_array)
                .prop_map(Self)
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::{arbitrary::any, prop_assert_eq, proptest};

    use super::*;
    use crate::test_utils::roundtrip;

    #[test]
    fn txid_roundtrip() {
        roundtrip::fromstr_display_roundtrip_proptest::<LxTxid>();
        roundtrip::json_string_roundtrip_proptest::<LxTxid>();
        roundtrip::bcs_roundtrip_proptest::<LxTxid>();
    }

    #[test]
    fn txid_ordering_equivalence() {
        proptest!(|(txid1 in any::<LxTxid>(), txid2 in any::<LxTxid>())| {
            let txid1_str = txid1.to_string();
            let txid2_str = txid2.to_string();

            let unserialized_order = txid1.cmp(&txid2);
            let string_serialized_order = txid1_str.cmp(&txid2_str);

            prop_assert_eq!(unserialized_order, string_serialized_order);
        });
    }
}
