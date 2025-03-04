/// Helpers for linking to a bitcoin block explorers.
library;

/// Build a URL pointing to mempool.space for the given txid.
String txid(final String txid) => "https://mempool.space/tx/$txid";
