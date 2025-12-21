// NOTE(phlip9): for some reason, placing this in `crate::app` causes
// flutter_rust_bridge to generate a dart type for it...
pub struct GDriveSignupCredentials {
    /// The user's backup password, used to encrypt their `RootSeed` backup
    /// on Google Drive.
    pub backup_password: String,
    /// The google auth code passed to the node enclave during provisioning.
    pub google_auth_code: String,
}
