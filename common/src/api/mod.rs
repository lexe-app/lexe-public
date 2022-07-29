use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::hex::{self, FromHex};
use crate::hexstr_or_bytes;

pub mod vfs;

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserPk(#[serde(with = "hexstr_or_bytes")] [u8; 32]);

impl UserPk {
    pub const fn new(inner: [u8; 32]) -> Self {
        Self(inner)
    }

    pub fn inner(&self) -> [u8; 32] {
        self.0
    }

    /// Used to quickly construct `UserPk`s for tests.
    pub fn from_i64(i: i64) -> Self {
        // Convert i64 to [u8; 8]
        let i_bytes = i.to_ne_bytes();

        // Fill the first 8 bytes with the i64 bytes
        let mut inner = [0u8; 32];
        inner[0..8].clone_from_slice(&i_bytes);

        Self(inner)
    }

    /// Used to compare inner `i64` values set during tests
    pub fn to_i64(self) -> i64 {
        let mut i_bytes = [0u8; 8];
        i_bytes.clone_from_slice(&self.0[0..8]);
        i64::from_ne_bytes(i_bytes)
    }
}

impl FromStr for UserPk {
    type Err = hex::DecodeError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <[u8; 32]>::from_hex(s).map(Self::new)
    }
}

impl Display for UserPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::display(self.0.as_slice()))
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::{prop_assert_eq, proptest};

    use super::*;

    #[test]
    fn user_pk_consistent() {
        let user_pk1 = UserPk::new(hex::decode_const(
            b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        let user_pk2 = UserPk::new(hex::decode_const(
            b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        assert_eq!(user_pk1, user_pk2);
    }

    proptest! {
        #[test]
        fn user_pk_tofromstring_round_trip(inner in any::<[u8; 32]>()) {
            let user_pk1 = UserPk::new(inner);
            let user_pk_str = user_pk1.to_string();
            let user_pk2 = UserPk::from_str(&user_pk_str).unwrap();
            prop_assert_eq!(user_pk1, user_pk2);
        }
    }
}
