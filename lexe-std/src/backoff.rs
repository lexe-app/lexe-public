use std::{cmp::min, time::Duration};

const INITIAL_WAIT_MS: u64 = 250;
const MAX_WAIT_MS: u64 = 32_000;
const EXP_BASE: u64 = 2;

/// Get a iterator of [`Duration`]s which can be passed into e.g.
/// `tokio::time::sleep` to observe time-based exponential backoff.
///
/// ```ignore
/// # use common::backoff;
/// # #[tokio::test(start_paused = true)]
/// # async fn backoff_example() {
/// let mut backoff_durations = backoff::get_backoff_iter();
/// for _ in 0..10 {
///     tokio::time::sleep(backoff_durations.next().unwrap()).await;
/// }
/// # }
/// ```
pub fn get_backoff_iter() -> Backoff {
    Backoff::default()
}

/// Like [`get_backoff_iter`], but allows specifying the initial wait time in
/// milliseconds.
pub fn iter_with_initial_wait_ms(initial_wait_ms: u64) -> Backoff {
    // The initial wait being greater than the maximum wait won't cause any
    // problems, but the programmer probably didn't intend this.
    debug_assert!(initial_wait_ms <= MAX_WAIT_MS);

    Backoff {
        initial_wait_ms,
        ..Backoff::default()
    }
}

/// Exponential backoff iterator yielding [`Duration`]s.
///
/// ```text
/// delay_i := min(2^i * initial_wait_ms, max_wait_ms)
/// ```
pub struct Backoff {
    pub initial_wait_ms: u64,
    pub max_wait_ms: u64,
    pub attempt: u32,
}

impl Backoff {
    /// Reset the backoff to the initial wait delay.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Get the next delay duration according to the exponential backoff.
    pub fn next_delay(&mut self) -> Duration {
        let factor = EXP_BASE.saturating_pow(self.attempt);
        let wait_ms = self.initial_wait_ms.saturating_mul(factor);
        let bounded_wait_ms = min(wait_ms, self.max_wait_ms);
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(bounded_wait_ms)
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial_wait_ms: INITIAL_WAIT_MS,
            max_wait_ms: MAX_WAIT_MS,
            attempt: 0,
        }
    }
}

impl Iterator for Backoff {
    type Item = Duration;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        Some(self.next_delay())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn no_integer_overflow() {
        let mut backoff_durations = get_backoff_iter();
        for _ in 0..200 {
            backoff_durations.next();
        }
    }
}
