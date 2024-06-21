use std::{io::Write, process::ExitCode, time::Instant};

use lexe_ln::logger;
use node::cli::NodeCommand;
use tracing::{error, info, info_span};

pub fn main() -> ExitCode {
    let start = Instant::now();

    // Get useful, human-readable, symbolized backtraces even in an SGX enclave.
    #[cfg(target_env = "sgx")]
    sgx_panic_backtrace::set_panic_hook();

    logger::init();

    let command = match NodeCommand::from_env() {
        Ok(Some(cmd)) => cmd,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            println!("{e:#}");
            node::cli::print_help();
            return ExitCode::FAILURE;
        }
    };

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
