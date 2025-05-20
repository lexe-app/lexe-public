// TODO(max): All of these modules should be moved to `lexe_api[_core]`.

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
