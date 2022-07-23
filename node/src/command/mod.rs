pub mod server;
// BitcoinD regtest doesn't work in SGX, hence the additional not(sgx) flag
#[cfg(all(test, not(target_env = "sgx")))]
pub mod test;

/// Commands that can only be initiated by the host (Lexe).
mod host;
/// Commands that can only be initiated by the node owner.
mod owner;
