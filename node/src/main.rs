use node::cli::Args;
use node::logger;

pub fn main() -> anyhow::Result<()> {
    logger::init();

    let args = argh::from_env::<Args>();
    args.run()
}
