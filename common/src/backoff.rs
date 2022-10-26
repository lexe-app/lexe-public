use std::cmp::min;
use std::time::Duration;

use crate::const_assert;

const INITIAL_WAIT_MS: u64 = 250;
const MAXIMUM_WAIT_MS: u64 = 32_000;
const EXP_BASE: u64 = 2;

const_assert!(INITIAL_WAIT_MS != 0);

/// Get a iterator of [`Duration`]s which can be passed into e.g.
/// [`tokio::time::sleep`] to observe time-based exponential backoff.
///
/// ```
/// # use common::backoff;
/// # #[tokio::test(start_paused = true)]
/// # async fn backoff_example() {
/// let mut backoff_durations = backoff::get_backoff_iter();
/// for _ in 0..10 {
///     tokio::time::sleep(backoff_durations.next().unwrap()).await;
/// }
/// # }
/// ```
pub fn get_backoff_iter() -> impl Iterator<Item = Duration> {
    (0u32..).map(|index| {
        let factor = EXP_BASE.saturating_pow(index);
        let wait_ms = INITIAL_WAIT_MS.saturating_mul(factor);
        let bounded_wait_ms = min(wait_ms, MAXIMUM_WAIT_MS);
        Duration::from_millis(bounded_wait_ms)
    })
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
