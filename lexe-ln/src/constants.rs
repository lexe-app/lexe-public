use std::time::Duration;

use lightning::routing::scoring::ProbabilisticScoringDecayParameters;

/// Research by Stillmark has shown that liquidity doesn't change much.
/// We probe once a minute (1440 probes a day), but the Lightning Network has
/// 47k public channels. Ensure our hard-won probe data doesn't get thrown away.
pub const LEXE_SCORER_PARAMS: ProbabilisticScoringDecayParameters =
    ProbabilisticScoringDecayParameters {
        // Decay once every 30 days. LDK default: 14 days.
        historical_no_updates_half_life: Duration::new(30 * 24 * 60 * 60, 0),
        // Decay once every two weeks. LDK default: 30 minutes.
        liquidity_offset_half_life: Duration::new(14 * 24 * 60 * 60, 0),
    };
