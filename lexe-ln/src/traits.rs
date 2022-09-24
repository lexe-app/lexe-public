use std::sync::Mutex;

use async_trait::async_trait;
use lightning::chain::chainmonitor::Persist;
use lightning::util::ser::Writeable;

use crate::alias::{NetworkGraphType, ProbabilisticScorerType, SignerType};

/// An async version of [`lightning::util::persist::Persister`],
/// used by the background processor.
#[async_trait]
pub trait LexePersister: Persist<SignerType> {
    async fn persist_manager<W: Writeable + Send + Sync>(
        &self,
        channel_manager: &W,
    ) -> anyhow::Result<()>;

    async fn persist_graph(
        &self,
        network_graph: &NetworkGraphType,
    ) -> anyhow::Result<()>;

    async fn persist_scorer(
        &self,
        scorer_mutex: &Mutex<ProbabilisticScorerType>,
    ) -> anyhow::Result<()>;
}
