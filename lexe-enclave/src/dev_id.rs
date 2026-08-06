//! Post-build version/measurement patching for SGX enclaves in testing.
//!
//! With this, `sgx-builder` only needs to compile and link each enclave once.
//! It can then copy the binary and patch the `PATCH_SLOT_PLACEHOLDER` with
//! the dev version and measurement. This produces distinct enclave measurements
//! without rebuilding and relinking the enclave for every test version.

use lexe_byte_array::ByteArray;

use crate::types::Measurement;

/// Bytes which `sgx-builder` replaces after compiling a dev enclave.
pub const PATCH_SLOT_PLACEHOLDER: [u8; PATCH_SLOT_LEN] =
    *b"\xffLEXE_DEV_ENCLAVE_IDENTITY_PATCH_SLOT_REPLACE_ME__0123456789ABCD";

/// Returns the version patched into this enclave binary.
pub fn version() -> Option<&'static str> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "test-utils")] {
            patch::version()
        } else {
            None
        }
    }
}

/// Returns the measurement patched into the enclave binary.
///
/// Only effective outside SGX in test/simulation binaries.
#[cfg(not(target_env = "sgx"))]
pub(crate) fn measurement() -> Option<Measurement> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "test-utils")] {
            patch::measurement()
        } else {
            None
        }
    }
}

/// Encodes a dev enclave identity for patching into a compiled binary. This
/// gets written into `PATCH_SLOT` below.
pub fn encode(version: &str, measurement: Measurement) -> [u8; PATCH_SLOT_LEN] {
    assert!(!version.is_empty(), "Dev version is empty");
    assert!(
        version.len() <= VERSION_MAX_LEN,
        "Dev version exceeds {VERSION_MAX_LEN} bytes: {version}"
    );

    let mut encoded = [0; PATCH_SLOT_LEN];
    encoded[0] = version.len() as u8;
    encoded[VERSION_OFFSET..][..version.len()]
        .copy_from_slice(version.as_bytes());
    encoded[MEASUREMENT_OFFSET..].copy_from_slice(measurement.as_slice());
    encoded
}

const MEASUREMENT_OFFSET: usize = 32;
const PATCH_SLOT_LEN: usize = 64;
const VERSION_MAX_LEN: usize = MEASUREMENT_OFFSET - 1;
const VERSION_OFFSET: usize = 1;

#[cfg(feature = "test-utils")]
mod patch {
    use std::sync::LazyLock;

    use super::*;

    const UNPATCHED_VERSION_LEN: u8 = 0xff;

    static DEV_ID: LazyLock<DevId> = LazyLock::new(DevId::read);

    pub(super) fn version() -> Option<&'static str> {
        DEV_ID.is_patched().then(|| DEV_ID.version())
    }

    #[cfg(not(target_env = "sgx"))]
    pub(super) fn measurement() -> Option<Measurement> {
        DEV_ID
            .is_patched()
            .then(|| Measurement::new(*DEV_ID.measurement()))
    }

    /// The unparsed value read out of the `PATCH_SLOT`.
    struct DevId([u8; PATCH_SLOT_LEN]);

    impl DevId {
        fn read() -> Self {
            Self(patch_slot())
        }

        fn is_patched(&self) -> bool {
            self.0[0] != UNPATCHED_VERSION_LEN
        }

        fn version(&self) -> &str {
            let version_len = usize::from(self.0[0]);
            assert!(
                (1..=VERSION_MAX_LEN).contains(&version_len),
                "Invalid patched dev version length: {version_len}"
            );
            std::str::from_utf8(&self.0[VERSION_OFFSET..][..version_len])
                .expect("Patched dev version is not UTF-8")
        }

        #[cfg(not(target_env = "sgx"))]
        fn measurement(&self) -> &[u8; 32] {
            self.0[MEASUREMENT_OFFSET..].try_into().unwrap()
        }
    }

    /// Reads the bytes which `sgx-builder` patches after linking.
    #[inline(never)]
    fn patch_slot() -> [u8; PATCH_SLOT_LEN] {
        #[used]
        static mut PATCH_SLOT: [u8; PATCH_SLOT_LEN] = PATCH_SLOT_PLACEHOLDER;

        // SAFETY: The patch slot is never mutated while the process runs, and
        // the byte array is `Copy`. `static mut` prevents this non-inlined read
        // from being replaced with the placeholder.
        unsafe { std::ptr::read(&raw mut PATCH_SLOT) }
    }

    #[cfg(all(test, feature = "test-utils"))]
    mod test {
        use super::*;

        #[test]
        fn parses_encoded_dev_id() {
            let version = "0.0.0-dev.1";
            let measurement = Measurement::new([42; 32]);

            let dev_id = DevId(encode(version, measurement));

            assert!(dev_id.is_patched());
            assert_eq!(dev_id.version(), version);
            assert_eq!(Measurement::new(*dev_id.measurement()), measurement);
        }

        #[test]
        fn unpatched_placeholder() {
            assert!(!DevId::read().is_patched());
        }
    }
}
