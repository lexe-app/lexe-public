//! Bitcoin / Lightning Lexe newtypes which have to be in `common` for some
//! reason, likely because they are referenced in an API definition.

/// `LxSocketAddress`
pub mod addr;
/// `Amount`.
pub mod amount;
/// `AmountOrAll`.
pub mod amount_or_all;
/// `Balance`.
pub mod balance;
/// Channel outpoint, details, counterparty
pub mod channel;
/// Bitcoin hash types, such as `LxTxid`.
pub mod hashes;
/// `LxNetwork`, a newtype for [`bitcoin::Network`].
pub mod network;
/// `LxNodeAlias`.
pub mod node_alias;
/// Confirmation priorities.
pub mod priority;
/// `LxRoute`.
pub mod route;
