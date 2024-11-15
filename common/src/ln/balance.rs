use serde::{Deserialize, Serialize};

/// Basically `bdk::Balance`, so that `common` doesn't need to depend on `bdk`.
///
/// Partitions a wallet balance into different categories.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone, Default)]
pub struct Balance {
    /// All coinbase outputs not yet matured
    pub immature: bitcoin::Amount,
    /// Unconfirmed UTXOs generated by a wallet tx
    pub trusted_pending: bitcoin::Amount,
    /// Unconfirmed UTXOs received from an external wallet
    pub untrusted_pending: bitcoin::Amount,
    /// Confirmed and immediately spendable balance
    pub confirmed: bitcoin::Amount,
}

impl Balance {
    /// Get sum of trusted pending and confirmed coins
    pub fn spendable(&self) -> bitcoin::Amount {
        self.confirmed + self.trusted_pending
    }

    /// Get the whole balance visible to the wallet
    pub fn total(&self) -> bitcoin::Amount {
        self.confirmed
            + self.trusted_pending
            + self.untrusted_pending
            + self.immature
    }
}

impl std::fmt::Display for Balance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let spendable = self.spendable();
        let total = self.total();
        write!(
            f,
            "{{ spendable balance: {spendable} sats, total: {total} sats }}"
        )
    }
}

impl std::ops::Add for Balance {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            immature: self.immature + other.immature,
            trusted_pending: self.trusted_pending + other.trusted_pending,
            untrusted_pending: self.untrusted_pending + other.untrusted_pending,
            confirmed: self.confirmed + other.confirmed,
        }
    }
}

impl std::iter::Sum for Balance {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(
            Balance {
                ..Default::default()
            },
            |a, b| a + b,
        )
    }
}
