use common::{
    api::{user::UserPk, MegaId},
    enclave::Measurement,
};
use serde::{Deserialize, Serialize};

pub type Port = u16;

/// The ports exposed by a mega node.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct MegaPorts {
    pub mega_id: MegaId,
    pub measurement: Measurement,
    pub app_provision_port: Port,
    pub lexe_mega_port: Port,
}

/// The ports exposed by a running user node.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RunPorts {
    pub user_pk: UserPk,
    pub app_port: Port,
    pub lexe_port: Port,
}

/// The ports exposed by the provision server within a meganode.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionPorts {
    pub measurement: Measurement,
    pub app_port: Port,
}

impl From<MegaPorts> for ProvisionPorts {
    fn from(mega_ports: MegaPorts) -> Self {
        ProvisionPorts {
            measurement: mega_ports.measurement,
            app_port: mega_ports.app_provision_port,
        }
    }
}
