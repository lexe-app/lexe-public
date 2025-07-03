use std::{env, io::Write, process::ExitCode, time::Instant};

use lexe_ln::logger;
use node::cli::NodeCommand;
use tracing::{error, info, info_span};

pub fn main() -> ExitCode {
    // Disable _non-panic_ `std::backtrace::Backtrace::capture()`.
    //
    // 2025-02-04: In SGX and outside a panic, `Backtrace::capture()` appears to
    // enter an infinite loop, causing the caller to hang indefinitely.
    //
    // See: <https://docs.rs/anyhow/latest/anyhow/struct.Error.html#method.backtrace>
    unsafe { std::env::set_var("RUST_LIB_BACKTRACE", "0") };

    let start = Instant::now();

    let command = match NodeCommand::from_env() {
        Ok(Some(cmd)) => cmd,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            println!("{e:#}");
            node::cli::print_help();
            return ExitCode::FAILURE;
        }
    };

    // SAFETY: All our thread spawning is in `command.run()`, so we're in a
    // single-threaded environment at this point.
    // Also, in SGX, this fn is safe because there is a lock around the envs.
    unsafe {
        // We don't set `RUST_LOG` so `logger::init` can enforce a log policy.
        if let Some(value) = command.rust_backtrace() {
            env::set_var("RUST_BACKTRACE", value);
        }
    }

    // TODO(max): For safety, this needs to be false.
    let allow_trace = true;
    logger::init(command.rust_log(), allow_trace);

    let span = match command {
        NodeCommand::Mega(_) => info_span!("(mega)"),
    };

    let exit_code = span.in_scope(|| {
        let result = command.run();
        let elapsed = start.elapsed();

        match result {
            Ok(()) => {
                info!("Node completed successfully. Time elapsed: {elapsed:?}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                error!("Node errored: {e:#}; Time elapsed: {elapsed:?}");
                ExitCode::FAILURE
            }
        }
    });

    // ensure stdout flushes so we don't lose any buffered log messages.
    std::io::stdout().flush().unwrap();

    exit_code
}
