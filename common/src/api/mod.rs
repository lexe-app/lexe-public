// TODO(max): All of these modules should be moved to `lexe_api[_core]`.

use serde::{Deserialize, Serialize};

/// Authentication and User Signup.
// TODO(max): `error` depends on `auth`
pub mod auth;
/// Data types returned from the fiat exchange rate API.
pub mod fiat_rates;
/// API models which don't fit anywhere else.
pub mod models;
/// Data types specific to provisioning.
pub mod provision;
/// Revocable clients.
pub mod revocable_clients;
/// `TestEvent`.
pub mod test_event;
/// User ID-like types: `User`, `UserPk`, `NodePk`, `Scid`
pub mod user;
/// Data types which relate to node versions: `NodeRelease`, `MeasurementStruct`
pub mod version;

/// A randomly generated id for each mega node.
pub type MegaId = u16;

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MegaIdStruct {
    pub mega_id: MegaId,
}
