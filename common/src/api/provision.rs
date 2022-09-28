use anyhow::{ensure, Context};
use bitcoin::secp256k1::PublicKey;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::{self, MachineId, Measurement, MinCpusvn, Sealed};
use crate::rng::Crng;
use crate::root_seed::RootSeed;

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
pub struct NodeProvisionRequest {
    /// The client's user pk.
    pub user_pk: UserPk,
    /// The client's node public key, derived from the root seed. The node
    /// should sanity check by re-deriving the node pk and checking that it
    /// equals the client's expected value.
    pub node_pk: PublicKey,
    /// The secret root seed the client wants to provision into the node.
    pub root_seed: RootSeed,
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceSeed {
    pub node: Node,
    pub instance: Instance,
    pub sealed_seed: SealedSeed,
}

#[derive(Serialize, Deserialize)]
pub struct Node {
    pub user_pk: UserPk,
    pub node_pk: PublicKey,
}

#[derive(Serialize, Deserialize)]
pub struct Instance {
    pub node_pk: PublicKey,
    pub measurement: Measurement,
}

/// Uniquely identifies a sealed seed using its primary key fields.
#[derive(Clone, Serialize, Deserialize)]
pub struct SealedSeedId {
    pub node_pk: PublicKey,
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
    pub ciphertext: Vec<u8>,
}

impl SealedSeed {
    const LABEL: &'static [u8] = b"sealed seed";

    pub fn new(
        node_pk: PublicKey,
        measurement: Measurement,
        machine_id: MachineId,
        min_cpusvn: MinCpusvn,
        ciphertext: Vec<u8>,
    ) -> Self {
        Self {
            id: SealedSeedId {
                node_pk,
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
        let node_pk = root_seed.derive_node_pk(rng);
        let measurement = enclave::measurement();
        let machine_id = enclave::machine_id();
        let min_cpusvn = enclave::MIN_SGX_CPUSVN;

        Ok(Self::new(
            node_pk,
            measurement,
            machine_id,
            min_cpusvn,
            ciphertext,
        ))
    }

    pub fn unseal_and_validate<R: Crng>(
        self,
        rng: &mut R,
    ) -> anyhow::Result<RootSeed> {
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

        // Validate node_pk
        let derived_node_pk = root_seed.derive_node_pk(rng);
        ensure!(
            self.id.node_pk == derived_node_pk,
            "Saved node pk doesn't match derived node pk"
        );

        // Validation complete, everything OK.
        Ok(root_seed)
    }
}

#[cfg(test)]
mod test {
    use proptest::arbitrary::any;
    use proptest::proptest;
    use secrecy::ExposeSecret;

    use super::*;
    use crate::rng::SysRng;

    proptest! {
        #[test]
        fn seal_unseal_roundtrip(root_seed1 in any::<RootSeed>()) {
            let mut rng = SysRng::new();

            let root_seed2 =
                SealedSeed::seal_from_root_seed(&mut rng, &root_seed1)
                    .unwrap()
                    .unseal_and_validate(&mut rng)
                    .unwrap();

            assert_eq!(
                root_seed1.expose_secret(),
                root_seed2.expose_secret(),
            );
        }
    }
}
