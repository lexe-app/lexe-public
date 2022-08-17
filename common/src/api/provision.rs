use anyhow::Context;
use bitcoin::secp256k1::PublicKey;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::api::UserPk;
use crate::enclave::{self, MachineId, Measurement, MinCpusvn, Sealed};
use crate::rng::Crng;
use crate::root_seed::RootSeed;

/// The client sends this provisioning request to the node.
#[derive(Serialize, Deserialize)]
pub struct ProvisionRequest {
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
#[derive(Serialize, Deserialize)]
pub struct SealedSeedId {
    pub node_pk: PublicKey,
    pub measurement: Measurement,
    pub machine_id: MachineId,
    pub min_cpusvn: MinCpusvn,
}

#[derive(Serialize, Deserialize)]
pub struct SealedSeed {
    #[serde(flatten)]
    pub id: SealedSeedId,
    pub seed: Vec<u8>,
}

impl SealedSeed {
    pub fn new(
        node_pk: PublicKey,
        measurement: Measurement,
        machine_id: MachineId,
        min_cpusvn: MinCpusvn,
        seed: Vec<u8>,
    ) -> Self {
        Self {
            id: SealedSeedId {
                node_pk,
                measurement,
                machine_id,
                min_cpusvn,
            },
            seed,
        }
    }
}

/// The enclave's provisioned secrets that it will seal and persist using its
/// platform enclave keys that are software and version specific.
///
/// See: [`crate::enclave::seal`]
// TODO(phlip9): rename this or SealedSeed?
pub struct ProvisionedSecrets {
    pub root_seed: RootSeed,
}

impl ProvisionedSecrets {
    const LABEL: &'static [u8] = b"provisioned secrets";

    pub fn seal(&self, rng: &mut dyn Crng) -> anyhow::Result<Sealed<'_>> {
        let root_seed_ref = self.root_seed.expose_secret().as_slice();
        enclave::seal(rng, Self::LABEL, root_seed_ref.into())
            .context("Failed to seal provisioned secrets")
    }

    pub fn unseal(sealed: Sealed<'_>) -> anyhow::Result<Self> {
        let bytes = enclave::unseal(Self::LABEL, sealed)
            .context("Failed to unseal provisioned secrets")?;
        let root_seed = RootSeed::try_from(bytes.as_slice())
            .context("Failed to deserialize root seed")?;
        Ok(Self { root_seed })
    }
}

#[derive(Serialize, Deserialize)]
pub struct NodeInstanceSeed {
    pub node: Node,
    pub instance: Instance,
    pub sealed_seed: SealedSeed,
}
