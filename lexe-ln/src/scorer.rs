use std::time::Duration;

use lightning::{
    io,
    routing::{
        router::{CandidateRouteHop, Path},
        scoring::{
            ChannelUsage, ProbabilisticScoringFeeParameters, ScoreLookUp,
            ScoreUpdate,
        },
    },
    util::ser::{Writeable, Writer},
};

use crate::alias::ProbabilisticScorerType;

/// Lexe's payment scorer for both the LSP and user nodes. Wraps the shared
/// [`ProbabilisticScorer`].
///
/// [`ProbabilisticScorer`]: lightning::routing::scoring::ProbabilisticScorer
pub struct LexeScorer {
    inner: ProbabilisticScorerType,
}

impl LexeScorer {
    pub fn new(inner: ProbabilisticScorerType) -> Self {
        Self { inner }
    }
}

impl ScoreLookUp for LexeScorer {
    type ScoreParams = ProbabilisticScoringFeeParameters;

    fn channel_penalty_msat(
        &self,
        candidate: &CandidateRouteHop,
        usage: ChannelUsage,
        params: &Self::ScoreParams,
    ) -> u64 {
        self.inner.channel_penalty_msat(candidate, usage, params)
    }
}

impl ScoreUpdate for LexeScorer {
    fn payment_path_failed(
        &mut self,
        path: &Path,
        short_channel_id: u64,
        duration_since_epoch: Duration,
    ) {
        self.inner.payment_path_failed(
            path,
            short_channel_id,
            duration_since_epoch,
        );
    }

    fn payment_path_successful(
        &mut self,
        path: &Path,
        duration_since_epoch: Duration,
    ) {
        self.inner
            .payment_path_successful(path, duration_since_epoch);
    }

    fn probe_failed(
        &mut self,
        path: &Path,
        short_channel_id: u64,
        duration_since_epoch: Duration,
    ) {
        self.inner
            .probe_failed(path, short_channel_id, duration_since_epoch);
    }

    fn probe_successful(
        &mut self,
        path: &Path,
        duration_since_epoch: Duration,
    ) {
        self.inner.probe_successful(path, duration_since_epoch);
    }

    fn time_passed(&mut self, duration_since_epoch: Duration) {
        self.inner.time_passed(duration_since_epoch);
    }
}

impl Writeable for LexeScorer {
    fn write<W: Writer>(&self, writer: &mut W) -> Result<(), io::Error> {
        self.inner.write(writer)
    }
}
