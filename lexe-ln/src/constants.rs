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

/// Minimum CLTV difference between the current block height and received
/// inbound payments. Invoices generated for payment to us must set their
/// `min_final_cltv_expiry_delta` field to at least this value.
//
// TODO(phlip9): This is one of our key security parameters. It impacts how
// often we need to run our user nodes to prevent accidental force closes.
// I believe we want this longer?
pub const USER_MIN_FINAL_CLTV_EXPIRY_DELTA: u16 =
    lightning::ln::channelmanager::MIN_FINAL_CLTV_EXPIRY_DELTA;
// 24 blocks ≈ 4 hours
lexe_std::const_assert_usize_eq!(USER_MIN_FINAL_CLTV_EXPIRY_DELTA as usize, 24,);

/// Minimum CLTV difference between the current block height and received
/// inbound payments. Invoices generated for payment to us must set their
/// `min_final_cltv_expiry_delta` field to at least this value.
//
// LSP should always be running, so this value could be shorter.
pub const LSP_MIN_FINAL_CLTV_EXPIRY_DELTA: u16 =
    lightning::ln::channelmanager::MIN_FINAL_CLTV_EXPIRY_DELTA;
