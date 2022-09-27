use std::io::Write;
use std::process::ExitCode;
use std::time::Instant;

use lexe_ln::logger;
use node::cli::Args;
use tracing::{error, info};

pub fn main() -> ExitCode {
    let start = Instant::now();

    // Get useful, human-readable, symbolized backtraces even in an SGX enclave.
    #[cfg(target_env = "sgx")]
    sgx_panic_backtrace::set_panic_hook();

    logger::init();

    let args = argh::from_env::<Args>();
    let result = args.run();

    let time_elapsed = start.elapsed();
    let exit_code = match result {
        Ok(()) => {
            info!("completed without error: time elapsed: {time_elapsed:?}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            error!(
                "error running command: {error:#}, time_elapsed: {time_elapsed:?}",
            );
            ExitCode::FAILURE
        }
    };

    // ensure stdout flushes so we don't lose any buffered log messages.
    std::io::stdout().flush().unwrap();

    exit_code
}
