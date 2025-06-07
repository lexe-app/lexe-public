use anyhow::Context;
use common::{
    cli::node::{MegaArgs, ProvisionArgs},
    constants,
    rng::Crng,
};
use lexe_tokio::{notify_once::NotifyOnce, task};
use tokio::sync::mpsc;

use crate::provision::ProvisionInstance;

pub async fn run(rng: &mut impl Crng, args: MegaArgs) -> anyhow::Result<()> {
    let mut static_tasks = Vec::with_capacity(5);

    let (eph_tasks_tx, eph_tasks_rx) =
        mpsc::channel(lexe_tokio::DEFAULT_CHANNEL_SIZE);

    // TODO(max): User node tasks should be spawned into this.
    let _ = eph_tasks_tx;

    // Shutdown channel for the entire mega instance.
    let mega_shutdown = NotifyOnce::new();

    // Init the provision service. Since it's a static service that should
    // live as long as the mega node itself, we can reuse the mega_shutdown.
    let provision_args = ProvisionArgs::from(&args);
    let provision =
        ProvisionInstance::init(rng, provision_args, mega_shutdown.clone())
            .await?;
    static_tasks.push(provision.spawn_into_task());

    task::try_join_tasks_and_shutdown(
        static_tasks,
        eph_tasks_rx,
        mega_shutdown,
        constants::USER_NODE_SHUTDOWN_TIMEOUT,
    )
    .await
    .context("Error awaiting tasks")?;

    Ok(())
}
