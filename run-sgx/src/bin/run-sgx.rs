use anyhow::{format_err, Result};
use argh::{EarlyExit, FromArgs, TopLevelCommand};

#[derive(Debug)]
pub struct Args {
    pub opts: Options,
    pub enclave_args: Vec<String>,
}

/// Run an SGX enclave binary in ".sgxs" format.
///
/// Pass args to the enclave like `run-sgx foo.sgxs -- arg-1
/// arg-2 ..`.
///
/// NOTE: secrets must not be passed to the enclave via cli args.
#[derive(Debug, FromArgs)]
pub struct Options {
    /// path to the ".sgxs" enclave binary
    #[argh(option, short = 'b')]
    pub bin: String,

    /// path to the ".sig" enclave SIGSTRUCT file. defaults to the binary path
    /// with ".sig" instead of ".sgxs" if unset.
    #[argh(option, short = 's')]
    pub sig: Option<String>,
}

// -- impl Args -- //

impl Args {
    // Can only load real enclaves on x86_64-unknown-linux
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    pub fn run(self) -> Result<()> {
        use std::path::Path;

        use aesm_client::AesmClient;
        use anyhow::Context;
        use enclave_runner::EnclaveBuilder;
        use run_sgx::aesm_proxy::AesmProxy;
        use sgxs_loaders::isgx;

        let bin_path = Path::new(&self.opts.bin);

        let aesm_client = AesmClient::new();

        let mut device = isgx::Device::new()
            .context("Failed to init SGX device")?
            .einittoken_provider(aesm_client)
            .build();

        let mut enclave = EnclaveBuilder::new(bin_path);

        // problem: enclave can't talk to the AESM (fs access denied).
        // solution: proxy TCP connections from "aesm.local" to the local AESM
        // unix socket.
        enclave.usercall_extension(AesmProxy);

        // EnclaveBuilder already adds the "coresident" .sig file by default.
        if let Some(sig) = self.opts.sig.as_ref() {
            let sig_path = Path::new(sig);
            enclave
                .signature(sig_path)
                .context("Failed to read .sig sigstruct")?;
        }

        // attach the enclave's args
        enclave.args(self.enclave_args);

        // TODO(phlip9): get this working again
        // // hook stdout so we can symbolize backtraces
        // let stdout = tokio02::io::stdout();
        // let stdout = backtrace_symbolizer_stream(stdout);
        // enclave.stdout(stdout);

        // // TODO(phlip9): for some reason, this causes the runner to hang if
        // the enclave ever panics...
        // enclave.forward_panics(true);

        let enclave_cmd = enclave
            .build(&mut device)
            .map_err(|err| format_err!("{err:#?}"))
            .context("Failed to build enclave")?;

        // TODO(phlip9): catch SIGBUS to print nice error msg on stack overflow?

        enclave_cmd
            .run()
            .map_err(|err| format_err!("{err:#?}"))
            .context("SGX enclave error")?;
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
        let (our_args, enclave_args) = split_args(args);
        let opts = Options::from_args(cmd_name, our_args)?;

        let enclave_args = enclave_args.iter().map(|s| s.to_string()).collect();

        Ok(Self { opts, enclave_args })
    }
}

impl TopLevelCommand for Args {}

/// Split the args on the first "--" separator (if there is one).
fn split_args<'a>(args: &'a [&'a str]) -> (&'a [&'a str], &'a [&'a str]) {
    let maybe_sep_idx = args.iter().position(|&arg| arg == "--");

    match maybe_sep_idx {
        Some(sep_idx) => args.split_at(sep_idx + 1), // +1 to trim "--"
        None => (args, &[]),
    }
}

fn main() {
    // Note: can't just use `argh::from_env` here b/c we need to parse out the
    // enclave args after the "--"

    let args = argh::from_env::<Args>();
    let result = args.run();

    if let Err(err) = result {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
