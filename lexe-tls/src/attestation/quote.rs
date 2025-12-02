//! custom extension, which we'll embed into our remote attestation cert.
//! Get a quote for the running node enclave and return it as an x509 cert
//!
//! On non-SGX platforms, we just return a dummy extension for now.

#[rustfmt::skip]
#[cfg(target_env = "sgx")]
pub use sgx::quote_enclave;
use common::ed25519;
#[cfg(not(target_env = "sgx"))]
pub use not_sgx::quote_enclave;

/// Small newtype for [`sgx_isa::Report::reportdata`] field.
/// For now, we only use the first 32 out of 64 bytes to commit to a cert pk.
#[derive(Debug)]
pub struct ReportData([u8; 64]);

impl ReportData {
    /// Construct from the `reportdata` of an existing [`sgx_isa::Report`].
    pub fn new(reportdata: [u8; 64]) -> Self {
        Self(reportdata)
    }

    pub fn from_cert_pk(pk: &ed25519::PublicKey) -> Self {
        let mut report_data = [0u8; 64];
        // ed25519 pks are always 32 bytes.
        // This will panic if this internal invariant is somehow not true.
        report_data[..32].copy_from_slice(pk.as_ref());
        Self(report_data)
    }

    pub fn as_inner(&self) -> &[u8; 64] {
        &self.0
    }

    pub fn into_inner(self) -> [u8; 64] {
        self.0
    }

    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// Whether the first 32 bytes contains the given [`ed25519::PublicKey`].
    pub fn contains(&self, cert_pk: &ed25519::PublicKey) -> bool {
        &self.0[..32] == cert_pk.as_slice()
    }
}

#[cfg(target_env = "sgx")]
mod sgx {
    use std::{borrow::Cow, fmt, net::TcpStream};

    use aes::Aes128;
    use aesm_client::{AesmClient, sgx::AesmClientExt};
    use anyhow::{Context, ensure, format_err};
    use bytemuck::{Pod, Zeroable};
    use cmac::{Cmac, Mac, digest::generic_array::GenericArray};
    use common::rng::{Crng, RngExt};
    use sgx_isa::{Report, Targetinfo};

    use super::*;
    use crate::attestation::{
        cert::SgxAttestationExtension, verifier::EnclavePolicy,
    };

    #[cfg(all(not(target_feature = "aes"), not(doc)))]
    std::compile_error!(
        "Intel AES-NI intrinsics must be enabled at compile time via RUSTFLAGS"
    );

    /// TODO(max): Needs docs
    pub fn quote_enclave(
        mut rng: &mut dyn Crng,
        cert_pk: &ed25519::PublicKey,
    ) -> anyhow::Result<SgxAttestationExtension<'static>> {
        // TODO(phlip9): AESM retries

        // 1. Connect to the local AESM service.

        // NOTE: By default, the AESM service only listens on a local unix
        // socket. To make this work, the `run-sgx` enclave runner hooks a small
        // TCP <-> AESM socket proxy for enclave TCP connections on the special
        // "aesm.local" DNS name.

        let aesm_sock = TcpStream::connect("aesm.local")
            .context("Failed to connect to AESM service")?;
        let aesm_client = AesmClient::new(aesm_sock);

        // 2. Get the ECDSA-P256 Attestation Key Id (pk hash)

        let supported_key_ids = aesm_client
            .get_supported_att_key_ids()
            .map_err(ErrString::new)
            .context("Failed to get supported attestation key ids from AESM")?;
        let key_id_buf = supported_key_ids
            .into_iter()
            .find_map(|key_id_buf| {
                let key_id_ref =
                    bytemuck::try_from_bytes::<QlAttKeyIdExt>(&key_id_buf)
                        .ok()?;
                if key_id_ref.is_ecdsa_p256() {
                    Some(key_id_buf)
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                format_err!(
                    "AESM doesn't appear to support ECDSA-P256 Attestation"
                )
            })?;

        // 3. Get the Targetinfo from the Quoting Enclave (QE)
        //
        // Here we're asking the Intel Quoting Enclave (QE) to give us its
        // enclave measurement (and misc attributes). The QE will expect our
        // Report to be bound to its enclave info.

        // NOTE: When run on a cold machine, this can take ~1 sec since azure
        // DCAP needs to fetch the platform certs (?). Subsequent calls only
        // take ~700 us as the PCK cert cache is hot.
        let qe_quote_info = aesm_client
            .init_quote_ex(key_id_buf.clone())
            .map_err(|err| {
                format_err!(
                    "failed to get the Quoting Enclave (QE) QuoteInfo: {err:#}"
                )
            })?;

        // 4. Build our enclave Report
        //
        // Bind the cert pk and QE Targetinfo to our enclave Report. When
        // the verifier checks the attestation evidence, this linkage is
        // what allows them to then trust the associated certificate.

        let report_data = ReportData::from_cert_pk(cert_pk);
        let qe_target_info =
            Targetinfo::try_copy_from(qe_quote_info.target_info())
                .context("Failed to deserialize QE Quote Targetinfo")?;
        let report =
            Report::for_target(&qe_target_info, report_data.as_inner());

        // 5. Get enclave Report Quoted

        // Sample a nonce for this request to protect against replay attacks.
        let req_nonce = rng.gen_bytes::<16>().to_vec();

        let report_ref: &[u8] = report.as_ref();
        let quote_res = aesm_client
            .get_quote_ex(
                key_id_buf,
                report_ref.to_vec(),
                None, // If None, use the Targetinfo embedded in the Report
                req_nonce.clone(),
            )
            .map_err(ErrString::new)
            .context("Failed to make Quote request to AESM")?;

        // 6. Verify the Quote was generated by a trusted QE and contains the
        //    expected data.

        let qe_report = sgx_isa::Report::try_copy_from(quote_res.qe_report())
            .context("AESM or QE returned an invalid Report")?;

        // TODO(phlip9): request QE identity from IAC?
        let qe_reportdata = EnclavePolicy::trust_intel_qe()
            .verify(&qe_report)
            .context("Invalid QE identity")?;

        // Verify the QE Report was produced by the QE in another local enclave
        let is_valid_mac = qe_report.verify(|key, report, mac| {
            let key = GenericArray::from(*key);
            let mac = GenericArray::from(*mac);
            let mut cmac = Cmac::<Aes128>::new(&key);
            cmac.update(report);
            cmac.verify(&mac).is_ok()
        });
        ensure!(is_valid_mac, "QE Report MAC failed to verify");

        // The QE ReportData field should commit to our nonce (to ensure
        // freshness) and our Quote (to ensure the Quote was gen'd in an
        // enclave).

        let h_nonce_quote =
            sha256::digest_many(&[&req_nonce, quote_res.quote()]);
        let mut expected_reportdata = [0u8; 64];
        expected_reportdata[0..32].copy_from_slice(h_nonce_quote.as_ref());

        ensure!(
            &expected_reportdata == qe_reportdata.as_inner(),
            "QE ReportData doesn't match the expected value: actual: '{}', expected: '{}'",
            hex::display(qe_reportdata.as_slice()),
            hex::display(expected_reportdata.as_slice()),
        );

        // TODO(phlip9): check that quote actually contains our report?

        // 7. Collect the attestation evidence into an x509 cert extension

        Ok(SgxAttestationExtension {
            quote: Cow::Owned(quote_res.quote().to_vec()),
        })
    }

    // dumb error type compatibility hack

    #[derive(Debug)]
    struct ErrString(String);

    impl ErrString {
        fn new(err: impl fmt::Display) -> Self {
            Self(format!("{err:#}"))
        }
    }

    impl fmt::Display for ErrString {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(&self.0)
        }
    }

    impl std::error::Error for ErrString {
        fn description(&self) -> &str {
            &self.0
        }
    }

    #[allow(clippy::empty_line_after_outer_attr)]
    #[rustfmt::skip]
    // // C struct definitions from:
    // // <https://github.com/intel/linux-sgx/blob/master/common/inc/sgx_quote.h>
    //
    // typedef enum {
    //     SGX_QL_ALG_EPID = 0,       ///< EPID 2.0 - Anonymous
    //     SGX_QL_ALG_RESERVED_1 = 1, ///< Reserved
    //     SGX_QL_ALG_ECDSA_P256 = 2, ///< ECDSA-256-with-P-256 curve, Non - Anonymous
    //     SGX_QL_ALG_ECDSA_P384 = 3, ///< ECDSA-384-with-P-384 curve (Note: currently not supported), Non-Anonymous
    //     SGX_QL_ALG_MAX = 4
    // } sgx_ql_attestation_algorithm_id_t;
    //
    // typedef struct _sgx_ql_att_key_id_t {
    //     uint16_t    id;                              ///< Structure ID
    //     uint16_t    version;                         ///< Structure version
    //     uint16_t    mrsigner_length;                 ///< Number of valid bytes in MRSIGNER.
    //     uint8_t     mrsigner[48];                    ///< SHA256 or SHA384 hash of the Public key that signed the QE.
    //                                                  ///< The lower bytes contain MRSIGNER.  Bytes beyond mrsigner_length '0'
    //     uint32_t    prod_id;                         ///< Legacy Product ID of the QE
    //     uint8_t     extended_prod_id[16];            ///< Extended Product ID or the QE. All 0's for legacy format enclaves.
    //     uint8_t     config_id[64];                   ///< Config ID of the QE.
    //     uint8_t     family_id[16];                   ///< Family ID of the QE.
    //     uint32_t    algorithm_id;                    ///< Identity of the attestation key algorithm.
    // } sgx_ql_att_key_id_t;
    //
    // typedef struct _sgx_att_key_id_ext_t {
    //     sgx_ql_att_key_id_t base;
    //     uint8_t             spid[16];                ///< Service Provider ID, should be 0s for ECDSA quote
    //     uint16_t            att_key_type;            ///< For non-EPID quote, it should be 0
    //                                                  ///< For EPID quote, it equals to sgx_quote_sign_type_t
    //     uint8_t             reserved[80];            ///< It should have the same size of sgx_att_key_id_t
    // } sgx_att_key_id_ext_t;

    /// ECDSA-256-with-P-256 curve, Non-Anonymous
    pub(crate) const SGX_QL_ALG_ECDSA_P256: u32 = 2;

    /// An extended SGX attestation key.
    ///
    /// Mirrors the C struct above [`sgx_quote.h/sgx_att_key_id_ext_t`](https://github.com/intel/linux-sgx/blob/master/common/inc/sgx_quote.h#L127).
    ///
    /// This struct needs `repr(C, packed)` for the memory layout to match the C
    /// definition. The extra `packed` modifier is necessary, otherwise the
    /// standard alignment causes some extra padding between fields, which
    /// makes the struct larger than the original C struct.
    #[repr(C, packed)]
    #[derive(Copy, Clone, Pod, Zeroable)]
    pub(crate) struct QlAttKeyIdExt {
        // The original attestation key id type, `sgx_ql_att_key_id_t`.
        /// Structure ID
        pub id: u16,
        /// Structure Version
        pub version: u16,
        /// Number of valid bytes in `mrsigner`
        pub mrsigner_len: u16,
        /// SHA256 or SHA384 hash of the pk that signed the QE
        pub mrsigner: [u8; 48],

        /// Legacy Product ID of the QE
        pub prod_id: u32,
        /// Extended Product ID of the QE
        pub extended_prod_id: [u8; 16],
        /// Config ID of the QE
        pub config_id: [u8; 64],
        /// Family ID of the QE
        pub family_id: [u8; 16],
        /// The attestation key algorithm ID
        pub algorithm_id: u32,

        // The extended attestation key id type, `sgx_att_key_id_ext_t`.
        /// Service Provider ID, should be 0s for ECDSA quote
        pub spid: [u8; 16],
        /// For non-EPID quote, it should be 0
        /// For EPID quote, it equals `sgx_quote_sign_type_t`
        pub att_key_type: u16,
        /// Padding so this struct has the same size as `sgx_att_key_id_t`
        /// (read: 256 bytes).
        reserved: [u8; 80],
    }

    // Statically guarantee that the `QlAttKeyIdExt` struct is exactly 256 bytes
    // in size.
    const _: [(); 256] = [(); std::mem::size_of::<QlAttKeyIdExt>()];

    impl QlAttKeyIdExt {
        pub fn is_ecdsa_p256(&self) -> bool {
            self.algorithm_id == SGX_QL_ALG_ECDSA_P256
        }
    }
}

#[cfg(not(target_env = "sgx"))]
mod not_sgx {
    use common::rng::Crng;

    use super::*;
    use crate::attestation::cert::SgxAttestationExtension;

    pub fn quote_enclave(
        _rng: &mut dyn Crng,
        cert_pk: &ed25519::PublicKey,
    ) -> anyhow::Result<SgxAttestationExtension<'static>> {
        Ok(SgxAttestationExtension::dummy(cert_pk))
    }
}
