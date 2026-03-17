//! A tiny utility crate for reading the `[package.metadata.fortanix-sgx]`
//! section of a `Cargo.toml`.

use std::{fs, path::Path};

use anyhow::{Context, anyhow};
use toml1::{
    Spanned,
    de::{DeTable, DeValue},
};

#[derive(Clone, Debug)]
pub struct FortanixSgxConfig {
    pub debug: bool,
    pub heap_size: u64,
    pub ssaframesize: u32,
    pub stack_size: u64,
    pub threads: u8,
}

// Default SGX config
impl Default for FortanixSgxConfig {
    fn default() -> Self {
        Self {
            debug: false,
            heap_size: 0x0200_0000, // 32 MiB
            ssaframesize: 1,
            stack_size: 0x0002_0000, // 128 KiB
            // Want 1 thread, but async_usercalls needs another
            threads: 2,
        }
    }
}

/// Given a path to a `Cargo.toml`, tries to read the [`FortanixSgxConfig`].
pub fn read_fortanix_sgx_config(
    cargo_toml_path: &Path,
) -> anyhow::Result<FortanixSgxConfig> {
    let cargo_toml_str = fs::read_to_string(cargo_toml_path)
        .with_context(|| format!("{}", cargo_toml_path.display()))
        .context("Failed to read Cargo.toml")?;

    let cargo_toml = toml1::de::DeTable::parse(&cargo_toml_str)
        .map_err(|err| anyhow!("Failed to deserialize Cargo.toml: {err}"))?
        .into_inner();

    FortanixSgxConfig::from_cargo_toml(cargo_toml)
}

impl FortanixSgxConfig {
    fn from_cargo_toml(cargo_toml: DeTable<'_>) -> anyhow::Result<Self> {
        let mut cfg = FortanixSgxConfig::default();

        let sgx = if let Some(sgx) = deser_sgx_table(&cargo_toml)? {
            sgx
        } else {
            return Ok(cfg);
        };

        if let Some(x) = sgx.get("debug").map(deser_bool).transpose()? {
            cfg.debug = x;
        }
        if let Some(x) = sgx.get("heap-size").map(deser_u64).transpose()? {
            cfg.heap_size = x;
        }
        if let Some(x) = sgx.get("ssaframesize").map(deser_u32).transpose()? {
            cfg.ssaframesize = x;
        }
        if let Some(x) = sgx.get("stack-size").map(deser_u64).transpose()? {
            cfg.stack_size = x;
        }
        if let Some(x) = sgx.get("threads").map(deser_u8).transpose()? {
            cfg.threads = x;
        }

        Ok(cfg)
    }
}

fn deser_sgx_table<'a>(
    cargo_toml: &'a DeTable<'a>,
) -> anyhow::Result<Option<&'a DeTable<'a>>> {
    let package = if let Some(package) = cargo_toml.get("package") {
        package.get_ref().as_table().context("not a table")?
    } else {
        return Ok(None);
    };

    let metadata = if let Some(metadata) = package.get("metadata") {
        metadata.get_ref().as_table().context("not a table")?
    } else {
        return Ok(None);
    };

    let sgx = if let Some(sgx) = metadata.get("fortanix-sgx") {
        sgx.get_ref().as_table().context("not a table")?
    } else {
        return Ok(None);
    };

    Ok(Some(sgx))
}

fn deser_bool(x: &Spanned<DeValue<'_>>) -> anyhow::Result<bool> {
    x.get_ref().as_bool().context("not a bool")
}

fn deser_u8(x: &Spanned<DeValue<'_>>) -> anyhow::Result<u8> {
    let x = x.get_ref().as_integer().context("not an integer")?;
    u8::from_str_radix(x.as_str(), x.radix()).map_err(anyhow::Error::new)
}

fn deser_u32(x: &Spanned<DeValue<'_>>) -> anyhow::Result<u32> {
    let x = x.get_ref().as_integer().context("not an integer")?;
    u32::from_str_radix(x.as_str(), x.radix()).map_err(anyhow::Error::new)
}

fn deser_u64(x: &Spanned<DeValue<'_>>) -> anyhow::Result<u64> {
    let x = x.get_ref().as_integer().context("not an integer")?;
    u64::from_str_radix(x.as_str(), x.radix()).map_err(anyhow::Error::new)
}
