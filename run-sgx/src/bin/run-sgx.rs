use std::{
    fmt,
    io::Write,
    path::PathBuf,
    str::{self, FromStr},
    sync::{LazyLock, OnceLock},
};

use anyhow::{Context as _, Result, format_err};
use argh::{EarlyExit, FromArgs, TopLevelCommand};
use object::{
    Object,
    read::{SymbolMap, SymbolMapName},
};
use rustc_demangle::{Demangle, demangle};

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
    #[argh(positional)]
    pub bin: PathBuf,

    /// create a just-in-time dummy enclave signature with DEBUG enabled
    /// instead of using any adjacent ".sigstruct" file. Useful when
    /// developing to avoid the ".sgxs" signing ceremony.
    #[argh(switch)]
    pub debug: bool,

    /// optional path to the ".sigstruct" enclave SIGSTRUCT file. defaults to
    /// the binary path with ".sigstruct" instead of ".sgxs" if unset.
    /// `run-sgx` will fail if it can't find a ".sigstruct", unless
    /// `--debug` is set.
    #[argh(option)]
    pub sigstruct: Option<PathBuf>,

    /// optional path to the original elf binary, before going through the
    /// ".sgxs" conversion.
    ///
    /// Used to symbolize raw backtrace addresses in the event of a panic.
    ///
    /// If unset, will attempt to use the ".sgxs" binary path without the
    /// extension.
    ///
    /// If the file doesn't exist, backtraces just won't be symbolized.
    #[argh(option)]
    pub elf: Option<PathBuf>,
}

// -- impl Args -- //

impl Args {
    // Can only load real enclaves on x86_64-unknown-linux
    #[cfg(all(target_arch = "x86_64", target_os = "linux"))]
    pub fn run(self) -> Result<()> {
        use std::path::Path;

        use aesm_client::AesmClient;
        use anyhow::Context;
        use common::enclave::Measurement;
        use enclave_runner::EnclaveBuilder;
        use run_sgx::aesm_proxy::AesmProxy;
        use sgxs_loaders::isgx;

        let aesm_client = AesmClient::new();

        let mut device = isgx::Device::new()
            .context("Failed to init SGX device")?
            .einittoken_provider(aesm_client)
            .build();

        // use the passed ELF binary arg or look for an adjacent ELF binary
        // ("<bin>.sgxs" -> "<bin>") that contains symbols for backtraces.
        let bin_path: &Path = &self.opts.bin;
        let maybe_elf_bin_path = self.opts.elf.clone().or_else(|| {
            let elf = bin_path.with_extension("");
            if elf.exists() { Some(elf) } else { None }
        });
        // set the ELF binary path so we can symbolize backtraces
        if let Some(elf_bin_path) = maybe_elf_bin_path {
            ENCLAVE_ELF_BIN_PATH.set(elf_bin_path).expect(
                "ENCLAVE_ELF_BIN_PATH should never be set more than once",
            );
        }
        let mut enclave = EnclaveBuilder::new(bin_path);

        // problem: enclave can't talk to the AESM (no filesystem in enclave).
        // solution: proxy TCP connections from "aesm.local" to the local AESM
        // unix socket.
        enclave.usercall_extension(AesmProxy);

        // load enclave sigstruct
        if !self.opts.debug {
            // Load sigstruct from arg path or adjacent .sigstruct file
            let sigstruct_path = self
                .opts
                .sigstruct
                .clone()
                .unwrap_or_else(|| bin_path.with_extension("sigstruct"));
            let sigstruct_path_ref: &Path = &sigstruct_path;
            enclave.signature(sigstruct_path_ref).with_context(|| {
                format!(
                    "Failed to read sigstruct: '{}'. If this is just for local development, try using '--debug'.",
                    sigstruct_path_ref.display()
                )
            })?;
        } else {
            // Create a debug sigstruct with the dev signer keypair.
            let bin_file = std::fs::File::open(bin_path)
                .context("Failed to open .sgxs binary")?;
            let measurement = Measurement::compute_from_sgxs(bin_file)
                .context("Failed to compute SGX binary measurement")?;
            let key = sgxs_sign::KeyPair::dev_signer();
            // SGX DEBUG mode: disables memory protections
            let debug = false;
            let sigstruct = key
                .sign_sgxs(measurement, debug, None)
                .context("Failed to sign .sgxs")?;
            enclave.sigstruct(sigstruct);
        }

        // attach the enclave's args
        enclave.args(self.enclave_args);

        // // TODO(phlip9): for some reason, this causes the runner to hang if
        // // the enclave ever panics...
        // enclave.forward_panics(true);

        let enclave_cmd = enclave
            .build(&mut device)
            .map_err(|err| format_err!("{err:#?}"))
            .context("Failed to build enclave")?;

        // TODO(phlip9): catch SIGBUS to print nice error msg on stack overflow?

        // run the enclave
        let res = enclave_cmd.run();

        // if the enclave panics, return the panic message with a symbolized
        // backtrace.
        if let Err(err) = res {
            use enclave_runner::EnclavePanic;

            // when the enclave is built in debug mode, it will dump panics with
            // unsymbolized backtraces into a 1000 B "debug buffer" shared with
            // the untrusted host.
            //
            // On panic we'll try reading that buffer and symbolize any
            // backtraces in the output.
            //
            // An enclave Cargo.toml configured to build in debug mode:
            //
            // ```toml
            // [package.metadata.fortanix-sgx]
            // debug = true
            // ```
            let panic_str = match err.downcast::<EnclavePanic>() {
                Ok(EnclavePanic::NoDebugBuf) =>
                    return Err(format_err!(
                        "enclave panicked without writing any debug info!"
                    )),
                Err(err) => return Err(err),
                Ok(EnclavePanic::DebugStr(s)) => s,
                Ok(EnclavePanic::DebugBuf(b)) =>
                    String::from_utf8_lossy(&b).into_owned(),
            };

            let symbolized_panic_str = symbolize_panic_output(&panic_str);
            return Err(anyhow::Error::msg(symbolized_panic_str));
        }

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

// -- impl BacktraceFrame -- //

/// Permissively parse backtrace frame lines from the panic output buffer
/// and symbolize them.
///
/// Ex:
///    "  15:            0x25775 - <unknown>"
/// -> "  15:            0x25775 - sgx_test::foo"
#[cfg_attr(
    not(all(target_arch = "x86_64", target_os = "linux")),
    allow(dead_code)
)]
fn symbolize_panic_output(panic_output: &str) -> String {
    use std::fmt::Write;

    let mut buf = String::with_capacity(panic_output.len());
    for line in panic_output.lines() {
        let maybe_frame = BacktraceFrame::parse_from_backtrace_line(line);
        match maybe_frame {
            Some(mut frame) => {
                frame.symbolize();
                write!(&mut buf, "{frame}").unwrap();
            }
            None => {
                buf.push_str(line);
            }
        }
        buf.push('\n');
    }
    buf
}

#[cfg_attr(test, derive(Debug))]
struct BacktraceFrame {
    frame_idx: usize,
    instruction_ptr: usize,
    symbol_name: Option<Demangle<'static>>,
}

impl BacktraceFrame {
    fn parse_from_backtrace_line(line: &str) -> Option<Self> {
        // quickly avoid processing long lines, which definitely aren't
        // backtrace frames
        if line.len() >= 40 {
            return None;
        }

        // example backtrace line:
        // "  11:         0x3933f - <unknown>\n"

        fn parse_hex(s: &str) -> Option<usize> {
            usize::from_str_radix(s, 16).ok()
        }

        let (frame_idx, rest) =
            line.split_once(": ").and_then(|(prefix, rest)| {
                let prefix = prefix.trim_start();
                let frame_idx = usize::from_str(prefix).ok()?;
                let rest = rest.trim_start();
                Some((frame_idx, rest))
            })?;

        let (ip, _rest) = rest.split_once(" - ").unwrap_or((rest, ""));

        let instruction_ptr =
            ip.trim_end().strip_prefix("0x").and_then(parse_hex)?;

        Some(Self {
            frame_idx,
            instruction_ptr,
            symbol_name: None,
        })
    }

    // 1. Lazy load the elf binary and extract the symbol table
    // 2. Try to resolve the symbol name.
    // 3. The symbol name comes out mangled, e.g.
    //    "_ZN11sgx_enclave4main17h26101c5064988311E", so we need to demangle to
    //    make it pretty, like "sgx_enclave::main".
    fn symbolize(&mut self) {
        self.symbol_name = enclave_elf_symbol_map()
            .get(self.instruction_ptr as u64)
            .map(|symbol| demangle(symbol.name()));
    }
}

impl fmt::Display for BacktraceFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let frame_idx = self.frame_idx;
        let frame_ip = self.instruction_ptr;

        write!(f, "{frame_idx:4}: {frame_ip:#18x} - ")?;
        match &self.symbol_name {
            Some(symbol_name) => write!(f, "{symbol_name:#}")?,
            None => write!(f, "<unknown>")?,
        }
        Ok(())
    }
}

// -- lazy load symbol map -- //

/// We'll set this value at startup with the corresponding `--elf <path>` arg.
/// See: [`Options::elf`].
static ENCLAVE_ELF_BIN_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Lazily read, parse, and load the enclave symbol table so we can symbolize
/// enclave panic backtraces. We wan't to avoid this large memory overhead
/// during normal operation, since panics are usually rare.
///
/// This contains a `LazyLock` inside so only load this once, even if there are
/// multiple panics.
fn enclave_elf_symbol_map() -> &'static SymbolMap<SymbolMapName<'static>> {
    fn read_enclave_elf_symbol_map()
    -> anyhow::Result<SymbolMap<SymbolMapName<'static>>> {
        let path = ENCLAVE_ELF_BIN_PATH
            .get()
            .context("ENCLAVE_ELF_BIN_PATH not set")?;

        // We'll hold onto these bytes for the rest of the program lifetime.
        // Let's just `Vec::leak` to get a `&'static` that we can return
        // references to.
        let elf_bytes: &'static [u8] = &*std::fs::read(path)
            .context("Failed to read enclave ELF binary")?
            .leak();

        let elf_object = object::File::parse(elf_bytes)
            .context("Failed to parse enclave ELF binary as an ELF object")?;

        Ok(elf_object.symbol_map())
    }

    static ENCLAVE_ELF_SYMBOL_MAP: LazyLock<SymbolMap<SymbolMapName<'static>>> =
        LazyLock::new(|| {
            read_enclave_elf_symbol_map()
                .map_err(|err| {
                    eprintln!(
                        "run-sgx: Failed to build enclave symbol map: {err:#}"
                    )
                })
                // Just return an empty symbol map if there was some error
                // reading or parsing the elf binary.
                .unwrap_or_else(|()| SymbolMap::new(Vec::new()))
        });

    &ENCLAVE_ELF_SYMBOL_MAP
}

// -- main -- //

fn main() {
    // Note: can't just use `argh::from_env` here b/c we need to parse out the
    // enclave args after the "--"

    let args = argh::from_env::<Args>();
    let result = args.run();

    if let Err(err) = result {
        eprintln!("run-sgx error: {err:#}");
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        std::process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_backtrace() {
        let input = r#"thread 'main' panicked at public/sgx-test/src/main.rs:62:5:
foo failed!
stack backtrace:
   0:            0x2104c - <unknown>
   1:             0xdc8c - <unknown>
   2:            0x204be - <unknown>
   3:            0x21e85 - <unknown>
   4:            0x230c9 - <unknown>
   5:            0x224e1 - <unknown>
   6:            0x21259 - <unknown>
   7:            0x22127 - <unknown>
   8:             0xce35 - <unknown>
   9:            0x1878b - <unknown>
  10:            0x18845 - <unknown>
  11:            0x186c9 - <unknown>
  12:            0x186ac - <unknown>
  13:            0x1de31 - <unknown>
  14:            0x18b04 - <unknown>
  15:            0x25775 - <unknown>
"#;

        // parsing and symbolizing w/o symbols available will just roundtrip
        let output = symbolize_panic_output(input);
        assert_eq!(input, output);
    }

    #[test]
    fn test_parse_backtrace_frame() {
        let frame = BacktraceFrame::parse_from_backtrace_line(
            "  12:            0x186ac - <unknown>",
        )
        .unwrap();
        assert_eq!(frame.frame_idx, 12);
        assert_eq!(frame.instruction_ptr, 0x186ac);
        assert!(frame.symbol_name.is_none());

        assert!(BacktraceFrame::parse_from_backtrace_line("").is_none());
        assert!(BacktraceFrame::parse_from_backtrace_line("foo bar").is_none());
        assert!(
            BacktraceFrame::parse_from_backtrace_line(
                "enclave panic: panicked at 'failed to spawn thread: Os'"
            )
            .is_none()
        );
    }
}
