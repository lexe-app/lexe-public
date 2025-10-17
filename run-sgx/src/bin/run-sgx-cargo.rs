//! TODO

use std::{path::PathBuf, process::Command};

use anyhow::{Context, Result, format_err};
use argh::{EarlyExit, FromArgs, TopLevelCommand};

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
    pub elf_bin: PathBuf,
}

// -- impl Args -- //

impl Args {
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    pub fn run(self) -> Result<()> {
        use std::{env, path::Path};

        use sgx_toml::FortanixSgxConfig;

        // 1. read SGX config from crate's Cargo.toml

        // CARGO_MANIFEST_DIR is the directory containing the Cargo.toml of the
        // package cargo just built.
        let target_dir = env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| format_err!("missing `CARGO_MANIFEST_DIR` env var: this tool expects `cargo` to run it"))?;

        let mut cargo_toml_path = PathBuf::from(target_dir);
        cargo_toml_path.push("Cargo.toml");

        let sgx_config = sgx_toml::read_fortanix_sgx_config(&cargo_toml_path)
            .expect("Couldn't read Fortanix SGX config");
        let FortanixSgxConfig {
            debug,
            heap_size,
            ssaframesize,
            stack_size,
            threads,
        } = sgx_config;

        // 2. convert compiled ELF binary to SGXS format

        let elf_bin_path = Path::new(&self.opts.elf_bin);

        let mut sgxs_bin_path = elf_bin_path.to_path_buf();
        sgxs_bin_path.set_extension("sgxs");

        let mut ftxsgx_elf2sgxs_cmd = Command::new("ftxsgx-elf2sgxs");
        ftxsgx_elf2sgxs_cmd
            .arg(elf_bin_path)
            .arg("--output")
            .arg(&sgxs_bin_path)
            .arg("--heap-size")
            .arg(heap_size.to_string())
            .arg("--ssaframesize")
            .arg(ssaframesize.to_string())
            .arg("--stack-size")
            .arg(stack_size.to_string())
            .arg("--threads")
            .arg(threads.to_string());

        if debug {
            ftxsgx_elf2sgxs_cmd.arg("--debug");
        }

        run_cmd(ftxsgx_elf2sgxs_cmd)
            .context("Failed to convert enclave binary to .sgxs")?;

        // 3. run the enclave with `run-sgx`

        let mut run_sgx_cmd = Command::new("run-sgx");
        run_sgx_cmd
            .arg(&sgxs_bin_path)
            .arg("--elf")
            .arg(elf_bin_path)
            // sign as DEBUG enclave w/ dummy keypair just before running. this
            // avoids tedious sign-sgxs infrastructure while developing.
            .arg("--debug")
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

#[allow(dead_code)]
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
