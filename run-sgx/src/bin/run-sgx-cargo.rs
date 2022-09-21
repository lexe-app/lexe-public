//! TODO

#![allow(dead_code)]

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
use std::{env, fs};

use anyhow::{format_err, Context, Result};
use argh::{EarlyExit, FromArgs, TopLevelCommand};
use serde::Deserialize;

// const DEBUG_SIGNER_KEY_PEM_BYTES: &[u8] =
//     std::include_bytes!("../../data/debug-signer-key.pem");

// default SGX config
const DEBUG: bool = true;
const HEAP_SIZE: u64 = 0x0200_0000; // 32 MiB
const SSAFRAMESIZE: u32 = 1;
const STACK_SIZE: u32 = 0x0002_0000; // 128 KiB
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

/// The subset of a crate's `Cargo.toml` with the SGX config.
///
/// ```toml
/// [package.metadata.fortanix_sgx]
/// heap_size = 0x2000000
/// ssaframesize = 1
/// stack_size = 0x20000
/// threads = 4
/// debug = true
/// ```
#[derive(Deserialize, Debug)]
pub struct SgxConfig {
    package: Package,
}

#[derive(Deserialize, Debug)]
pub struct Package {
    #[serde(default)]
    metadata: Metadata,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Metadata {
    #[serde(default)]
    fortanix_sgx: Target,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Target {
    heap_size: Option<u64>,
    ssaframesize: Option<u32>,
    stack_size: Option<u32>,
    threads: Option<u32>,
    debug: Option<bool>,
}

// -- impl Args -- //

impl Args {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    pub fn run(self) -> Result<()> {
        // 1. read SGX config from crate's Cargo.toml

        // CARGO_MANIFEST_DIR is the directory containing the Cargo.toml of the
        // package cargo just built.
        let target_dir = env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| format_err!("missing `CARGO_MANIFEST_DIR` env var: this tool expects `cargo` to run it"))?;

        let mut cargo_toml = PathBuf::from(target_dir);
        cargo_toml.push("Cargo.toml");

        let config_str = fs::read_to_string(&cargo_toml)
            .context("Failed to read Cargo.toml")?;

        let config: SgxConfig = toml::from_str(&config_str)
            .context("Failed to deserialize Cargo.toml")?;

        let sgx_config = config.package.metadata.fortanix_sgx;

        let heap_size = sgx_config.heap_size.unwrap_or(HEAP_SIZE);
        let ssaframesize = sgx_config.ssaframesize.unwrap_or(SSAFRAMESIZE);
        let stack_size = sgx_config.stack_size.unwrap_or(STACK_SIZE);
        let threads = sgx_config.threads.unwrap_or(THREADS);
        let debug = sgx_config.debug.unwrap_or(DEBUG);

        // 2. convert compiled ELF binary to SGXS format

        // TODO(phlip9): inline? would remove error-prone setup step

        let elf_bin_path = Path::new(&self.opts.elf_bin);

        let mut sgxs_bin_path = elf_bin_path.to_path_buf();
        sgxs_bin_path.set_extension("sgxs");

        let mut ftxsgx_elf2sgxs_cmd = Command::new("ftxsgx-elf2sgxs");
        ftxsgx_elf2sgxs_cmd
            .arg(elf_bin_path)
            .arg("--output")
            .arg(&sgxs_bin_path)
            .arg("--heap-size")
            .arg(&heap_size.to_string())
            .arg("--ssaframesize")
            .arg(&ssaframesize.to_string())
            .arg("--stack-size")
            .arg(&stack_size.to_string())
            .arg("--threads")
            .arg(&threads.to_string());

        if debug {
            ftxsgx_elf2sgxs_cmd.arg("--debug");
        }

        run_cmd(ftxsgx_elf2sgxs_cmd)
            .context("Failed to convert enclave binary to .sgxs")?;

        // 3. sign `<enclave-binary>.sgxs` with a dummy key (for now) to get a
        //    serialized `Sigstruct` as `<enclave-binary>.sig`.

        // TODO(phlip9): inline? would remove error-prone setup step

        // TODO(phlip9): figure out why this isn't working
        // // dump debug signer key to file
        // let mut key_path = sgxs_bin_path.clone();
        // key_path.set_file_name("debug-signer-key.pem");
        //
        // fs::write(&key_path, &DEBUG_SIGNER_KEY_PEM_BYTES).with_context(
        //     || {
        //         format!(
        //             "Failed to write debug key file: {}",
        //             key_path.display(),
        //         )
        //     },
        // )?;
        //
        // let mut sigstruct_path = sgxs_bin_path.clone();
        // sigstruct_path.set_extension("sig");
        //
        // let mut sgxs_sign_cmd = Command::new("sgxs-sign");
        // sgxs_sign_cmd
        //     // input .sgxs
        //     .arg(&sgxs_bin_path)
        //     // output .sig sigstruct
        //     .arg(&sigstruct_path)
        //     .arg("--key")
        //     .arg(&key_path);
        //
        // if debug {
        //     sgxs_sign_cmd.arg("--debug");
        // }
        //
        // run_cmd(sgxs_sign_cmd).context("Failed to sign enclave")?;

        // 4. run the enclave with `run-sgx`

        let mut run_sgx_cmd = Command::new("run-sgx");
        run_sgx_cmd
            .arg(&sgxs_bin_path)
            .arg("--elf")
            .arg(elf_bin_path)
            .arg("--")
            .args(self.enclave_args);

        run_cmd(run_sgx_cmd).context("Failed to run enclave")?;

        Ok(())
    }

    #[cfg(not(all(target_arch = "x86_64", target_os = "linux")))]
    pub fn run(self) -> Result<()> {
        Err(format_err!(
            "unsupported platform: can only run SGX enclaves on x86_64-linux"
        ))
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

    if let Err(err) = args.run() {
        eprintln!("Error: {err:#?}");
        std::process::exit(1);
    }
}
