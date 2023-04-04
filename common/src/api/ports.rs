use serde::{Deserialize, Serialize};

use crate::api::UserPk;

pub type Port = u16;

/// Used to return the port of a loaded node.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortReply {
    pub ports: Ports,
}

/// Used to (de)serialize /ready requests and responses
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct UserPorts {
    pub user_pk: UserPk,
    pub ports: Ports,
}

// TODO(max): Expose only one port, then remove this entire enum + child structs
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Ports {
    Run(RunPorts),
    Provision(ProvisionPorts),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RunPorts {
    pub app_port: Port,
    pub host_port: Port,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionPorts {
    pub app_port: Port,
}

// --- impl UserPorts --- //

impl UserPorts {
    /// Shorthand to construct a UserPorts containing a Run variant.
    /// Be careful to specify the app/host ports in the correct order.
    pub fn new_run(user_pk: UserPk, app_port: Port, host_port: Port) -> Self {
        Self {
            user_pk,
            ports: Ports::Run(RunPorts {
                app_port,
                host_port,
            }),
        }
    }

    /// Shorthand to construct a UserPorts containing a Provision variant.
    pub fn new_provision(user_pk: UserPk, app_port: Port) -> Self {
        Self {
            user_pk,
            ports: Ports::Provision(ProvisionPorts { app_port }),
        }
    }

    /// 'Unwraps' self to the RunPorts struct.
    pub fn unwrap_run(self) -> RunPorts {
        self.ports.unwrap_run()
    }

    /// 'Unwraps' self to the ProvisionPorts struct.
    pub fn unwrap_provision(self) -> ProvisionPorts {
        self.ports.unwrap_provision()
    }
}

// --- impl Ports --- //

impl Ports {
    /// Shorthand to return the app port.
    pub fn app(&self) -> Port {
        match self {
            Self::Run(run_ports) => run_ports.app_port,
            Self::Provision(provision_ports) => provision_ports.app_port,
        }
    }

    /// 'Unwraps' self to the RunPorts struct.
    pub fn unwrap_run(self) -> RunPorts {
        match self {
            Self::Run(run_ports) => run_ports,
            Self::Provision(_) => panic!("Wrong variant"),
        }
    }

    /// 'Unwraps' self to the ProvisionPorts struct.
    pub fn unwrap_provision(self) -> ProvisionPorts {
        match self {
            Self::Run(_) => panic!("Wrong variant"),
            Self::Provision(provision_ports) => provision_ports,
        }
    }
}
