use std::time::Instant;

use lexe_ln::logger;
use node::cli::Args;
use tracing::{debug, error};

pub fn main() {
    let start = Instant::now();
    logger::init();

    let args = argh::from_env::<Args>();
    let result = args.run();

    let time_elapsed = start.elapsed();
    match result {
        Ok(()) => debug!(?time_elapsed, "completed without error"),
        Err(error) => {
            error!(?time_elapsed, "error running command: {:#}", error);
            std::process::exit(1);
        }
    }
}
