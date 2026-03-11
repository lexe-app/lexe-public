/// serde_with helper for base64-encoded bytes types.
pub mod base64_or_bytes;
/// `base64_or_bytes` but for [`Option`] bytes types.
pub mod base64_or_bytes_opt;
/// serde helper to consensus-encode a [`bitcoin::Transaction`] as hex or bytes.
pub mod consensus_encode_tx;
/// serde_with helper for hex-encoded bytes types.
pub mod hexstr_or_bytes;
/// `hex_str_or_bytes` but for [`Option`] bytes types.
pub mod hexstr_or_bytes_opt;
/// serde helper for "maybe defined" (`Option<Option<T>>`) values.
pub mod optopt;
