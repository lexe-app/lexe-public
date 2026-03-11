//! Types used by the Lexe SDK.

// ## Guidelines
//
// - **Simple**: Straightforward consumption by newbie developers via a JSON
//   REST API (Lexe Sidecar SDK) or via language bindings (Lexe SDK).
//
//   - *Minimal nesting* means users don't have to define multiple structs per
//     request / response.
//   - *Fewer fields* means fewer long-term compatibility commitments.
//
// - **User-facing docs**: `///` doc strings here will be rendered in the public
//   API docs. Write for SDK users, not Lexe developers.
//
// - **Document serialization and units**: When newtypes are used, document how
//   users should interpret the serialized form:
//
//   - `UserPk`s and `NodePk`s are serialized as hex; mention it.
//   - `Amount`s are serialized as sats; mention it.
//   - `TimestampMs` is serialized as *milliseconds* since the UNIX epoch.
//   - `semver::Version`s don't use a `v-` prefix; give an example: `0.6.9`.
//
// - **Serialize `null`**: Don't use `#[serde(skip_serializing_if = ...)]` as
//   serializing `null` fields makes it clear to SDK users that information
//   could be returned there in future responses.

/// Request, response, and command types for SDK operations.
pub mod command;
/// Payment data types.
pub mod payment;

/// Authentication, identity, and node verification.
pub mod auth {
    pub use lexe_common::{
        api::user::{NodePk, UserPk},
        enclave::Measurement,
        root_seed::RootSeed,
    };
    pub use lexe_node_client::credentials::{
        ClientCredentials, Credentials, CredentialsRef,
    };
}

/// On-chain and Bitcoin primitives.
pub mod bitcoin {
    pub use lexe_api::types::invoice::LxInvoice;
    pub use lexe_common::ln::{
        amount::Amount, hashes::LxTxid, priority::ConfirmationPriority,
    };
}

/// General-purpose utilities.
pub mod util {
    pub use lexe_common::time::TimestampMs;
}
