//! # Provisioning a new lexe node
//!
//! This module is responsible for running the node provisioning process for new
//! users and for existing users upgrading to new enclave versions.
//!
//! The intention of the provisioning process is for users to transfer their
//! secure secrets into a trusted enclave version with the operator (lexe)
//! learning their secrets. These secrets include sensitive data like wallet
//! private keys or mTLS certificates.
//!
//! A node enclave must also convince the user that the software is a version
//! that they trust and the software is running inside an up-to-date secure
//! enclave. We do this using a variant of RA-TLS (Remote Attestation TLS),
//! where the enclave platform endorsements and enclave measurements are bundled
//! into a self-signed TLS certificate, which users must verify when connecting
//! to the provisioning endpoint.

struct UserId(i64);

/// Provision a new lexe node
///
/// Both `userid` and `auth_token` are given by the orchestrator so we know
/// which user we should provision to and have a simple method to authenticate
/// their connection.
pub fn provision(dns_name: String, _userid: UserId, _auth_token: String) {
    // Q: we could wait to init cert + TLS until we've gotten a TCP connection?

    // # sgx
    //
    // 1. self report
    // 2. sample cert keypair
    // 3. aesm client
    // 4. get QE attestation key
    // 5. get QE QuoteInfo
    // 6. get Report binding cert pubkey hash
    // 7. get enclave Report quoted -> Quote
    // 8. (?) verify Quote - could be fake AESM, but user would just reject
    // anyway? 9. build enclave cert
    // 10. bind tcp listener
    // 11. orchestrator readiness ping
    // 12.

    // # local
    // 1. gen self-signed cert w/ fake quote
    // 2. listen
}
