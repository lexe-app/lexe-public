use std::{env, io::Write, process::ExitCode, time::Instant};

use lexe_ln::logger;
use node::cli::NodeCommand;
use tracing::{error, info, info_span};

pub fn main() -> ExitCode {
    let start = Instant::now();

    // Get useful, human-readable, symbolized backtraces even in an SGX enclave.
    #[cfg(target_env = "sgx")]
    sgx_panic_backtrace::set_panic_hook();

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

    logger::init(command.rust_log());

    let span = match command {
        NodeCommand::Run(_) => info_span!("(node)"),
        NodeCommand::Provision(_) => info_span!("(node-provision)"),
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
