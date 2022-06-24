use node::cli::Args;

pub fn main() -> anyhow::Result<()> {
    // TODO(phlip9): init tracing

    let args = argh::from_env::<Args>();
    args.run()
}
