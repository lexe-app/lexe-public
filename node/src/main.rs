use std::{io::Write, process::ExitCode, time::Instant};

use lexe_ln::logger;
use node::cli::NodeArgs;
use tracing::{error, info};

pub fn main() -> ExitCode {
    let start = Instant::now();

    // Get useful, human-readable, symbolized backtraces even in an SGX enclave.
    #[cfg(target_env = "sgx")]
    sgx_panic_backtrace::set_panic_hook();

    logger::init();

    let args = argh::from_env::<NodeArgs>();
    let result = args.run();
    let elapsed = start.elapsed();

    let exit_code = match result {
        Ok(()) => {
            info!("Node completed successfully. Time elapsed: {elapsed:?}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("Node errored: {e:#}; Time elapsed: {elapsed:?}");
            ExitCode::FAILURE
        }
    };

    // ensure stdout flushes so we don't lose any buffered log messages.
    std::io::stdout().flush().unwrap();

    exit_code
}
