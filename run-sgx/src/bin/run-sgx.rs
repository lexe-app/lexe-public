// Enable `OnceLock::get_or_try_init`
#![feature(once_cell_try)]

use std::{
    fmt,
    io::{self, Write},
    mem,
    path::PathBuf,
    pin::Pin,
    str::{self, FromStr},
    sync::OnceLock,
    task::{ready, Context, Poll},
};

use anyhow::{format_err, Result};
use argh::{EarlyExit, FromArgs, TopLevelCommand};
use object::{
    read::{SymbolMap, SymbolMapName},
    Object,
};
use rustc_demangle::{demangle, Demangle};
use tokio::io::AsyncWrite;

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
    pub bin: String,

    // TODO(phlip9): figure out why this isn't working
    /// optional path to the ".sig" enclave SIGSTRUCT file. defaults to the
    /// binary path with ".sig" instead of ".sgxs" if unset.
    #[argh(option)]
    pub sig: Option<String>,

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
    pub elf: Option<String>,
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

        let aesm_client = AesmClient::new();

        let mut device = isgx::Device::new()
            .context("Failed to init SGX device")?
            .einittoken_provider(aesm_client)
            .build();

        let bin_path = Path::new(&self.opts.bin);
        let maybe_elf_bin_path =
            self.opts.elf.as_ref().map(PathBuf::from).or_else(|| {
                let elf = bin_path.with_extension("");
                if elf.exists() {
                    Some(elf)
                } else {
                    None
                }
            });
        let mut enclave = EnclaveBuilder::new(bin_path);

        // problem: enclave can't talk to the AESM (fs access denied).
        // solution: proxy TCP connections from "aesm.local" to the local AESM
        // unix socket.
        enclave.usercall_extension(AesmProxy);

        // works for now
        enclave.dummy_signature();

        // TODO(phlip9): figure out why this isn't working
        // let maybe_sig = self.opts.sig.as_ref();
        // let sig_path = maybe_sig
        //     .map(PathBuf::from)
        //     .unwrap_or_else(|| bin_path.with_extension("sig"));
        // dbg!(&sig_path);
        // enclave
        //     .signature(sig_path)
        //     .context("Failed to read .sig sigstruct")?;

        // attach the enclave's args
        enclave.args(self.enclave_args);

        // hook stdout so we can symbolize backtraces
        if let Some(elf_bin_path) = maybe_elf_bin_path {
            ENCLAVE_ELF_BIN_PATH.set(elf_bin_path).expect(
                "ENCLAVE_ELF_BIN_PATH should never be set more than once",
            );
            let stdout = tokio::io::stdout();
            let stdout = backtrace_symbolizer_stream(stdout);
            enclave.stdout(stdout);
        }

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

// -- impl AsyncLineWriter -- //

/// Buffers writes until we hit a newline, then calls a callback on the line
/// before writing the modified line into the wrapped [`AsyncWrite`].
pub struct AsyncLineWriter<W, F> {
    inner: W,
    buf: Vec<u8>,
    line_callback: F,
    need_flush: bool,
    write_offset: usize,
}

impl<W, F> AsyncLineWriter<W, F>
where
    W: AsyncWrite + Unpin,
    F: Fn(Vec<u8>) -> Vec<u8> + Unpin,
{
    pub fn new(inner: W, line_callback: F) -> Self {
        Self {
            inner,
            buf: Vec::with_capacity(8192),
            line_callback,
            need_flush: false,
            write_offset: 0,
        }
    }

    /// Try to write the buffered (and maybe modified) line, `self.buf`, into
    /// the underlying [`AsyncWrite`]. We won't accept more input until this
    /// pending write is complete.
    fn poll_write_pending(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.need_flush {
            return Poll::Ready(Ok(()));
        }

        let write_buf = &self.buf[self.write_offset..];
        if write_buf.is_empty() {
            self.need_flush = false;
            return Poll::Ready(Ok(()));
        }

        let bytes_written = match Pin::new(&mut self.inner)
            .poll_write(cx, &self.buf[self.write_offset..])
        {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(err)) => return Poll::Ready(Err(err)),
            Poll::Ready(Ok(bytes_written)) => bytes_written,
        };

        self.write_offset += bytes_written;

        // we've written all the pending bytes. reset.
        if self.write_offset == self.buf.len() {
            self.need_flush = false;
            self.write_offset = 0;
            self.buf.clear();
        }

        Poll::Ready(Ok(()))
    }
}

impl<W, F> AsyncWrite for AsyncLineWriter<W, F>
where
    W: AsyncWrite + Unpin,
    F: Fn(Vec<u8>) -> Vec<u8> + Unpin,
{
    /// 1. first try to flush any pending write we might have buffered
    /// 2. accept and buffer more bytes from the input until we see a '\n'
    /// 3. notify the callback of a new line, which they might modify
    /// 4. move into flush mode to write the buffered, modified line before
    ///    accepting more input.
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // finish writing any pending writes first.
        ready!(self.poll_write_pending(cx))?;

        // buffer until we find a newline byte
        let newline_idx = memchr::memchr(b'\n', buf);
        let newline_idx = match newline_idx {
            None => {
                // no newline byte yet, just keep buffering
                self.buf.extend_from_slice(buf);
                return Poll::Ready(Ok(buf.len()));
            }
            Some(newline_idx) => newline_idx,
        };

        // we'll only write up to and including the newline
        let buf = &buf[..newline_idx + 1];
        let bytes_written = buf.len();

        // copy line into buf
        self.buf.extend_from_slice(buf);

        // notify the caller of the new line, they can modify it if they wish.
        let buf = mem::take(&mut self.buf);
        self.buf = (self.line_callback)(buf);

        // set mode to flush pending write
        self.need_flush = true;
        self.write_offset = 0;

        Poll::Ready(Ok(bytes_written))
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        ready!(self.poll_write_pending(cx))?;
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// -- impl Backtrace -- //

#[derive(Debug)]
struct BacktraceFrame {
    frame_idx: usize,
    instruction_ptr: usize,
    symbol_name: Option<Demangle<'static>>,
}

impl BacktraceFrame {
    fn parse_from_backtrace_line(line: &str) -> Option<Self> {
        // just some quick and dirty parsing code

        // example backtrace line:
        // "  11: 0x3933f\n"

        fn parse_hex(s: &str) -> Option<usize> {
            usize::from_str_radix(s, 16).ok()
        }

        let (frame_idx, rest) =
            line.split_once(": ").and_then(|(prefix, rest)| {
                let prefix = prefix.trim_start();
                let frame_idx = usize::from_str(prefix).ok()?;
                Some((frame_idx, rest))
            })?;

        let instruction_ptr =
            rest.trim_end().strip_prefix("0x").and_then(parse_hex)?;

        Some(Self {
            frame_idx,
            instruction_ptr,
            symbol_name: None,
        })
    }
}

impl fmt::Display for BacktraceFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let idx = self.frame_idx;
        let ip = self.instruction_ptr;

        write!(f, "{idx:>4}: ip={ip:#x}")?;
        if let Some(symbol_name) = &self.symbol_name {
            write!(f, " : {symbol_name:#}")?;
        }
        Ok(())
    }
}

/// A stream that symbolizes backtrace lines. When we see a backtrace line,
/// try to symbolize the line (convert the raw addresses to human-readable
/// symbols).
pub fn backtrace_symbolizer_stream<W: AsyncWrite + Unpin>(
    stream: W,
) -> impl AsyncWrite + Unpin {
    AsyncLineWriter::new(stream, move |mut line_buf| {
        // quickly avoid processing long lines, which definitely aren't
        // backtrace frames
        if line_buf.len() >= 32 {
            return line_buf;
        }

        // only parse utf8-encoded lines
        let line_str = match str::from_utf8(&line_buf) {
            Ok(s) => s,
            Err(_) => return line_buf,
        };

        // try to parse a backtrace frame from this line
        let mut frame =
            match BacktraceFrame::parse_from_backtrace_line(line_str) {
                Some(frame) => frame,
                None => return line_buf,
            };

        // we found a backtrace frame, try to lazily load the elf binary and
        // extract the symbol table
        //
        // 1. Try to resolve the symbol name.
        // 2. The symbol name comes out mangled, e.g.
        //    "_ZN11sgx_enclave4main17h26101c5064988311E", so we need to
        //    demangle to make it pretty, like "sgx_enclave::main".
        frame.symbol_name = enclave_elf_symbol_map()
            .get(frame.instruction_ptr as u64)
            .map(|symbol| demangle(symbol.name()));

        // in the current line, replace the raw backtrace frame with the
        // symbolized version.
        line_buf.clear();
        writeln!(&mut line_buf, "{frame}")
            .expect("Formatting into a Vec<u8> should never fail");
        line_buf
    })
}

// -- lazy load symbol map -- //

fn io_err_other<E>(err: E) -> io::Error
where
    E: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    io::Error::new(io::ErrorKind::Other, err)
}

// The symbol names that `object` parses from a binary are just references to
// parts of the binary, so they can't live longer than the binary itself. This
// is significantly easier if all the lifetimes are 'static, so we just stuff
// these intermediate values into global `OnceLock`s.

static ENCLAVE_ELF_BIN_PATH: OnceLock<PathBuf> = OnceLock::new();

fn enclave_elf_bin_bytes() -> io::Result<&'static [u8]> {
    static ENCLAVE_ELF_BIN_BYTES: OnceLock<Vec<u8>> = OnceLock::new();

    ENCLAVE_ELF_BIN_BYTES
        .get_or_try_init(|| -> io::Result<Vec<u8>> {
            let path = ENCLAVE_ELF_BIN_PATH
                .get()
                .ok_or_else(|| io_err_other("ENCLAVE_ELF_BIN_PATH not set"))?;
            std::fs::read(path).map_err(|err| {
                eprintln!("run-sgx: error reading enclave elf binary: {err}");
                err
            })
        })
        .map(Vec::as_slice)
}

fn enclave_elf_object(
) -> io::Result<&'static object::File<'static, &'static [u8]>> {
    static ENCLAVE_ELF_OBJECT: OnceLock<object::File<'static, &'static [u8]>> =
        OnceLock::new();

    ENCLAVE_ELF_OBJECT.get_or_try_init(|| {
        let bytes = enclave_elf_bin_bytes()?;
        object::File::parse(bytes)
            .map_err(|err| {
                eprintln!("run-sgx: error parsing enclave elf binary as an elf object: {err}");
                io::Error::new(io::ErrorKind::Other, err)
            })
    })
}

fn enclave_elf_symbol_map() -> &'static SymbolMap<SymbolMapName<'static>> {
    static ENCLAVE_ELF_SYMBOL_MAP: OnceLock<SymbolMap<SymbolMapName<'static>>> =
        OnceLock::new();

    ENCLAVE_ELF_SYMBOL_MAP.get_or_init(|| {
        enclave_elf_object()
            .map(|obj| obj.symbol_map())
            // just return an empty symbol map if there was some error
            // reading or parsing the elf binary
            .unwrap_or_else(|_| SymbolMap::new(Vec::new()))
    })
}

// -- main -- //

fn main() {
    // Note: can't just use `argh::from_env` here b/c we need to parse out the
    // enclave args after the "--"

    let args = argh::from_env::<Args>();
    let result = args.run();

    if let Err(err) = result {
        eprintln!("run-sgx error: {err:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_backtrace_frame() {
        let frame = BacktraceFrame::parse_from_backtrace_line("  43: 0x2f648a")
            .unwrap();
        assert_eq!(frame.frame_idx, 43);
        assert_eq!(frame.instruction_ptr, 0x2f648a);

        assert!(BacktraceFrame::parse_from_backtrace_line("").is_none());
        assert!(BacktraceFrame::parse_from_backtrace_line("foo bar").is_none());
        assert!(BacktraceFrame::parse_from_backtrace_line(
            "enclave panic: panicked at 'failed to spawn thread: Os'"
        )
        .is_none());
    }
}
