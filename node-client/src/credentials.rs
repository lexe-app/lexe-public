//! Client credentials for authentication with Lexe services.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use base64::Engine;
use common::{
    api::{
        auth::{BearerAuthToken, Scope},
        revocable_clients::CreateRevocableClientResponse,
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
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

/// Credentials required to connect to a user node via mTLS.
pub enum Credentials<'a> {
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
    derive(Debug, PartialEq, Eq, Arbitrary)
)]
pub struct ClientCredentials {
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

// --- impl Credentials --- //

impl<'a> Credentials<'a> {
    pub fn from_root_seed(root_seed: &'a RootSeed) -> Self {
        Credentials::RootSeed(root_seed)
    }

    pub fn from_client_credentials(
        client_credentials: &'a ClientCredentials,
    ) -> Self {
        Credentials::ClientCredentials(client_credentials)
    }

    /// Create a [`BearerAuthenticator`] appropriate for the given credentials.
    ///
    /// Currently limits to [`Scope::NodeConnect`] for [`RootSeed`] credentials.
    pub fn bearer_authenticator(&self) -> Arc<BearerAuthenticator> {
        match self {
            Credentials::RootSeed(root_seed) => {
                let maybe_cached_token = None;
                Arc::new(BearerAuthenticator::new_with_scope(
                    root_seed.derive_user_key_pair(),
                    maybe_cached_token,
                    Some(Scope::NodeConnect),
                ))
            }
            Credentials::ClientCredentials(client_credentials) =>
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
            Credentials::RootSeed(root_seed) =>
                shared_seed::app_node_run_client_config(
                    rng, deploy_env, root_seed,
                )
                .context("Failed to build RootSeed TLS client config"),
            Credentials::ClientCredentials(client_credentials) =>
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
    use common::{byte_str::ByteStr, rng::FastRng};
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

        let eph_ca_cert = EphemeralIssuingCaCert::from_root_seed(&root_seed);
        let eph_ca_cert_der = eph_ca_cert.serialize_der_self_signed().unwrap();

        let rev_ca_cert = RevocableIssuingCaCert::from_root_seed(&root_seed);

        let rev_client_cert = RevocableClientCert::generate_from_rng(&mut rng);
        let rev_client_cert_der = rev_client_cert
            .serialize_der_ca_signed(&rev_ca_cert)
            .unwrap();
        let rev_client_key_der = rev_client_cert.serialize_key_der();
        let client_pk = rev_client_cert.public_key();

        let client_auth = ClientCredentials {
            lexe_auth_token: BearerAuthToken(ByteStr::from_static(
                "9dTCUvC8y7qcNyUbqynz3nwIQQHbQqPVKeMhXUj1Afr-vgj9E217_2tCS1IQM7LFqfBUC8Ec9fcb-dQiCRy6ot2FN-kR60edRFJUztAa2Rxao1Q0BS1s6vE8grgfhMYIAJDLMWgAAAAASE4zaAAAAABpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaQE",
            )),
            client_pk: *client_pk,
            rev_client_key_der,
            rev_client_cert_der,
            eph_ca_cert_der,
        };

        let client_auth_str = client_auth.to_base64_blob();
        // json: ~2.2 KiB, base64(json): ~2.9 KiB
        let expected_str = "eyJsZXhlX2F1dGhfdG9rZW4iOiI5ZFRDVXZDOHk3cWNOeVVicXluejNud0lRUUhiUXFQVktlTWhYVWoxQWZyLXZnajlFMjE3XzJ0Q1MxSVFNN0xGcWZCVUM4RWM5ZmNiLWRRaUNSeTZvdDJGTi1rUjYwZWRSRkpVenRBYTJSeGFvMVEwQlMxczZ2RThncmdmaE1ZSUFKRExNV2dBQUFBQVNFNHphQUFBQUFCcGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhV2xwYVdscGFXbHBhUUUiLCJjbGllbnRfcGsiOiI3MDg4YWYxZmMxMmFiMDRhZDZkZDE2NWJjM2EzYzVlYjMwNjJiNDExYTJmNTVhMTY2YjBlNDAwYjM5MGZlNGRiIiwicmV2X2NsaWVudF9rZXlfZGVyIjoiMzA1MTAyMDEwMTMwMDUwNjAzMmI2NTcwMDQyMjA0MjAwZjU4MGQzNDYxYzRlYTBiMzZiODM2ZDQ1MWMxYzE5OWVlM2UwNjQ2YWQwZDY0MjM1Mzc5NzM5ZDY4Njk5Mjg5ODEyMTAwNzA4OGFmMWZjMTJhYjA0YWQ2ZGQxNjViYzNhM2M1ZWIzMDYyYjQxMWEyZjU1YTE2NmIwZTQwMGIzOTBmZTRkYiIsInJldl9jbGllbnRfY2VydF9kZXIiOiIzMDgyMDE4MzMwODIwMTM1YTAwMzAyMDEwMjAyMTQ0MGJlZGM1NmQwM2Q2YjUyZjI4NDJkNjRkZjkwZDAyZDZkYTM2YTViMzAwNTA2MDMyYjY1NzAzMDU2MzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyNzMwMjUwNjAzNTUwNDAzMGMxZTRjNjU3ODY1MjA3MjY1NzY2ZjYzNjE2MjZjNjUyMDY5NzM3Mzc1Njk2ZTY3MjA0MzQxMjA2MzY1NzI3NDMwMjAxNzBkMzczNTMwMzEzMDMxMzAzMDMwMzAzMDMwNWExODBmMzQzMDM5MzYzMDMxMzAzMTMwMzAzMDMwMzAzMDVhMzA1MjMxMGIzMDA5MDYwMzU1MDQwNjBjMDI1NTUzMzEwYjMwMDkwNjAzNTUwNDA4MGMwMjQzNDEzMTExMzAwZjA2MDM1NTA0MGEwYzA4NmM2NTc4NjUyZDYxNzA3MDMxMjMzMDIxMDYwMzU1MDQwMzBjMWE0YzY1Nzg2NTIwNzI2NTc2NmY2MzYxNjI2YzY1MjA2MzZjNjk2NTZlNzQyMDYzNjU3Mjc0MzAyYTMwMDUwNjAzMmI2NTcwMDMyMTAwNzA4OGFmMWZjMTJhYjA0YWQ2ZGQxNjViYzNhM2M1ZWIzMDYyYjQxMWEyZjU1YTE2NmIwZTQwMGIzOTBmZTRkYmEzMTczMDE1MzAxMzA2MDM1NTFkMTEwNDBjMzAwYTgyMDg2YzY1Nzg2NTJlNjE3MDcwMzAwNTA2MDMyYjY1NzAwMzQxMDA3YjE3YmM5NTM4MjY3YjM1NGYwNzI2ZDg5Y2IxZWMzMTBiMTAyZTQyMmFiOTY5NmI4N2Q5YWU3MDBjZWYyZTgzYzEzNjZkMGFkMTkwMzVkOWUzZWQwNGNmNWY3ZjA1ZGVmNjhhNzFkZTIxMmI4OTgzNDQ3Nzk0MmFlNzYzYTIwZiIsImVwaF9jYV9jZXJ0X2RlciI6IjMwODIwMWFlMzA4MjAxNjBhMDAzMDIwMTAyMDIxNDEwY2Q1Yzk5ODlmOTY1MjA5NDllMGU5YWI0Y2U0ZGJlMTQ3NjY3MTAzMDA1MDYwMzJiNjU3MDMwNTAzMTBiMzAwOTA2MDM1NTA0MDYwYzAyNTU1MzMxMGIzMDA5MDYwMzU1MDQwODBjMDI0MzQxMzExMTMwMGYwNjAzNTUwNDBhMGMwODZjNjU3ODY1MmQ2MTcwNzAzMTIxMzAxZjA2MDM1NTA0MDMwYzE4NGM2NTc4NjUyMDczNjg2MTcyNjU2NDIwNzM2NTY1NjQyMDQzNDEyMDYzNjU3Mjc0MzAyMDE3MGQzNzM1MzAzMTMwMzEzMDMwMzAzMDMwMzA1YTE4MGYzNDMwMzkzNjMwMzEzMDMxMzAzMDMwMzAzMDMwNWEzMDUwMzEwYjMwMDkwNjAzNTUwNDA2MGMwMjU1NTMzMTBiMzAwOTA2MDM1NTA0MDgwYzAyNDM0MTMxMTEzMDBmMDYwMzU1MDQwYTBjMDg2YzY1Nzg2NTJkNjE3MDcwMzEyMTMwMWYwNjAzNTUwNDAzMGMxODRjNjU3ODY1MjA3MzY4NjE3MjY1NjQyMDczNjU2NTY0MjA0MzQxMjA2MzY1NzI3NDMwMmEzMDA1MDYwMzJiNjU3MDAzMjEwMGVmZTljZTFhYmNhZWFiY2VmOGVhMmY1NGE1Njk1NTBkY2VkNGE4ZjNhOGNiYjA0Y2Q5NDVkMWI0ZTI0NWY2ODdhMzRhMzA0ODMwMTMwNjAzNTUxZDExMDQwYzMwMGE4MjA4NmM2NTc4NjUyZTYxNzA3MDMwMWQwNjAzNTUxZDBlMDQxNjA0MTQ5MGNkNWM5OTg5Zjk2NTIwOTQ5ZTBlOWFiNGNlNGRiZTE0NzY2NzEwMzAxMjA2MDM1NTFkMTMwMTAxZmYwNDA4MzAwNjAxMDFmZjAyMDEwMDMwMDUwNjAzMmI2NTcwMDM0MTAwMzcyNTQyOWY1YmNhODA1NjIxYzJiMmRjNDQ1ODAyZWQyMWNhYjI0NmI0NWFkMTIxZGQyYTQzMmVmYTJmOTNlZjcyNWVmYTE3ODJlNjQxMDhkMjI5OGU4Njk0ZjQ2ODZjZWQ5OGNlOTI4MGVkNzQ5ZDBhZDRiNDRhNGExY2VlMGQifQ";
        assert_eq!(client_auth_str, expected_str);

        let client_auth2 =
            ClientCredentials::try_from_base64_blob(&client_auth_str)
                .expect("Failed to decode ClientAuth");
        assert_eq!(client_auth, client_auth2);
    }
}
