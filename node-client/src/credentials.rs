//! Client credentials for authentication with Lexe services.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use base64::Engine;
use common::{
    api::{
        auth::{BearerAuthToken, Scope},
        revocable_clients::CreateRevocableClientResponse,
        user::UserPk,
    },
    ed25519,
    env::DeployEnv,
    rng::Crng,
    root_seed::RootSeed,
};
use lexe_api::auth::BearerAuthenticator;
use lexe_tls::{
    rustls, shared_seed,
    types::{LxCertificateDer, LxPrivatePkcs8KeyDer},
};
#[cfg(any(test, feature = "test-utils"))]
use proptest::{prelude::any, strategy::Strategy};
#[cfg(any(test, feature = "test-utils"))]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// Credentials required to connect to a user node via mTLS.
pub enum Credentials {
    /// Using a [`RootSeed`]. Ex: app.
    RootSeed(RootSeed),
    /// Using a revocable client cert. Ex: SDK sidecar.
    ClientCredentials(ClientCredentials),
}

/// Borrowed credentials required to connect to a user node via mTLS.
pub enum CredentialsRef<'a> {
    /// Using a [`RootSeed`]. Ex: app.
    RootSeed(&'a RootSeed),
    /// Using a revocable client cert. Ex: SDK sidecar.
    ClientCredentials(&'a ClientCredentials),
}

/// All secrets required for a non-RootSeed client to authenticate and
/// communicate with a user's node.
///
/// This is exposed to users as a base64-encoded JSON blob.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(
    any(test, feature = "test-utils"),
    derive(Arbitrary, Debug, Eq, PartialEq)
)]
pub struct ClientCredentials {
    /// The user public key.
    ///
    /// Always `Some(_)` if the credentials were created by `node-v0.8.11+`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(
        any(test, feature = "test-utils"),
        proptest(strategy = "any::<UserPk>().prop_map(Some)")
    )]
    pub user_pk: Option<UserPk>,
    /// The base64 encoded long-lived connect token.
    pub lexe_auth_token: BearerAuthToken,
    /// The hex-encoded client public key.
    pub client_pk: ed25519::PublicKey,
    /// The DER-encoded client key.
    pub rev_client_key_der: LxPrivatePkcs8KeyDer,
    /// The DER-encoded cert of the revocable client.
    pub rev_client_cert_der: LxCertificateDer,
    /// The DER-encoded cert of the ephemeral issuing CA.
    pub eph_ca_cert_der: LxCertificateDer,
}

// --- impl Credentials / CredentialsRef --- //

impl Credentials {
    pub fn as_ref(&self) -> CredentialsRef<'_> {
        match self {
            Credentials::RootSeed(root_seed) =>
                CredentialsRef::RootSeed(root_seed),
            Credentials::ClientCredentials(client_credentials) =>
                CredentialsRef::ClientCredentials(client_credentials),
        }
    }
}

impl From<RootSeed> for Credentials {
    fn from(root_seed: RootSeed) -> Self {
        Credentials::RootSeed(root_seed)
    }
}

impl From<ClientCredentials> for Credentials {
    fn from(client_credentials: ClientCredentials) -> Self {
        Credentials::ClientCredentials(client_credentials)
    }
}

impl<'a> From<&'a RootSeed> for CredentialsRef<'a> {
    fn from(root_seed: &'a RootSeed) -> Self {
        CredentialsRef::RootSeed(root_seed)
    }
}

impl<'a> From<&'a ClientCredentials> for CredentialsRef<'a> {
    fn from(client_credentials: &'a ClientCredentials) -> Self {
        CredentialsRef::ClientCredentials(client_credentials)
    }
}

impl<'a> CredentialsRef<'a> {
    /// Returns the user public key.
    ///
    /// Always `Some(_)` if the credentials were created by `node-v0.8.11+`.
    pub fn user_pk(&self) -> Option<UserPk> {
        match self {
            Self::RootSeed(root_seed) => Some(root_seed.derive_user_pk()),
            Self::ClientCredentials(cc) => cc.user_pk,
        }
    }

    /// Create a [`BearerAuthenticator`] appropriate for the given credentials.
    ///
    /// Currently limits to [`Scope::NodeConnect`] for [`RootSeed`] credentials.
    pub fn bearer_authenticator(&self) -> Arc<BearerAuthenticator> {
        match self {
            Self::RootSeed(root_seed) => {
                let maybe_cached_token = None;
                Arc::new(BearerAuthenticator::new_with_scope(
                    root_seed.derive_user_key_pair(),
                    maybe_cached_token,
                    Some(Scope::NodeConnect),
                ))
            }
            Self::ClientCredentials(client_credentials) =>
                Arc::new(BearerAuthenticator::new_static_token(
                    client_credentials.lexe_auth_token.clone(),
                )),
        }
    }

    /// Build a TLS client config appropriate for the given credentials.
    pub fn tls_config(
        &self,
        rng: &mut impl Crng,
        deploy_env: DeployEnv,
    ) -> anyhow::Result<rustls::ClientConfig> {
        match self {
            Self::RootSeed(root_seed) =>
                shared_seed::app_node_run_client_config(
                    rng, deploy_env, root_seed,
                )
                .context("Failed to build RootSeed TLS client config"),
            Self::ClientCredentials(client_credentials) =>
                shared_seed::sdk_node_run_client_config(
                    deploy_env,
                    &client_credentials.eph_ca_cert_der,
                    client_credentials.rev_client_cert_der.clone(),
                    client_credentials.rev_client_key_der.clone(),
                )
                .context("Failed to build revocable client TLS config"),
        }
    }
}

// --- impl ClientCredentials --- //

impl FromStr for ClientCredentials {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from_base64_blob(s)
    }
}

impl ClientCredentials {
    pub fn from_response(
        lexe_auth_token: BearerAuthToken,
        resp: CreateRevocableClientResponse,
    ) -> Self {
        ClientCredentials {
            user_pk: resp.user_pk,
            lexe_auth_token,
            client_pk: resp.pubkey,
            rev_client_key_der: LxPrivatePkcs8KeyDer(
                resp.rev_client_cert_key_der,
            ),
            rev_client_cert_der: LxCertificateDer(resp.rev_client_cert_der),
            eph_ca_cert_der: LxCertificateDer(resp.eph_ca_cert_der),
        }
    }

    /// Encodes a [`ClientCredentials`] to a base64 blob using
    /// [`base64::engine::general_purpose::STANDARD_NO_PAD`].
    //
    // We use `STANDARD_NO_PAD` because trailing `=`s cause problems with
    // autocomplete on iPhone. For example, if the base64 string ends with:
    //
    // - `NzB2mIn0=`
    // - `NzBm2In0=`
    //
    // the iPhone autocompletes it to the following respectively when pasted
    // into iMessage, even if you 'tap away' to reject the suggestion:
    //
    // - `NzB2mIn0=120 secs`
    // - `NzBm2In0=0 in`
    pub fn to_base64_blob(&self) -> String {
        let json_str =
            serde_json::to_string(self).expect("Failed to JSON serialize");
        base64::engine::general_purpose::STANDARD_NO_PAD
            .encode(json_str.as_bytes())
    }

    /// Decodes a [`ClientCredentials`] from a base64 blob encoded with either
    /// [`base64::engine::general_purpose::STANDARD`] or
    /// [`base64::engine::general_purpose::STANDARD_NO_PAD`].
    //
    // NOTE: This function accepts `STANDARD` encodings because historical
    // client credentials were encoded with the `STANDARD` engine until we
    // discovered that iPhones interpret the trailing `=` as part of a unit
    // conversion, resulting in unintended autocompletions.
    pub fn try_from_base64_blob(s: &str) -> anyhow::Result<Self> {
        let s = s.trim().trim_end_matches('=');
        let bytes = base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(s)
            .context("String is not valid base64")?;
        let string =
            String::from_utf8(bytes).context("String is not valid UTF-8")?;
        serde_json::from_str(&string).context("Failed to deserialize")
    }
}

#[cfg(test)]
mod test {
    use std::fs;

    use common::{
        byte_str::ByteStr,
        rng::FastRng,
        test_utils::{arbitrary, snapshot},
    };
    use lexe_tls::shared_seed::certs::{
        EphemeralIssuingCaCert, RevocableClientCert, RevocableIssuingCaCert,
    };
    use proptest::{prelude::any, prop_assert_eq, proptest};

    use super::*;

    /// Tests [`ClientCredentials`] roundtrip to/from base64.
    ///
    /// We also test compatibility: client credentials encoded with the old
    /// STANDARD engine can be decoded with the new try_from_base64_blob method
    /// which should accept both STANDARD and STANDARD_NO_PAD.
    #[test]
    fn prop_client_credentials_base64_roundtrip() {
        proptest!(|(creds1 in proptest::prelude::any::<ClientCredentials>())| {
            // Encode using `to_base64_blob` (STANDARD_NO_PAD).
            // Decode using `try_from_base64_blob`.
            {
                let new_base64_blob = creds1.to_base64_blob();

                let creds2 =
                    ClientCredentials::try_from_base64_blob(&new_base64_blob)
                        .expect("Failed to decode from new format");

                prop_assert_eq!(&creds1, &creds2);
            }

            // Compatibility test:
            // Encode using the engine used by old clients (STANDARD).
            // Decode using `try_from_base64_blob`.
            {
                let json_str = serde_json::to_string(&creds1)
                    .expect("Failed to JSON serialize");
                let old_base64_blob = base64::engine::general_purpose::STANDARD
                    .encode(json_str.as_bytes());
                let creds2 =
                    ClientCredentials::try_from_base64_blob(&old_base64_blob)
                        .expect("Failed to decode from old format");

                prop_assert_eq!(&creds1, &creds2);
            }
        });
    }

    /// Tests that the `STANDARD_NO_PAD` engine can decode any base64 string
    /// encoded with the `STANDARD` engine after removing trailing `=`s.
    #[test]
    fn prop_base64_pad_to_no_pad_compat() {
        proptest!(|(bytes1 in any::<Vec<u8>>())| {
            let string =
                base64::engine::general_purpose::STANDARD.encode(&bytes1);
            let trimmed = string.trim_end_matches('=');
            let bytes2 = base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(trimmed)
                .expect("Failed to decode base64");
            prop_assert_eq!(bytes1, bytes2);
        })
    }

    #[test]
    fn test_client_auth_encoding() {
        let mut rng = FastRng::from_u64(202505121546);
        let root_seed = RootSeed::from_rng(&mut rng);

        let user_pk = root_seed.derive_user_pk();

        let eph_ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let eph_ca_cert_der = eph_ca_cert.serialize_der_self_signed().unwrap();

        let rev_ca_cert = RevocableIssuingCaCert::from_root_seed(&root_seed);

        let rev_client_cert = RevocableClientCert::generate_from_rng(&mut rng);
        let rev_client_cert_der = rev_client_cert
            .serialize_der_ca_signed(&rev_ca_cert)
            .unwrap();
        let rev_client_key_der = rev_client_cert.serialize_key_der();
        let client_pk = rev_client_cert.public_key();

        let credentials = ClientCredentials {
            user_pk: Some(user_pk),
            lexe_auth_token: BearerAuthToken(ByteStr::from_static(
                "9dTCUvC8y7qcNyUbqynz3nwIQQHbQqPVKeMhXUj1Afr-vgj9E217_2tCS1IQM7LFqfBUC8Ec9fcb-dQiCRy6ot2FN-kR60edRFJUztAa2Rxao1Q0BS1s6vE8grgfhMYIAJDLMWgAAAAASE4zaAAAAABpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaQE",
            )),
            client_pk: *client_pk,
            rev_client_key_der,
            rev_client_cert_der,
            eph_ca_cert_der,
        };

        let credentials_str = credentials.to_base64_blob();

        // let json_len = serde_json::to_string(&credentials).unwrap().len();
        // let base64_len = credentials_str.len();
        // println!("json: {json_len}, base64: {base64_len}");

        // json: 2259 B, base64(json): 3012 B
        let expected_str = "eyJ1c2VyX3BrIjoiNmZkNzY0MTU2OTMwNTA5ZmFkNTM2MWQzYjIyYjYxZjc1YWE5MWVkNjQwMjE1YjJjNDFjMmZmODZiMmJmYzQ3MiIsImxleGVfYXV0aF90b2tlbiI6IjlkVENVdkM4eTdxY055VWJxeW56M253SVFRSGJRcVBWS2VNaFhVajFBZnItdmdqOUUyMTdfMnRDUzFJUU03TEZxZkJVQzhFYzlmY2ItZFFpQ1J5Nm90MkZOLWtSNjBlZFJGSlV6dEFhMlJ4YW8xUTBCUzFzNnZFOGdyZ2ZoTVlJQUpETE1XZ0FBQUFBU0U0emFBQUFBQUJwYVdscGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFRRSIsImNsaWVudF9wayI6IjcwODhhZjFmYzEyYWIwNGFkNmRkMTY1YmMzYTNjNWViMzA2MmI0MTFhMmY1NWExNjZiMGU0MDBiMzkwZmU0ZGIiLCJyZXZfY2xpZW50X2tleV9kZXIiOiIzMDUxMDIwMTAxMzAwNTA2MDMyYjY1NzAwNDIyMDQyMDBmNTgwZDM0NjFjNGVhMGIzNmI4MzZkNDUxYzFjMTk5ZWUzZTA2NDZhZDBkNjQyMzUzNzk3MzlkNjg2OTkyODk4MTIxMDA3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiIiwicmV2X2NsaWVudF9jZXJ0X2RlciI6IjMwODIwMTgzMzA4MjAxMzVhMDAzMDIwMTAyMDIxNDQwYmVkYzU2ZDAzZDZiNTJmMjg0MmQ2NGRmOTBkMDJkNmRhMzZhNWIzMDA1MDYwMzJiNjU3MDMwNTYzMTBiMzAwOTA2MDM1NTA0MDYwYzAyNTU1MzMxMGIzMDA5MDYwMzU1MDQwODBjMDI0MzQxMzExMTMwMGYwNjAzNTUwNDBhMGMwODZjNjU3ODY1MmQ2MTcwNzAzMTI3MzAyNTA2MDM1NTA0MDMwYzFlNGM2NTc4NjUyMDcyNjU3NjZmNjM2MTYyNmM2NTIwNjk3MzczNzU2OTZlNjcyMDQzNDEyMDYzNjU3Mjc0MzAyMDE3MGQzNzM1MzAzMTMwMzEzMDMwMzAzMDMwMzA1YTE4MGYzNDMwMzkzNjMwMzEzMDMxMzAzMDMwMzAzMDMwNWEzMDUyMzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyMzMwMjEwNjAzNTUwNDAzMGMxYTRjNjU3ODY1MjA3MjY1NzY2ZjYzNjE2MjZjNjUyMDYzNmM2OTY1NmU3NDIwNjM2NTcyNzQzMDJhMzAwNTA2MDMyYjY1NzAwMzIxMDA3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiYTMxNzMwMTUzMDEzMDYwMzU1MWQxMTA0MGMzMDBhODIwODZjNjU3ODY1MmU2MTcwNzAzMDA1MDYwMzJiNjU3MDAzNDEwMDdiMTdiYzk1MzgyNjdiMzU0ZjA3MjZkODljYjFlYzMxMGIxMDJlNDIyYWI5Njk2Yjg3ZDlhZTcwMGNlZjJlODNjMTM2NmQwYWQxOTAzNWQ5ZTNlZDA0Y2Y1ZjdmMDVkZWY2OGE3MWRlMjEyYjg5ODM0NDc3OTQyYWU3NjNhMjBmIiwiZXBoX2NhX2NlcnRfZGVyIjoiMzA4MjAxYWUzMDgyMDE2MGEwMDMwMjAxMDIwMjE0MTBjZDVjOTk4OWY5NjUyMDk0OWUwZTlhYjRjZTRkYmUxNDc2NjcxMDMwMDUwNjAzMmI2NTcwMzA1MDMxMGIzMDA5MDYwMzU1MDQwNjBjMDI1NTUzMzEwYjMwMDkwNjAzNTUwNDA4MGMwMjQzNDEzMTExMzAwZjA2MDM1NTA0MGEwYzA4NmM2NTc4NjUyZDYxNzA3MDMxMjEzMDFmMDYwMzU1MDQwMzBjMTg0YzY1Nzg2NTIwNzM2ODYxNzI2NTY0MjA3MzY1NjU2NDIwNDM0MTIwNjM2NTcyNzQzMDIwMTcwZDM3MzUzMDMxMzAzMTMwMzAzMDMwMzAzMDVhMTgwZjM0MzAzOTM2MzAzMTMwMzEzMDMwMzAzMDMwMzA1YTMwNTAzMTBiMzAwOTA2MDM1NTA0MDYwYzAyNTU1MzMxMGIzMDA5MDYwMzU1MDQwODBjMDI0MzQxMzExMTMwMGYwNjAzNTUwNDBhMGMwODZjNjU3ODY1MmQ2MTcwNzAzMTIxMzAxZjA2MDM1NTA0MDMwYzE4NGM2NTc4NjUyMDczNjg2MTcyNjU2NDIwNzM2NTY1NjQyMDQzNDEyMDYzNjU3Mjc0MzAyYTMwMDUwNjAzMmI2NTcwMDMyMTAwZWZlOWNlMWFiY2FlYWJjZWY4ZWEyZjU0YTU2OTU1MGRjZWQ0YThmM2E4Y2JiMDRjZDk0NWQxYjRlMjQ1ZjY4N2EzNGEzMDQ4MzAxMzA2MDM1NTFkMTEwNDBjMzAwYTgyMDg2YzY1Nzg2NTJlNjE3MDcwMzAxZDA2MDM1NTFkMGUwNDE2MDQxNDkwY2Q1Yzk5ODlmOTY1MjA5NDllMGU5YWI0Y2U0ZGJlMTQ3NjY3MTAzMDEyMDYwMzU1MWQxMzAxMDFmZjA0MDgzMDA2MDEwMWZmMDIwMTAwMzAwNTA2MDMyYjY1NzAwMzQxMDAzNzI1NDI5ZjViY2E4MDU2MjFjMmIyZGM0NDU4MDJlZDIxY2FiMjQ2YjQ1YWQxMjFkZDJhNDMyZWZhMmY5M2VmNzI1ZWZhMTc4MmU2NDEwOGQyMjk4ZTg2OTRmNDY4NmNlZDk4Y2U5MjgwZWQ3NDlkMGFkNGI0NGE0YTFjZWUwZCJ9";
        assert_eq!(credentials_str, expected_str);

        let credentials2 =
            ClientCredentials::try_from_base64_blob(&credentials_str)
                .expect("Failed to decode ClientAuth");
        assert_eq!(credentials, credentials2);
    }

    /// Generate serialized `ClientCredentials` sample json data:
    ///
    /// ```bash
    /// $ cargo test -p node-client --lib -- take_client_credentials_snapshot --ignored --nocapture
    /// ```
    #[test]
    #[ignore]
    fn take_client_credentials_snapshot() {
        let mut rng = FastRng::from_u64(202512210138);
        const N: usize = 3;

        let samples: Vec<ClientCredentials> =
            arbitrary::gen_values(&mut rng, any::<ClientCredentials>(), N);

        for sample in samples {
            println!("{}", serde_json::to_string(&sample).unwrap());
        }
    }

    // NOTE: see `take_client_credentials_snapshot` to generate new sample data.
    #[test]
    fn client_credentials_deser_compat() {
        let snapshot =
            fs::read_to_string("data/client_credentials_snapshot.txt").unwrap();

        for input in snapshot::parse_sample_data(&snapshot) {
            let value1: ClientCredentials =
                serde_json::from_str(input).unwrap();
            let output = serde_json::to_string(&value1).unwrap();
            let value2: ClientCredentials =
                serde_json::from_str(&output).unwrap();
            assert_eq!(value1, value2);
        }
    }
}
