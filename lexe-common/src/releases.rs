use serde::{Deserialize, Serialize};

use crate::enclave;

/// Information about a single release in a 'releases.json' file, excluding its
/// bin kind and [`semver::Version`] (which are its indices).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Release {
    /// e.g. "8f4d5576a6a657992b6f9103f7587cb832a346982fa468055b30642143cc31fd"
    pub measurement: enclave::Measurement,
    /// Git revision, e.g. "edb70312d24ab871e2278b46b55fffc373d0889b".
    /// Is a monorepo git hash for private SGX binaries, is a `public/` subrepo
    /// hash for public SGX binaries (e.g. user nodes)
    pub revision: String,
    /// e.g. "2024-06-27"
    pub release_date: String,
    /// e.g. <https://github.com/lexe-app/lexe/releases/tag/lsp-v0.1.1>
    pub release_url: String,
    #[serde(flatten)]
    pub sgx_metadata: SgxMetadata,
}

/// SGX metadata for an enclave binary parsed from the crate's Cargo.toml.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SgxMetadata {
    /// Typically a hex number, e.g. `0x8000_0000`
    pub heap_size: u64,
    /// Typically a hex number, e.g. `0x80_0000`
    pub stack_size: u64,
    /// Written in Cargo.toml alongside `heap-size` and `stack-size`
    pub threads: u8,
}
