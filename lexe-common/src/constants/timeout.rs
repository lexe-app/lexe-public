//! Timeout constants, organized into nested modules named after the API trait,
//! service, or struct whose timeouts it holds.
//!
//! `const_assert!`s are used to enforce our timeout hierarchies; e.g. each
//! timeout along a request chain must be shorter than the one enclosing it.
//!
//! Principles for tuning:
//!
//! - When a timeout increase is needed: prefer to raise the timeout for a
//!   specific server or client method that needs it, rather than the global
//!   default.
//! - Be careful when decreasing timeouts baked into enclave programs - if it
//!   cannot be overridden via CLI, there is no way to backtrack if the decrease
//!   was too aggressive. Wire through a CLI arg before decreasing the timeout.

use std::time::Duration;

use lexe_std::const_assert;

/// Defaults for Lexe API servers.
pub mod server {
    use super::*;

    /// The grace period passed to `axum_server::Handle::graceful_shutdown`
    /// during which new connections are refused and we wait for existing
    /// connections to terminate before initiating a hard shutdown.
    pub const SHUTDOWN_GRACE_PERIOD: Duration = Duration::from_secs(3);

    /// The maximum time we'll wait for a server to complete shutdown.
    pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

    /// The default maximum time a server can spend handling a request.
    pub const DEFAULT_HANDLER_TIMEOUT: Duration = Duration::from_secs(25);

    const_assert!(SHUTDOWN_TIMEOUT.as_secs() > SHUTDOWN_GRACE_PERIOD.as_secs());
}

/// Defaults for Lexe `RestClient`s.
pub mod client {
    use super::*;

    /// The default request timeout for Lexe `RestClient`s.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    const_assert!(
        DEFAULT_TIMEOUT.as_secs() > server::DEFAULT_HANDLER_TIMEOUT.as_secs()
    );
}

/// Timeouts for `UserNode`s.
pub mod usernode {
    use super::*;

    /// Default sync timeout for user nodes (BDK/LDK sync).
    //
    // (2026-07-24): 30s -> 15s. The const was created at 30s despite 15s being
    // the documented default; prod doesn't override it.
    pub const DEFAULT_SYNC_TIMEOUT: Duration = Duration::from_secs(15);

    /// The amount of time user node tasks have to finish after a graceful
    /// shutdown signal is received before the task is forced to exit.
    pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(25);

    // The meganode's `run_user` handler (bounded by the default
    // `server::DEFAULT_HANDLER_TIMEOUT`) blocks on the usernode's boot sync
    // before responding.
    const_assert!(
        DEFAULT_SYNC_TIMEOUT.as_secs()
            < server::DEFAULT_HANDLER_TIMEOUT.as_secs()
    );
    const_assert!(
        SHUTDOWN_TIMEOUT.as_secs() > server::SHUTDOWN_TIMEOUT.as_secs()
    );
}

/// Timeouts for in-enclave `UserRunner`.
pub mod userrunner {
    use super::*;

    /// The amount of time the user runner has to finish after a graceful
    /// shutdown signal is received before the program is forced to exit.
    pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(27);

    const_assert!(
        SHUTDOWN_TIMEOUT.as_secs() > usernode::SHUTDOWN_TIMEOUT.as_secs()
    );
}

/// Timeouts for `UserNodeProvisionApi` (User -> Node).
pub mod user_node_provision_api {
    use super::*;

    /// `NodeClient`'s request timeout for provisioning. Generous because the
    /// provision handler does the Google Drive OAuth exchange and GVFS setup.
    pub const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

    const_assert!(
        CLIENT_TIMEOUT.as_secs() > server::DEFAULT_HANDLER_TIMEOUT.as_secs()
    );
}

/// Timeouts for `UserNodeRunApi` (User -> Node).
pub mod user_node_run_api {
    use super::*;

    /// Handling timeout for the node's user-facing server. Generous because
    /// `[preflight_]pay_invoice` may compute `max_flow`, which takes ~30s at
    /// 10 iterations and ~50s at 17 iterations.
    /// See `compute_max_flow_to_recipient` for more details.
    pub const SERVER_HANDLER_TIMEOUT: Duration = Duration::from_secs(60);

    /// Timeouts for endpoints which may compute `max_flow`.
    pub mod max_flow {
        use super::*;

        /// Client timeout for endpoints which may compute `max_flow`.
        pub const CLIENT_TIMEOUT: Duration = Duration::from_secs(62);

        const_assert!(
            CLIENT_TIMEOUT.as_secs()
                > user_node_run_api::SERVER_HANDLER_TIMEOUT.as_secs()
        );
    }
}
