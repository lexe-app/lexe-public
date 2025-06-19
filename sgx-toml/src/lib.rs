//! A tiny utility crate for reading the `[package.metadata.fortanix-sgx]`
//! section of a `Cargo.toml`.

use std::{fs, path::Path};

use anyhow::Context;
use serde::Deserialize;

// Default SGX config
const DEBUG: bool = false;
const HEAP_SIZE: u64 = 0x0200_0000; // 32 MiB
const SSAFRAMESIZE: u32 = 1;
const STACK_SIZE: u64 = 0x0002_0000; // 128 KiB
const THREADS: u8 = 2; // Want 1 thread, but async_usercalls needs another

#[derive(Clone, Debug)]
pub struct FortanixSgxConfig {
    pub debug: bool,
    pub heap_size: u64,
    pub ssaframesize: u32,
    pub stack_size: u64,
    pub threads: u8,
}

/// Given a path to a `Cargo.toml`, tries to read the FortanixSgxConfig.
pub fn read_fortanix_sgx_config(
    cargo_toml_path: impl AsRef<Path>,
) -> anyhow::Result<FortanixSgxConfig> {
    let cargo_toml_str = fs::read_to_string(cargo_toml_path.as_ref())
        .with_context(|| format!("{:?}", cargo_toml_path.as_ref()))
        .context("Failed to read Cargo.toml")?;
    let cargo_toml = toml::from_str::<CargoToml>(&cargo_toml_str)
        .with_context(|| cargo_toml_str)
        .context("Failed to deserialize Cargo.toml")?;

    let fortanix_sgx = cargo_toml.package.metadata.fortanix_sgx;

    Ok(FortanixSgxConfig {
        debug: fortanix_sgx.debug.unwrap_or(DEBUG),
        heap_size: fortanix_sgx.heap_size.unwrap_or(HEAP_SIZE),
        ssaframesize: fortanix_sgx.ssaframesize.unwrap_or(SSAFRAMESIZE),
        stack_size: fortanix_sgx.stack_size.unwrap_or(STACK_SIZE),
        threads: fortanix_sgx.threads.unwrap_or(THREADS),
    })
}

#[derive(Deserialize, Debug)]
struct CargoToml {
    package: Package,
}

#[derive(Deserialize, Debug)]
struct Package {
    metadata: Metadata,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct Metadata {
    fortanix_sgx: FortanixSgx,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct FortanixSgx {
    debug: Option<bool>,
    heap_size: Option<u64>,
    ssaframesize: Option<u32>,
    stack_size: Option<u64>,
    threads: Option<u8>,
}
