use anyhow::{ensure, Context};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::api::{NodePk, User, UserPk};
use crate::enclave::{self, MachineId, Measurement, MinCpusvn, Sealed};
use crate::hexstr_or_bytes;
use crate::rng::Crng;
use crate::root_seed::RootSeed;

/// The client sends this provisioning request to the node.
#[derive(Debug, Serialize, Deserialize)]
pub struct NodeProvisionRequest {
    /// The client's user pk.
    pub user_pk: UserPk,
    /// The client's node public key, derived from the root seed. The node
    /// should sanity check by re-deriving the node pk and checking that it
    /// equals the client's expected value.
    pub node_pk: NodePk,
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
}

#[derive(Serialize, Deserialize)]
pub struct UserInstanceSeed {
    pub user: User,
    pub instance: Instance,
    pub sealed_seed: SealedSeed,
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub node_pk: NodePk,
    pub measurement: Measurement,
}

/// Uniquely identifies a sealed seed using its primary key fields.
#[derive(Clone, Serialize, Deserialize)]
pub struct SealedSeedId {
    pub user_pk: UserPk,
    pub measurement: Measurement,
    pub machine_id: MachineId,
    pub min_cpusvn: MinCpusvn,
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
        min_cpusvn: MinCpusvn,
        ciphertext: Vec<u8>,
    ) -> Self {
        Self {
            id: SealedSeedId {
                user_pk,
                measurement,
                machine_id,
                min_cpusvn,
            },
            ciphertext,
        }
    }

    pub fn seal_from_root_seed<R: Crng>(
        rng: &mut R,
        root_seed: &RootSeed,
    ) -> anyhow::Result<Self> {
        // Construct the root seed ciphertext
        let root_seed_ref = root_seed.expose_secret().as_slice();
        let sealed = enclave::seal(rng, Self::LABEL, root_seed_ref.into())
            .context("Failed to seal root seed")?;
        let ciphertext = sealed.serialize();

        // Derive / compute the other fields
        let user_pk = root_seed.derive_user_pk();
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = enclave::MIN_SGX_CPUSVN;

        Ok(Self::new(
            user_pk,
            measurement,
            machine_id,
            min_cpusvn,
            ciphertext,
        ))
    }

    pub fn unseal_and_validate(self) -> anyhow::Result<RootSeed> {
        // Compute the SGX fields
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = enclave::MIN_SGX_CPUSVN;

        // Validate SGX fields
        ensure!(
            self.id.measurement == measurement,
            "Saved measurement doesn't match current measurement",
        );
        ensure!(
            self.id.machine_id == machine_id,
            "Saved machine id doesn't match current machine id",
        );
        ensure!(
            self.id.min_cpusvn == min_cpusvn,
            "Saved min CPUSVN doesn't match current min CPUSVN",
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

#[cfg(any(test, feature = "test-utils"))]
pub mod prop {
    use proptest::arbitrary::{any, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Strategy};

    use super::*;
    use crate::rng::SmallRng;

    impl Arbitrary for NodeProvisionRequest {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            any::<SmallRng>()
                .prop_map(|mut rng| {
                    let root_seed = RootSeed::from_rng(&mut rng);
                    Self {
                        user_pk: root_seed.derive_user_pk(),
                        node_pk: root_seed.derive_node_pk(&mut rng),
                        root_seed,
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
    use crate::rng::SmallRng;
    use crate::test_utils::roundtrip;

    #[test]
    fn test_node_provision_request_sample() {
        let mut rng = SmallRng::from_u64(12345);
        let root_seed = RootSeed::from_rng(&mut rng);
        let user_pk = root_seed.derive_user_pk();
        let node_pk = root_seed.derive_node_pk(&mut rng);

        let req = NodeProvisionRequest {
            user_pk,
            node_pk,
            root_seed,
        };
        let actual = serde_json::to_value(&req).unwrap();
        let expected = serde_json::json!({
            "user_pk": "f2c1477810973cf17a74eccd01b6ed25494457408f8d506bad6c533dd7879331",
            "node_pk": "03da0d643b8bcd0167aaec47ae534a486eb865b75658ffa464e102b654ce146c31",
            "root_seed": "0a7d28d375bc07250ca30e015a808a6d70d43c5a55c4d5828cdeacca640191a1",
        });
        assert_eq!(&actual, &expected);
    }

    #[test]
    fn test_node_provision_request_json_canonical() {
        roundtrip::json_value_canonical_proptest::<NodeProvisionRequest>();
    }

    #[test]
    fn test_seal_unseal_roundtrip() {
        proptest!(|(mut rng: SmallRng)| {
            let root_seed1 = RootSeed::from_rng(&mut rng);
            let root_seed2 =
                SealedSeed::seal_from_root_seed(&mut rng, &root_seed1)
                    .unwrap()
                    .unseal_and_validate()
                    .unwrap();

            assert_eq!(
                root_seed1.expose_secret(),
                root_seed2.expose_secret(),
            );
        });
    }
}
