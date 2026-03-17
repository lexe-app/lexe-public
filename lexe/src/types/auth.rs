//! Authentication, identity, and node verification.

use std::{fmt, path::Path, str::FromStr};

use anyhow::Context;
use bip39::Mnemonic;
use lexe_common::{
    ExposeSecret, root_seed::RootSeed as UnstableRootSeed,
};
use lexe_crypto::rng::SysRng;
use lexe_node_client::credentials::{
    ClientCredentials as UnstableClientCredentials,
    CredentialsRef as UnstableCredentialsRef,
};
use serde::{Deserialize, Serialize};

use crate::{config::WalletEnv, hex};

/// Re-exports that are part of the SDK's public API.
/// Wrapped in a module so `rustfmt` doesn't merge them with regular imports.
mod reexports {
    pub use lexe_common::api::user::{NodePk, UserPk};
    pub use lexe_enclave::enclave::Measurement;
}
pub use reexports::*;

// --- Credentials --- //

/// Credentials used to authenticate with a Lexe user node.
pub enum Credentials {
    /// Authenticate with a [`RootSeed`].
    RootSeed(RootSeed),
    /// Authenticate with delegated [`ClientCredentials`].
    ClientCredentials(ClientCredentials),
}

impl Credentials {
    /// Borrow as a [`CredentialsRef`].
    pub fn as_ref(&self) -> CredentialsRef<'_> {
        match self {
            Self::RootSeed(root_seed) => CredentialsRef::RootSeed(root_seed),
            Self::ClientCredentials(cc) =>
                CredentialsRef::ClientCredentials(cc),
        }
    }
}

impl From<RootSeed> for Credentials {
    fn from(root_seed: RootSeed) -> Self {
        Self::RootSeed(root_seed)
    }
}

impl From<ClientCredentials> for Credentials {
    fn from(cc: ClientCredentials) -> Self {
        Self::ClientCredentials(cc)
    }
}

// --- CredentialsRef --- //

/// Borrowed version of [`Credentials`].
#[derive(Copy, Clone)]
pub enum CredentialsRef<'a> {
    /// Authenticate with a borrowed [`RootSeed`].
    RootSeed(&'a RootSeed),
    /// Authenticate with borrowed [`ClientCredentials`].
    ClientCredentials(&'a ClientCredentials),
}

impl<'a> CredentialsRef<'a> {
    /// Returns the user public key, if available.
    ///
    /// Always `Some(_)` if the credentials were created by `node-v0.8.11+`.
    pub(crate) fn user_pk(self) -> Option<UserPk> {
        self.to_unstable().user_pk()
    }

    /// Convert to the inner [`UnstableCredentialsRef`] used by
    /// `lexe-node-client`.
    pub(crate) fn to_unstable(self) -> UnstableCredentialsRef<'a> {
        match self {
            Self::RootSeed(root_seed) =>
                UnstableCredentialsRef::RootSeed(root_seed.unstable()),
            Self::ClientCredentials(cc) =>
                UnstableCredentialsRef::ClientCredentials(cc.unstable()),
        }
    }
}

impl<'a> From<&'a RootSeed> for CredentialsRef<'a> {
    fn from(root_seed: &'a RootSeed) -> Self {
        Self::RootSeed(root_seed)
    }
}

impl<'a> From<&'a ClientCredentials> for CredentialsRef<'a> {
    fn from(cc: &'a ClientCredentials) -> Self {
        Self::ClientCredentials(cc)
    }
}

// --- RootSeed --- //

/// The root secret from which the user node's keys and credentials are derived.
#[derive(Serialize, Deserialize)]
pub struct RootSeed(UnstableRootSeed);

impl RootSeed {
    // --- Constructors & File I/O --- //

    /// Generate a new random [`RootSeed`] using the system CSPRNG.
    pub fn generate() -> Self {
        Self(UnstableRootSeed::from_rng(&mut SysRng::new()))
    }

    /// Read a [`RootSeed`] from the default seedphrase path for this
    /// environment (`~/.lexe/seedphrase[.env].txt`).
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    pub fn read(wallet_env: &WalletEnv) -> anyhow::Result<Option<Self>> {
        let lexe_data_dir = lexe_common::default_lexe_data_dir()
            .context("Could not get default lexe data dir")?;
        let path = wallet_env.seedphrase_path(&lexe_data_dir);
        Self::read_from_path(&path)
    }

    /// Write this [`RootSeed`] to the default seedphrase path for this
    /// environment (`~/.lexe/seedphrase[.env].txt`).
    ///
    /// Creates parent directories if needed. Fails if the file already exists.
    pub fn write(&self, wallet_env: &WalletEnv) -> anyhow::Result<()> {
        let lexe_data_dir = lexe_common::default_lexe_data_dir()
            .context("Could not get default lexe data dir")?;
        let path = wallet_env.seedphrase_path(&lexe_data_dir);
        self.write_to_path(&path)
    }

    /// Read a [`RootSeed`] from a seedphrase file at a specific path.
    ///
    /// Returns `Ok(None)` if the file doesn't exist.
    pub fn read_from_path(path: &Path) -> anyhow::Result<Option<Self>> {
        UnstableRootSeed::read_from_path(path)
            .map(|maybe_root_seed| maybe_root_seed.map(Self))
    }

    /// Write this [`RootSeed`] to a seedphrase file at a specific path.
    ///
    /// Creates parent directories if needed. Returns an error if the file
    /// already exists. On Unix, the file is created with mode 0600 (owner
    /// read/write only).
    pub fn write_to_path(&self, path: &Path) -> anyhow::Result<()> {
        self.unstable().write_to_path(path)
    }

    /// Construct a [`RootSeed`] from a BIP39 mnemonic.
    pub fn from_mnemonic(mnemonic: Mnemonic) -> anyhow::Result<Self> {
        Self::try_from(mnemonic)
    }

    /// Construct a [`RootSeed`] from a 32-byte slice.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        Self::try_from(bytes)
    }

    /// Construct a [`RootSeed`] from a 64-character hex string.
    pub fn from_hex(hex: &str) -> anyhow::Result<Self> {
        Self::from_str(hex).map_err(anyhow::Error::from)
    }

    // --- Serialization --- //

    /// Convert this root secret to its BIP39 mnemonic.
    pub fn to_mnemonic(&self) -> Mnemonic {
        self.unstable().to_mnemonic()
    }

    /// Borrow the 32-byte root secret.
    pub fn as_bytes(&self) -> &[u8] {
        self.unstable().expose_secret()
    }

    /// Encode the root secret as a 64-character hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.as_bytes())
    }

    // --- Derived Identity --- //
    /// Derive the user's public key.
    pub fn derive_user_pk(&self) -> UserPk {
        self.unstable().derive_user_pk()
    }

    /// Derive the node public key.
    pub fn derive_node_pk(&self) -> NodePk {
        self.unstable().derive_node_pk()
    }

    // --- Encryption --- //

    /// Encrypt this root secret under the given password.
    pub fn password_encrypt(&self, password: &str) -> anyhow::Result<Vec<u8>> {
        self.unstable()
            .password_encrypt(&mut SysRng::new(), password)
    }

    /// Decrypt a password-encrypted root secret.
    pub fn password_decrypt(
        password: &str,
        encrypted: Vec<u8>,
    ) -> anyhow::Result<Self> {
        UnstableRootSeed::password_decrypt(password, encrypted).map(Self)
    }

    // --- Internal Escape Hatches --- //

    cfg_if::cfg_if! {
        if #[cfg(feature = "unstable")] {
            /// Returns the wrapped internal root-seed type.
            ///
            /// This is only exposed when the `unstable` feature is enabled.
            pub fn unstable(&self) -> &UnstableRootSeed {
                &self.0
            }
        } else {
            pub(crate) fn unstable(&self) -> &UnstableRootSeed {
                &self.0
            }
        }
    }

    cfg_if::cfg_if! {
        if #[cfg(feature = "unstable")] {
            /// Destructure this SDK root seed into the internal root-seed type.
            ///
            /// This is only exposed when the `unstable` feature is enabled.
            pub fn into_unstable(self) -> UnstableRootSeed {
                self.0
            }
        } else {
            pub(crate) fn into_unstable(self) -> UnstableRootSeed {
                self.0
            }
        }
    }
}

impl fmt::Debug for RootSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.unstable().fmt(f)
    }
}

impl FromStr for RootSeed {
    type Err = <UnstableRootSeed as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UnstableRootSeed::from_str(s).map(Self)
    }
}

impl TryFrom<&[u8]> for RootSeed {
    type Error = anyhow::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        UnstableRootSeed::try_from(bytes).map(Self)
    }
}

impl TryFrom<Mnemonic> for RootSeed {
    type Error = anyhow::Error;

    fn try_from(mnemonic: Mnemonic) -> Result<Self, Self::Error> {
        UnstableRootSeed::try_from(mnemonic).map(Self)
    }
}

// --- ClientCredentials --- //

/// Scoped and revocable credentials for controlling a Lexe user node.
///
/// These are useful when you want node access without exposing the user's
/// [`RootSeed`], which is irrevocable.
#[derive(Clone)]
pub struct ClientCredentials(UnstableClientCredentials);

impl ClientCredentials {
    /// Parse [`ClientCredentials`] from a string.
    pub fn from_string(s: &str) -> anyhow::Result<Self> {
        Self::from_str(s)
    }

    /// Export these credentials as a portable string.
    ///
    /// The returned string can be passed to [`ClientCredentials::from_string`]
    /// to reconstruct the credentials.
    pub fn export_string(&self) -> String {
        self.to_string()
    }

    /// Access the inner [`UnstableClientCredentials`].
    pub(crate) fn unstable(&self) -> &UnstableClientCredentials {
        &self.0
    }
}

impl FromStr for ClientCredentials {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UnstableClientCredentials::try_from_base64_blob(s).map(Self)
    }
}

impl fmt::Display for ClientCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.unstable().to_base64_blob())
    }
}
