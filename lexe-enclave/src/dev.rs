//! Post-build dev version/measurement patching for SGX enclaves in testing.
//!
//! With this, `sgx-builder` only needs to compile and link each enclave once.
//! It can then copy the binary and patch the `Identity::PLACEHOLDER` with the
//! dev version and measurement. This produces distinct enclave measurements
//! without rebuilding and relinking the enclave for every test version.
//!
//! Everything except the `Identity` encoding (needed by `sgx-builder`) is gated
//! on the `test-utils` feature: production enclaves contain no patch slot and
//! always report a `None` dev version and dev measurement.

#[cfg(feature = "test-utils")]
use std::sync::LazyLock;

use lexe_byte_array::ByteArray;

use crate::types::Measurement;

/// Returns the dev version patched into this enclave binary.
pub fn version() -> Option<&'static str> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "test-utils")] {
            DEV_IDENTITY.version()
        } else {
            None
        }
    }
}

/// Returns the dev measurement patched into the enclave binary.
///
/// Only effective outside SGX in test/simulation binaries.
#[cfg(not(target_env = "sgx"))]
pub(crate) fn measurement() -> Option<Measurement> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "test-utils")] {
            DEV_IDENTITY.measurement()
        } else {
            None
        }
    }
}

/// The dev enclave identity read once from this binary's patch slot.
#[cfg(feature = "test-utils")]
static DEV_IDENTITY: LazyLock<Identity> =
    LazyLock::new(Identity::read_patch_slot);

/// An encoded dev enclave identity (dev version + dev measurement), as
/// patched into a compiled binary. Possibly still the unpatched placeholder.
pub struct Identity([u8; Self::LEN]);

impl Identity {
    /// Length of the patch slot in bytes.
    pub const LEN: usize = 64;

    /// Bytes which `sgx-builder` replaces after compiling a dev enclave.
    pub const PLACEHOLDER: Identity = Identity(
        *b"\xffLEXE_DEV_ENCLAVE_IDENTITY_PATCH_SLOT_REPLACE_ME__0123456789ABCD",
    );

    /// ELF section which lets `sgx-builder` locate the patch slot.
    //
    // Keep in sync with the `read_patch_slot()` attribute literals.
    pub const SECTION_ELF: &'static str = ".lexe_dev_id";

    /// Mach-O equivalent of [`Identity::SECTION_ELF`], for non-SGX dev builds
    /// on macOS. Lives in the `__DATA` segment.
    //
    // Keep in sync with the `read_patch_slot()` attribute literals.
    pub const SECTION_MACHO: &'static str = "__lexe_dev_id";

    const MEASUREMENT_OFFSET: usize = 32;
    const VERSION_MAX_LEN: usize = Self::MEASUREMENT_OFFSET - 1;
    const VERSION_OFFSET: usize = 1;

    /// Encodes a dev enclave identity which `sgx-builder` writes over
    /// [`Identity::PLACEHOLDER`] in a copied binary.
    ///
    /// Panics if `version` is empty or too long to fit in the patch slot.
    pub fn encode(version: &str, measurement: Measurement) -> Self {
        assert!(!version.is_empty(), "Dev version is empty");
        let max_len = Self::VERSION_MAX_LEN;
        assert!(
            version.len() <= max_len,
            "Dev version exceeds {max_len} bytes: {version}"
        );

        let mut encoded = [0; Self::LEN];
        encoded[0] = version.len() as u8;
        encoded[Self::VERSION_OFFSET..][..version.len()]
            .copy_from_slice(version.as_bytes());
        encoded[Self::MEASUREMENT_OFFSET..]
            .copy_from_slice(measurement.as_slice());
        Self(encoded)
    }

    pub fn as_bytes(&self) -> &[u8; Self::LEN] {
        &self.0
    }
}

/// Parsing and reading the patched identity. Not available in production
/// enclaves, which contain no patch slot.
#[cfg(any(test, feature = "test-utils"))]
impl Identity {
    const UNPATCHED_VERSION_LEN: u8 = 0xff;

    /// Returns the dev version, or `None` if unpatched.
    fn version(&self) -> Option<&str> {
        self.is_patched().then(|| {
            let version_len = usize::from(self.0[0]);
            assert!(
                (1..=Self::VERSION_MAX_LEN).contains(&version_len),
                "Invalid patched dev version length: {version_len}"
            );
            std::str::from_utf8(&self.0[Self::VERSION_OFFSET..][..version_len])
                .expect("Patched dev version is not UTF-8")
        })
    }

    /// Returns the dev measurement, or `None` if unpatched.
    #[cfg(any(test, not(target_env = "sgx")))]
    fn measurement(&self) -> Option<Measurement> {
        self.is_patched().then(|| {
            let measurement: [u8; 32] =
                self.0[Self::MEASUREMENT_OFFSET..].try_into().unwrap();
            Measurement::new(measurement)
        })
    }

    fn is_patched(&self) -> bool {
        self.0[0] != Self::UNPATCHED_VERSION_LEN
    }

    /// Reads the bytes which `sgx-builder` patches after linking.
    #[cfg(feature = "test-utils")] // Not used by lexe-enclave unit tests
    #[inline(never)]
    fn read_patch_slot() -> Self {
        #[used]
        // Keep in sync with `Identity::SECTION_ELF` / `SECTION_MACHO`.
        // Assume ELF normally.
        #[cfg_attr(
            not(target_vendor = "apple"),
            unsafe(link_section = ".lexe_dev_id")
        )]
        // For Apple Mach-O binaries, we also need to specify the segment first.
        #[cfg_attr(
            target_vendor = "apple",
            unsafe(link_section = "__DATA,__lexe_dev_id")
        )]
        static mut PATCH_SLOT: [u8; Identity::LEN] = Identity::PLACEHOLDER.0;

        // SAFETY: The patch slot is never mutated while the process runs, and
        // the byte array is `Copy`. `static mut` prevents this non-inlined read
        // from being replaced with the placeholder.
        Self(unsafe { std::ptr::read(&raw mut PATCH_SLOT) })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn encode_roundtrip() {
        let version = "0.0.0-dev.1";
        let measurement = Measurement::new([42; 32]);

        let id = Identity::encode(version, measurement);

        assert_eq!(id.version(), Some(version));
        assert_eq!(id.measurement(), Some(measurement));
    }

    #[test]
    fn unpatched_placeholder() {
        assert!(Identity::PLACEHOLDER.version().is_none());
        assert!(Identity::PLACEHOLDER.measurement().is_none());
    }

    /// The patch slot in this test binary should be unpatched.
    #[cfg(feature = "test-utils")]
    #[test]
    fn unpatched_slot() {
        assert!(version().is_none());
    }
}
