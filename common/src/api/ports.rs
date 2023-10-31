use serde::{Deserialize, Serialize};

use crate::{api::UserPk, enclave::Measurement};

pub type Port = u16;

/// Represents the ports used by a user node.
/// Used to (de)serialize /ready requests and responses.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum Ports {
    Run(RunPorts),
    Provision(ProvisionPorts),
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RunPorts {
    pub user_pk: UserPk,
    pub app_port: Port,
    pub lexe_port: Port,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct ProvisionPorts {
    // TODO(max): Get rid of this field
    pub user_pk: UserPk,
    pub measurement: Measurement,
    pub app_port: Port,
    pub lexe_port: Port,
}

impl Ports {
    /// Shorthand to construct a [`Ports`] containing a Run variant.
    /// Be careful to specify the app/lexe ports in the correct order.
    pub fn new_run(user_pk: UserPk, app_port: Port, lexe_port: Port) -> Self {
        Ports::Run(RunPorts {
            user_pk,
            app_port,
            lexe_port,
        })
    }

    /// Shorthand to construct a [`Ports`] containing a Provision variant.
    /// Be careful to specify the app/lexe ports in the correct order.
    pub fn new_provision(
        user_pk: UserPk,
        measurement: Measurement,
        app_port: Port,
        lexe_port: Port,
    ) -> Self {
        Ports::Provision(ProvisionPorts {
            user_pk,
            measurement,
            app_port,
            lexe_port,
        })
    }

    // hACK - TODO(max): Remove
    pub fn user_pk(&self) -> UserPk {
        match self {
            Self::Run(RunPorts { user_pk, .. }) => *user_pk,
            Self::Provision(ProvisionPorts { user_pk, .. }) => *user_pk,
        }
    }

    /// Shorthand to return the app port.
    pub fn app(&self) -> Port {
        match self {
            Self::Run(run_ports) => run_ports.app_port,
            Self::Provision(provision_ports) => provision_ports.app_port,
        }
    }

    /// Shorthand to return the Lexe operator port.
    pub fn lexe(&self) -> Port {
        match self {
            Self::Run(run_ports) => run_ports.lexe_port,
            Self::Provision(provision_ports) => provision_ports.lexe_port,
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
