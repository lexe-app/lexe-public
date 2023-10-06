use std::{borrow::Cow, fmt};

use anyhow::{ensure, Context};
#[cfg(test)]
use proptest_derive::Arbitrary;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::test_utils::arbitrary;
use crate::{
    api::UserPk,
    cli::Network,
    enclave,
    enclave::{MachineId, Measurement, Sealed},
    hexstr_or_bytes,
    rng::Crng,
    root_seed::RootSeed,
};

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
// Only impl PartialEq in tests since root seed comparison is not constant time.
#[cfg_attr(test, derive(PartialEq, Arbitrary))]
pub struct NodeProvisionRequest {
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
    /// The [`Network`] that this [`RootSeed`] should be bound to.
    pub network: Network,
    /// The credentials required to store data in Google Drive.
    pub gdrive_credentials: GDriveCredentials,
}

/// A complete set of OAuth2 credentials which allows making requests to the
/// Google Drive v3 API and periodically refreshing the contained access token.
#[derive(Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(PartialEq, Arbitrary))]
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
/// untrusted and not-yet-validated.
/// - To validate and convert a [`SealedSeed`] into a [`RootSeed`], use
///   [`unseal_and_validate`]. The returned [`RootSeed`] is bound to the
///   returned [`Network`], which can then be used to validate the [`Network`]
///   given by an untrusted source (e.g. by the Lexe operators via CLI args).
/// - To encrypt an existing [`RootSeed`] (and [`Network`]) into a
///   [`SealedSeed`], use [`seal_from_root_seed`].
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

/// The data that is actually sealed. This struct is serialized to JSON bytes
/// before it is encrypted. By sealing the [`Network`] along with the
/// [`RootSeed`], the root seed is bound to this [`Network`]. This allows us to
/// validate the [`Network`] that Lexe passes in via CLI args, preventing any
/// attacks that might be triggered by supplying the wrong network.
#[derive(Serialize, Deserialize)]
// Not safe to allow non-constant time comparisons outside of tests
#[cfg_attr(test, derive(PartialEq))]
struct RootSeedWithNetwork<'a> {
    #[serde(with = "hexstr_or_bytes")]
    root_seed: Cow<'a, [u8]>,
    network: Network,
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
        network: Network,
        measurement: Measurement,
        machine_id: MachineId,
    ) -> anyhow::Result<Self> {
        // RootSeedWithNetwork -> JSON bytes
        let seed_w_network = RootSeedWithNetwork {
            root_seed: Cow::Borrowed(root_seed.expose_secret().as_slice()),
            network,
        };
        let json_bytes = serde_json::to_vec(&seed_w_network)
            .context("Failed to serialize RootSeedWithMetadata")?;

        // JSON bytes -> Sealed ciphertext
        // enclave::seal will encrypt the (Cow::Owned) json bytes in place,
        // thereby disposing of the sensitive root seed bytes.
        let json_bytes_cow = Cow::Owned(json_bytes);
        let sealed = enclave::seal(rng, Self::LABEL, json_bytes_cow)
            .context("Failed to seal root seed w network")?;
        let ciphertext = sealed.serialize();

        // Derive / compute the other fields
        let user_pk = root_seed.derive_user_pk();

        Ok(Self::new(user_pk, measurement, machine_id, ciphertext))
    }

    pub fn unseal_and_validate(
        self,
        expected_measurement: &Measurement,
        expected_machine_id: &MachineId,
    ) -> anyhow::Result<(RootSeed, Network)> {
        // Validate SGX fields
        ensure!(
            &self.id.measurement == expected_measurement,
            "Saved measurement doesn't match current measurement",
        );
        ensure!(
            &self.id.machine_id == expected_machine_id,
            "Saved machine id doesn't match current machine id",
        );

        // Ciphertext -unseal-> JSON bytes
        let sealed = Sealed::deserialize(&self.ciphertext)
            .context("Failed to deserialize Sealed")?;
        let unsealed_json_bytes = enclave::unseal(Self::LABEL, sealed)
            .context("Failed to unseal provisioned secrets")?;

        // JSON-deserialize -> RootSeedWithNetwork
        let seed_w_network = serde_json::from_slice::<RootSeedWithNetwork>(
            unsealed_json_bytes.as_slice(),
        )
        .context("Failed to JSON-deserialize unsealed bytes")?;
        let network = seed_w_network.network;

        // Ensure `RootSeedWithNetwork::root_seed` bytes are zeroized upon drop
        let secret_root_seed =
            Secret::new(seed_w_network.root_seed.into_owned());

        // &Secret<Vec<u8>> -> RootSeed
        let root_seed =
            RootSeed::try_from(secret_root_seed.expose_secret().as_slice())
                .context("Failed to deserialize root seed from secret bytes")?;

        // Validate user_pk
        let derived_user_pk = root_seed.derive_user_pk();
        ensure!(
            self.id.user_pk == derived_user_pk,
            "Saved user pk doesn't match derived user pk"
        );

        // Validation complete, everything OK.
        Ok((root_seed, network))
    }
}

impl fmt::Debug for NodeProvisionRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NodeProvisionRequest { .. }")
    }
}

impl fmt::Debug for GDriveCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let client_id = &self.client_id;
        let expires_at = &self.expires_at;
        write!(
            f,
            "GDriveCredentials {{ \
                client_id: {client_id}, \
                expires_at: {expires_at}, \
                .. \
            }}"
        )
    }
}

impl fmt::Debug for RootSeedWithNetwork<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        let network = &self.network;
        write!(
            f,
            "RootSeedWithNetwork {{\
            network: {network}, \
            root_seed: RootSeed(..) \
        }}"
        )
    }
}

#[cfg(test)]
mod test_impls {
    use proptest::{
        arbitrary::{any, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for RootSeedWithNetwork<'static> {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<Vec<u8>>(), any::<Network>())
                .prop_map(|(root_seed_vec, network)| RootSeedWithNetwork {
                    root_seed: Cow::from(root_seed_vec),
                    network,
                })
                .boxed()
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
        let gdrive_credentials = GDriveCredentials::dummy();
        let network = Network::REGTEST;
        let req = NodeProvisionRequest {
            root_seed,
            network,
            gdrive_credentials,
        };
        let actual = serde_json::to_value(&req).unwrap();
        let expected = serde_json::json!({
            "root_seed": "0a7d28d375bc07250ca30e015a808a6d70d43c5a55c4d5828cdeacca640191a1",
            "network": "regtest",
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

        proptest!(|(mut rng: WeakRng, network1: Network)| {
            let root_seed1 = RootSeed::from_rng(&mut rng);

            let sealed_seed = SealedSeed::seal_from_root_seed(
                &mut rng,
                &root_seed1,
                network1,
                measurement,
                machine_id,
            )
            .unwrap();

            let (root_seed2, network2) = sealed_seed
                .unseal_and_validate(&measurement, &machine_id)
                .unwrap();

            assert_eq!(network1, network2);
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

    #[test]
    fn test_root_seed_json_roundtrip() {
        roundtrip::json_value_canonical_proptest::<RootSeedWithNetwork>();
    }
}
