use std::fmt::{self, Display};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub const DEFAULT_USER_PK: UserPk = UserPk(1);

#[derive(Copy, Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserPk(i64);

impl UserPk {
    pub fn new(inner: i64) -> Self {
        Self(inner)
    }

    pub fn inner(&self) -> i64 {
        self.0
    }
}

impl FromStr for UserPk {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let inner = i64::from_str(s)?;
        Ok(Self(inner))
    }
}

impl Display for UserPk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
