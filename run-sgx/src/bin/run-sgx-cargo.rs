#![allow(dead_code)]

use std::env;
use std::process::Command;

use anyhow::{format_err, Context, Result};
use argh::{EarlyExit, FromArgs, TopLevelCommand};
use serde::Deserialize;

const DEBUG: bool = true;
const HEAP_SIZE: u64 = 0x2000000; // 32 MiB
const SSAFRAMESIZE: u32 = 1;
const STACK_SIZE: u32 = 0x20000; // 128 KiB
const THREADS: u32 = 4;

#[derive(Debug)]
pub struct Args {
    pub opts: Options,
    pub enclave_args: Vec<String>,
}

/// TODO
#[derive(Debug, FromArgs)]
pub struct Options {
    /// path to the compiled enclave binary in standard ELF format, not ".sgxs"
    #[argh(positional)]
    pub elf_bin: String,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case")]
struct Target {
    heap_size: Option<u64>,
    ssaframesize: Option<u32>,
    stack_size: Option<u32>,
    threads: Option<u32>,
    debug: Option<bool>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case")]
struct Metadata {
    #[serde(default)]
    fortanix_sgx: Target,
}

#[derive(Deserialize, Debug)]
struct Package {
    #[serde(default)]
    metadata: Metadata,
}

#[derive(Deserialize, Debug)]
struct Config {
    package: Package,
}

// -- impl Args -- //

impl Args {
    pub fn run(self) -> Result<()> {
        for (key, val) in env::vars() {
            println!("{key} = {val}");
        }

        let _target_dir = env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| format_err!("missing $CARGO_MANIFEST_DIR env"))?;

        // TODO(phlip9): dump .sgxs and .sig into tempdir?

        Ok(())
    }
}

// Manually implement `FromArgs`. We need this b/c argh's parsing seems broken
// for "--" separators (it includes positionals pre separator in the positionals
// post separator).
//
// Ex: `run-sgx -- foo.sgxs` should error "file arg missing", but instead
// parses it into `run-sgx`'s args, not the enclave args.
impl FromArgs for Args {
    fn from_args(cmd_name: &[&str], args: &[&str]) -> Result<Self, EarlyExit> {
        let (our_args, enclave_args) = args.split_at(1);
        let opts = Options::from_args(cmd_name, our_args)?;

        let enclave_args = enclave_args.iter().map(|s| s.to_string()).collect();

        Ok(Self { opts, enclave_args })
    }
}

impl TopLevelCommand for Args {}

// -- utils -- //

fn run_cmd(mut cmd: Command) -> Result<()> {
    // run the command and collect its status
    let status = cmd
        .status()
        .with_context(|| format!("Failed to run command: '{cmd:?}'"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format_err!(
            "Command exited with a non-zero status: {}, cmd: '{cmd:?}'",
            status
        ))
    }
}

// -- main -- //

fn main() {
    let args = argh::from_env::<Args>();
    println!("args: {args:?}");

    if let Err(err) = args.run() {
        eprintln!("Error: {err:#?}");
        std::process::exit(1);
    }
}
