use serde::{Deserialize, Serialize};

use crate::api::UserPk;

pub type Port = u16;

/// Used to return the port of a loaded node.
#[derive(Clone, Debug, Serialize)]
pub struct PortReply {
    pub port: Port,
}

/// Use to (de)serialize /ready requests and responses
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserPort {
    pub user_pk: UserPk,
    pub port: Port,
}
