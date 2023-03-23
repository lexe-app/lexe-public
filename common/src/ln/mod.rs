//! Bitcoin / Lightning Lexe newtypes which have to be in `common` for some
//! reason, likely because they are referenced in an API definition somewhere.

/// Channel outpoint, details, counterparty
pub mod channel;
/// `LxInvoice`
pub mod invoice;
/// Payments types and newtypes.
pub mod payments;
/// `ChannelPeer`.
pub mod peer;
