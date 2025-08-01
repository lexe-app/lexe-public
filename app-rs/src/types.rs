// NOTE(phlip9): for some reason, placing this in `crate::app` causes
// flutter_rust_bridge to generate a dart type for it...
pub struct GDriveSignupCredentials {
    /// The server auth code passed to the node enclave during provisioning.
    pub server_auth_code: String,
    /// The user's backup password, used to encrypt their `RootSeed` backup
    /// on Google Drive.
    pub password: String,
}
