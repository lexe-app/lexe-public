use anyhow::{ensure, Context};
#[cfg(test)]
use proptest_derive::Arbitrary;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::{NodePk, UserPk},
    enclave::{self, MachineId, Measurement, Sealed},
    hexstr_or_bytes,
    rng::Crng,
    root_seed::RootSeed,
};

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug))]
pub struct NodeProvisionRequest {
    /// The client's user pk.
    pub user_pk: UserPk,
    /// The client's node public key, derived from the root seed. The node
    /// should sanity check by re-deriving the node pk and checking that it
    /// equals the client's expected value.
    pub node_pk: NodePk,
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
    /// The credentials required to store data in Google Drive.
    pub gdrive_credentials: GDriveCredentials,
}

/// A complete set of OAuth2 credentials which allows making requests to the
/// Google Drive v3 API and periodically refreshing the contained access token.
// Contains sensitive info so we only derive Debug in tests
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(Debug, Arbitrary))]
pub struct GDriveCredentials {
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_id: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub client_secret: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub refresh_token: String,
    #[cfg_attr(test, proptest(strategy = "arbitrary::any_string()"))]
    pub access_token: String,
    /// Unix timestamp (in seconds) at which the current access token expires.
    /// Set to 0 if unknown; the tokens will just be refreshed at next use.
    pub expires_at: u64,
}

impl GDriveCredentials {
    /// Get a dummy value which can be used in tests.
    // TODO(max): Re-add cfg once app-rs does actual oauth
    // #[cfg(any(test, feature = "test-utils"))]
    pub fn dummy() -> Self {
        Self {
            client_id: String::from("client_id"),
            client_secret: String::from("client_secret"),
            refresh_token: String::from("refresh_token"),
            access_token: String::from("access_token"),
            expires_at: 0,
        }
    }

    /// Attempts to construct an [`GDriveCredentials`] from env.
    ///
    /// ```bash
    /// export GOOGLE_CLIENT_ID="<client_id>"
    /// export GOOGLE_CLIENT_SECRET="<client_secret>"
    /// export GOOGLE_REFRESH_TOKEN="<refresh_token>"
    /// export GOOGLE_ACCESS_TOKEN="<access_token>"
    /// export GOOGLE_ACCESS_TOKEN_EXPIRY="<timestamp>" # Set to 0 if unknown
    /// ```
    #[cfg(any(test, feature = "test-utils"))]
    pub fn from_env() -> anyhow::Result<Self> {
        use std::{env, str::FromStr};

        let client_id = env::var("GOOGLE_CLIENT_ID")
            .context("Missing 'GOOGLE_CLIENT_ID' in env")?;
        let client_secret = env::var("GOOGLE_CLIENT_SECRET")
            .context("Missing 'GOOGLE_CLIENT_SECRET' in env")?;
        let refresh_token = env::var("GOOGLE_REFRESH_TOKEN")
            .context("Missing 'GOOGLE_REFRESH_TOKEN' in env")?;
        let access_token = env::var("GOOGLE_ACCESS_TOKEN")
            .context("Missing 'GOOGLE_ACCESS_TOKEN' in env")?;
        let expires_at_str = env::var("GOOGLE_ACCESS_TOKEN_EXPIRY")
            .context("Missing 'GOOGLE_ACCESS_TOKEN_EXPIRY' in env")?;
        let expires_at = u64::from_str(&expires_at_str)
            .context("Invalid GOOGLE_ACCESS_TOKEN_EXPIRY")?;

        Ok(Self {
            client_id,
            client_secret,
            refresh_token,
            access_token,
            expires_at,
        })
    }
}

/// Uniquely identifies a sealed seed using its primary key fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct SealedSeedId {
    pub user_pk: UserPk,
    pub measurement: Measurement,
    pub machine_id: MachineId,
}

/// The user node's provisioned seed that is sealed and persisted using its
/// platform enclave keys that are software and version specific.
///
/// This struct is returned directly from the DB so it should be considered as
/// untrusted and not-yet-validated. To validate and convert a [`SealedSeed`]
/// into a [`RootSeed`], use [`unseal_and_validate`]. To encrypt an existing
/// [`RootSeed`] into a [`SealedSeed`], use [`seal_from_root_seed`].
///
/// See [`crate::enclave::seal`] for more implementation details.
///
/// [`unseal_and_validate`]: Self::unseal_and_validate
/// [`seal_from_root_seed`]: Self::seal_from_root_seed
#[derive(Clone, Serialize, Deserialize)]
pub struct SealedSeed {
    #[serde(flatten)]
    pub id: SealedSeedId,
    /// The root seed, fully sealed + serialized.
    #[serde(with = "hexstr_or_bytes")]
    pub ciphertext: Vec<u8>,
}

impl SealedSeed {
    const LABEL: &'static [u8] = b"sealed seed";

    pub fn new(
        user_pk: UserPk,
        measurement: Measurement,
        machine_id: MachineId,
        ciphertext: Vec<u8>,
    ) -> Self {
        Self {
            id: SealedSeedId {
                user_pk,
                measurement,
                machine_id,
            },
            ciphertext,
        }
    }

    pub fn seal_from_root_seed<R: Crng>(
        rng: &mut R,
        root_seed: &RootSeed,
        measurement: Measurement,
        machine_id: MachineId,
    ) -> anyhow::Result<Self> {
        // Construct the root seed ciphertext
        let root_seed_ref = root_seed.expose_secret().as_slice();
        let sealed = enclave::seal(rng, Self::LABEL, root_seed_ref.into())
            .context("Failed to seal root seed")?;
        let ciphertext = sealed.serialize();

        // Derive / compute the other fields
        let user_pk = root_seed.derive_user_pk();

        Ok(Self::new(user_pk, measurement, machine_id, ciphertext))
    }

    pub fn unseal_and_validate(
        self,
        expected_measurement: &Measurement,
        expected_machine_id: &MachineId,
    ) -> anyhow::Result<RootSeed> {
        // Validate SGX fields
        ensure!(
            &self.id.measurement == expected_measurement,
            "Saved measurement doesn't match current measurement",
        );
        ensure!(
            &self.id.machine_id == expected_machine_id,
            "Saved machine id doesn't match current machine id",
        );

        // Unseal
        let sealed = Sealed::deserialize(&self.ciphertext)
            .context("Failed to deserialize sealed seed")?;
        let unsealed_seed = enclave::unseal(Self::LABEL, sealed)
            .context("Failed to unseal provisioned secrets")?;

        // Reconstruct root seed
        let root_seed = RootSeed::try_from(unsealed_seed.as_slice())
            .context("Failed to deserialize root seed")?;

        // Validate user_pk
        let derived_user_pk = root_seed.derive_user_pk();
        ensure!(
            self.id.user_pk == derived_user_pk,
            "Saved user pk doesn't match derived user pk"
        );

        // Validation complete, everything OK.
        Ok(root_seed)
    }
}

// --- impl Arbitrary --- //

// Change to any(test, feature = "test-utils") only if needed; we end up with
// needlessly long #[cfg_attr(...)] declarations otherwise.
#[cfg(test)]
mod test_impls {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;
    use crate::rng::WeakRng;

    impl Arbitrary for NodeProvisionRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<WeakRng>(), any::<GDriveCredentials>())
                .prop_map(|(mut rng, gdrive_credentials)| {
                    let root_seed = RootSeed::from_rng(&mut rng);
                    Self {
                        user_pk: root_seed.derive_user_pk(),
                        node_pk: root_seed.derive_node_pk(&mut rng),
                        root_seed,
                        gdrive_credentials,
                    }
                })
                .boxed()
        }
    }

    // only impl PartialEq in tests; not safe to compare root seeds w/o constant
    // time comparison.

    impl PartialEq for NodeProvisionRequest {
        fn eq(&self, other: &Self) -> bool {
            self.root_seed.expose_secret() == other.root_seed.expose_secret()
                && self.user_pk == other.user_pk
                && self.node_pk == other.node_pk
        }
    }
}

#[cfg(test)]
mod test {
    use proptest::proptest;
    use secrecy::ExposeSecret;

    use super::*;
    use crate::{rng::WeakRng, test_utils::roundtrip};

    #[test]
    fn test_node_provision_request_sample() {
        let mut rng = WeakRng::from_u64(12345);
        let root_seed = RootSeed::from_rng(&mut rng);
        let user_pk = root_seed.derive_user_pk();
        let node_pk = root_seed.derive_node_pk(&mut rng);
        let gdrive_credentials = GDriveCredentials::dummy();
        let req = NodeProvisionRequest {
            user_pk,
            node_pk,
            root_seed,
            gdrive_credentials,
        };
        let actual = serde_json::to_value(&req).unwrap();
        let expected = serde_json::json!({
            "user_pk": "f2c1477810973cf17a74eccd01b6ed25494457408f8d506bad6c533dd7879331",
            "node_pk": "0306808498ee778b885aeca86409d3ef286e061c9205f2c6080cba863d09f10e85",
            "root_seed": "0a7d28d375bc07250ca30e015a808a6d70d43c5a55c4d5828cdeacca640191a1",
            "gdrive_credentials": {
                "client_id": "client_id",
                "client_secret": "client_secret",
                "refresh_token": "refresh_token",
                "access_token": "access_token",
                "expires_at": 0,
            }
        });
        assert_eq!(&actual, &expected);
    }

    #[test]
    fn test_node_provision_request_json_canonical() {
        roundtrip::json_value_canonical_proptest::<NodeProvisionRequest>();
    }

    #[test]
    fn test_seal_unseal_roundtrip() {
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();

        proptest!(|(mut rng: WeakRng)| {
            let root_seed1 = RootSeed::from_rng(&mut rng);

            let sealed_seed = SealedSeed::seal_from_root_seed(
                &mut rng,
                &root_seed1,
                measurement,
                machine_id,
            )
            .unwrap();

            let root_seed2 = sealed_seed
                .unseal_and_validate(&measurement, &machine_id)
                .unwrap();

            assert_eq!(
                root_seed1.expose_secret(),
                root_seed2.expose_secret(),
            );
        });
    }

    #[test]
    fn test_sealed_seed_id_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<SealedSeedId>();
    }
}
