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
/// `LxInvoice`, a wrapper around LDK's BOLT11 invoice type.
pub mod invoice;
/// `LxNetwork`, a newtype for [`bitcoin::Network`].
pub mod network;
/// `LxOffer`, a wrapper around LDK's BOLT12 offer type.
pub mod offer;
/// Payments types and newtypes.
pub mod payments;
/// `LnPeer`.
pub mod peer;
/// Confirmation priorities.
pub mod priority;
