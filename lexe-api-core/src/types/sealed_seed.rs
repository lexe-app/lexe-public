use std::{borrow::Cow, fmt};

use anyhow::{ensure, Context};
use common::{
    api::user::UserPk,
    ed25519,
    enclave::{self, MachineId, Measurement, Sealed},
    env::DeployEnv,
    ln::network::LxNetwork,
    rng::Crng,
    root_seed::RootSeed,
    serde_helpers::hexstr_or_bytes,
};
use lexe_std::array;
#[cfg(test)]
use proptest_derive::Arbitrary;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};

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
///   returned [`DeployEnv`] and [`LxNetwork`], which can be used to validate
///   e.g. the [`LxNetwork`] supplied by the Lexe operators via CLI args.
/// - To encrypt an existing [`RootSeed`] (and [`DeployEnv`] and [`LxNetwork`])
///   into a [`SealedSeed`], use [`seal_from_root_seed`].
///
/// See [`common::enclave::seal`] for more implementation details.
///
/// [`unseal_and_validate`]: Self::unseal_and_validate
/// [`seal_from_root_seed`]: Self::seal_from_root_seed
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(Arbitrary))]
pub struct SealedSeed {
    pub id: SealedSeedId,
    /// The root seed, fully sealed + serialized.
    #[serde(with = "hexstr_or_bytes")]
    pub ciphertext: Vec<u8>,
}

/// An upgradeable version of [`Option<SealedSeed>`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MaybeSealedSeed {
    pub maybe_seed: Option<SealedSeed>,
}

/// The data that is actually sealed. This struct is serialized to JSON bytes
/// before it is encrypted. By sealing the [`LxNetwork`] along with the
/// [`RootSeed`], the root seed is bound to this [`LxNetwork`]. This allows us
/// to validate the [`LxNetwork`] that Lexe passes in via CLI args, preventing
/// any attacks that might be triggered by supplying the wrong network.
#[derive(Serialize, Deserialize)]
// Not safe to allow non-constant time comparisons outside of tests
#[cfg_attr(test, derive(PartialEq))]
struct RootSeedWithMetadata<'a> {
    #[serde(with = "hexstr_or_bytes")]
    root_seed: Cow<'a, [u8]>,
    deploy_env: DeployEnv,
    network: LxNetwork,
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
        deploy_env: DeployEnv,
        network: LxNetwork,
        measurement: Measurement,
        machine_id: MachineId,
    ) -> anyhow::Result<Self> {
        deploy_env.validate_network(network)?;

        // RootSeedWithMetadata -> JSON bytes
        let seed_w_metadata = RootSeedWithMetadata {
            root_seed: Cow::Borrowed(root_seed.expose_secret().as_slice()),
            deploy_env,
            network,
        };
        let json_bytes = serde_json::to_vec(&seed_w_metadata)
            .context("Failed to serialize RootSeedWithMetadata")?;

        // JSON bytes -> Sealed ciphertext
        // Sealed::seal will encrypt the (Cow::Owned) json bytes in place,
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
    ) -> anyhow::Result<(RootSeed, DeployEnv, LxNetwork)> {
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
        let unsealed_json_bytes = enclave::unseal(sealed, Self::LABEL)
            .context("Failed to unseal provisioned secrets")?;

        // JSON-deserialize -> RootSeedWithMetadata
        let seed_w_metadata = serde_json::from_slice::<RootSeedWithMetadata>(
            unsealed_json_bytes.as_slice(),
        )
        .context("Failed to JSON-deserialize unsealed bytes")?;
        let RootSeedWithMetadata {
            root_seed,
            deploy_env,
            network,
        } = seed_w_metadata;

        // Ensure seed bytes are zeroized upon drop, even if something errors
        let secret_root_seed = Secret::new(root_seed.into_owned());

        // &Secret<Vec<u8>> -> RootSeed
        let root_seed =
            RootSeed::try_from(secret_root_seed.expose_secret().as_slice())
                .context("Failed to deserialize root seed from secret bytes")?;

        // Validation
        deploy_env.validate_network(network)?;
        ensure!(
            self.id.user_pk == root_seed.derive_user_pk(),
            "Saved user pk doesn't match derived user pk"
        );

        // Validation complete, everything OK.
        Ok((root_seed, deploy_env, network))
    }
}

impl ed25519::Signable for SealedSeed {
    const DOMAIN_SEPARATOR: [u8; 32] = array::pad(*b"LEXE-REALM::SealedSeed");
}

impl fmt::Debug for RootSeedWithMetadata<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        let deploy_env = &self.deploy_env;
        let network = &self.network;
        write!(
            f,
            "RootSeedWithMetadata {{\
                deploy_env: {deploy_env}, \
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

    impl Arbitrary for RootSeedWithMetadata<'static> {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;
        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            (any::<Vec<u8>>(), any::<DeployEnv>(), any::<LxNetwork>())
                .prop_map(|(root_seed_vec, deploy_env, network)| {
                    RootSeedWithMetadata {
                        root_seed: Cow::from(root_seed_vec),
                        deploy_env,
                        network,
                    }
                })
                .boxed()
        }
    }
}

#[cfg(test)]
mod test {
    use common::{rng::FastRng, test_utils::roundtrip};
    use proptest::{arbitrary::any, proptest};

    use super::*;

    #[test]
    fn test_seal_unseal_roundtrip() {
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();

        proptest!(|(
            mut rng in any::<FastRng>(),
            (env1, network1) in DeployEnv::any_valid_network_combo(),
        )| {
            let root_seed1 = RootSeed::from_rng(&mut rng);

            let sealed_seed = SealedSeed::seal_from_root_seed(
                &mut rng,
                &root_seed1,
                env1,
                network1,
                measurement,
                machine_id,
            )
            .unwrap();

            let (root_seed2, env2, network2) = sealed_seed
                .unseal_and_validate(&measurement, &machine_id)
                .unwrap();

            assert_eq!(env1, env2);
            assert_eq!(network1, network2);
            assert_eq!(
                root_seed1.expose_secret(),
                root_seed2.expose_secret(),
            );
        });
    }

    #[test]
    fn test_sealed_seed_signable_roundtrip() {
        roundtrip::signed_roundtrip_proptest::<SealedSeed>();
    }

    #[test]
    fn test_sealed_seed_id_roundtrip() {
        roundtrip::query_string_roundtrip_proptest::<SealedSeedId>();
    }

    #[test]
    fn test_root_seed_json_roundtrip() {
        roundtrip::json_value_roundtrip_proptest::<RootSeedWithMetadata>();
    }
}
