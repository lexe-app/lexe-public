/// Build the user agent string used for requests to internal services at
/// compile time.
///
/// Example: "node-v0.6.20".
#[macro_export]
macro_rules! user_agent_internal {
    () => {
        concat!(env!("CARGO_PKG_NAME"), "-v", env!("CARGO_PKG_VERSION"))
    };
}

/// Build the user agent string used for requests to external services at
/// compile time.
///
/// Example: "lexe-node-v0.6.20".
#[macro_export]
macro_rules! user_agent_external {
    () => {
        concat!("lexe-", $crate::user_agent_internal!())
    };
}
