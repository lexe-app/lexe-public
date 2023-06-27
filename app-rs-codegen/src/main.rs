use std::process::ExitCode;

fn main() -> ExitCode {
    let args = argh::from_env::<app_rs_codegen::Args>();
    match args.run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            println!("\napp-rs-codegen: error: {err:#}");
            ExitCode::FAILURE
        }
    }
}
