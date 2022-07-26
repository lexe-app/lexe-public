#![allow(dead_code)] // TODO Remove eventually

use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use common::hex;
use subtle::ConstantTimeEq;

use crate::api::ApiClient;

pub type Port = u16;
pub type InstanceId = String;
pub type EnclaveId = String;

pub type ApiClientType = Arc<dyn ApiClient + Send + Sync>;

#[derive(Clone)]
pub struct AuthToken([u8; Self::LENGTH]);

impl AuthToken {
    const LENGTH: usize = 32;

    pub fn new(bytes: [u8; Self::LENGTH]) -> Self {
        Self(bytes)
    }

    #[cfg(test)]
    pub fn string(&self) -> String {
        hex::encode(self.0.as_slice())
    }
}

// AuthToken is a secret. We need to compare in constant time.

impl ConstantTimeEq for AuthToken {
    fn ct_eq(&self, other: &Self) -> subtle::Choice {
        self.0.as_slice().ct_eq(other.0.as_slice())
    }
}

impl PartialEq for AuthToken {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).into()
    }
}

impl Eq for AuthToken {}

impl FromStr for AuthToken {
    type Err = hex::DecodeError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0u8; Self::LENGTH];
        hex::decode_to_slice_ct(hex, bytes.as_mut_slice())
            .map(|()| Self::new(bytes))
    }
}

impl fmt::Debug for AuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Avoid formatting secrets.
        f.write_str("AuthToken(..)")
    }
}
