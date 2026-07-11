use lexe_common::api::{MegaId, user::UserPk};
use lexe_enclave::enclave::Measurement;
use serde::{Deserialize, Serialize};

pub type Port = u16;

/// The ports exposed by a mega node.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct MegaPorts {
    pub mega_id: MegaId,
    pub measurement: Measurement,
    // compat: Remove alias once all nodes are node-v0.9.12+
    #[serde(alias = "app_provision_port")]
    pub user_provision_port: Port,
    pub lexe_mega_port: Port,
}

/// The ports exposed by a running user node.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RunPorts {
    pub user_pk: UserPk,
    // compat: Remove alias once all nodes are node-v0.9.12+
    #[serde(alias = "app_port")]
    pub user_port: Port,
    pub lexe_port: Port,
}

/// The ports exposed by the provision server within a meganode.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionPorts {
    pub measurement: Measurement,
    // compat: Remove alias once all nodes are node-v0.9.12+
    #[serde(alias = "app_port")]
    pub user_port: Port,
}

impl From<MegaPorts> for ProvisionPorts {
    fn from(mega_ports: MegaPorts) -> Self {
        ProvisionPorts {
            measurement: mega_ports.measurement,
            user_port: mega_ports.user_provision_port,
        }
    }
}
