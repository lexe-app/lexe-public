//! `Retries`, the retry strategy for API requests.

use std::time::Duration;

use lexe_common::constants::timeout;
use lexe_std::backoff::Backoff;

use crate::error::ErrorCode;

/// A strategy for retrying a failed API request.
/// Retrying stops as soon as any of the configured limits is hit.
///
/// ```rust
/// # use std::time::Duration;
/// # use lexe_api_core::types::retries::Retries;
/// let retries = Retries::from_count(3)
///     .with_timeout(Duration::from_secs(15))
///     .with_backoff(250, 8_000);
/// ```
//
// We keep fields private to ensure we can never construct an "empty" `Retries`,
// i.e. a strategy that retries forever, with no stopping conditions. All
// constructors add at least one stopping condition, and all `with_*` builder
// methods can only add further restrictions.
#[derive(Clone, Debug)]
pub struct Retries {
    /// If `Some`, make up to this many retries after the initial attempt.
    count: Option<usize>,
    /// If `Some`, keep retrying (with backoff) until this timeout elapses.
    /// In-flight requests are also bounded by the time remaining.
    timeout: Option<Duration>,
    /// Stop retrying immediately if an attempt fails with one of these codes.
    stop_codes: Vec<ErrorCode>,
    /// Wait between attempts per a [`Backoff`] constructed with these params.
    backoff_initial_wait_ms: u64,
    backoff_max_wait_ms: u64,
}

impl Default for Retries {
    fn default() -> Self {
        Self::from_count(0)
    }
}

impl Retries {
    /// The retry strategy recommended for important persists: they should be
    /// able to ride out a transient service outage.
    ///
    /// See [`timeout::TRANSIENT_ERROR_TOLERANCE`] for details.
    pub const IMPORTANT_PERSISTS: Self =
        Self::from_timeout(timeout::TRANSIENT_ERROR_TOLERANCE);

    /// Make up to `count` retries after the initial attempt.
    pub const fn from_count(count: usize) -> Self {
        Self {
            count: Some(count),
            timeout: None,
            stop_codes: Vec::new(),
            backoff_initial_wait_ms: Backoff::DEFAULT_INITIAL_WAIT_MS,
            backoff_max_wait_ms: Backoff::DEFAULT_MAX_WAIT_MS,
        }
    }

    /// Keep retrying (with backoff) until `timeout` elapses.
    pub const fn from_timeout(timeout: Duration) -> Self {
        Self {
            count: None,
            timeout: Some(timeout),
            stop_codes: Vec::new(),
            backoff_initial_wait_ms: Backoff::DEFAULT_INITIAL_WAIT_MS,
            backoff_max_wait_ms: Backoff::DEFAULT_MAX_WAIT_MS,
        }
    }

    /// Also make up to `count` retries after the initial attempt.
    pub const fn with_count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// Also stop retrying once `timeout` has elapsed.
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Stop retrying immediately if an attempt fails with one of these codes.
    pub fn with_stop_codes(mut self, stop_codes: Vec<ErrorCode>) -> Self {
        self.stop_codes = stop_codes;
        self
    }

    /// Wait according to a custom backoff schedule between attempts.
    pub const fn with_backoff(
        mut self,
        initial_wait_ms: u64,
        max_wait_ms: u64,
    ) -> Self {
        self.backoff_initial_wait_ms = initial_wait_ms;
        self.backoff_max_wait_ms = max_wait_ms;
        self
    }

    /// Get all parts of the retry strategy along with a fresh [`Backoff`]
    /// iterator, for consumption by `RestClient::send_with_retries_inner`.
    pub fn parts(
        &self,
    ) -> (Option<usize>, Option<Duration>, &[ErrorCode], Backoff) {
        let backoff = Backoff::new(
            self.backoff_initial_wait_ms,
            self.backoff_max_wait_ms,
        );
        (self.count, self.timeout, &self.stop_codes, backoff)
    }
}
