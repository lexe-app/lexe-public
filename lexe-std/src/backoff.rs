use std::{cmp::min, time::Duration};

const INITIAL_WAIT_MS: u64 = 250;
const MAXIMUM_WAIT_MS: u64 = 32_000;
const EXP_BASE: u64 = 2;

crate::const_assert!(INITIAL_WAIT_MS != 0);

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
pub fn get_backoff_iter() -> impl Iterator<Item = Duration> {
    iter_with_initial_wait_ms(INITIAL_WAIT_MS)
}

/// Like [`get_backoff_iter`], but allows specifying the initial wait time in
/// milliseconds.
// Haven't seen a good use case for customizing the maximum wait time yet, so
// for now we don't expose `maximum_wait_ms` as a parameter.
pub fn iter_with_initial_wait_ms(
    initial_wait_ms: u64,
) -> impl Iterator<Item = Duration> {
    // The initial wait being greater than the maximum wait won't cause any
    // problems, but the programmer probably didn't intend this.
    debug_assert!(initial_wait_ms <= MAXIMUM_WAIT_MS);

    (0u32..).map(move |index| {
        let factor = EXP_BASE.saturating_pow(index);
        let wait_ms = initial_wait_ms.saturating_mul(factor);
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
