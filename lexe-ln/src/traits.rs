use std::sync::Mutex;

use async_trait::async_trait;
use common::ln::peer::ChannelPeer;
use lightning::chain::chainmonitor::Persist;
use lightning::util::ser::Writeable;

use crate::alias::{NetworkGraphType, ProbabilisticScorerType, SignerType};

/// Defines all the methods needed in shared Lexe LN logic.
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

    async fn persist_channel_peer(
        &self,
        _channel_peer: ChannelPeer,
    ) -> anyhow::Result<()>;
}
