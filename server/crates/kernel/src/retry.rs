//! Retry delays shared by the outbox dispatcher and the job runner.

use std::time::Duration;

/// Exponential backoff: `base * 2^(attempt - 1)`, capped at `max`.
///
/// `attempt` is the number of failures so far (1 after the first failure). `attempt == 0`
/// yields no delay.
///
/// ```
/// use std::time::Duration;
/// use kernel::retry::backoff;
///
/// let base = Duration::from_secs(1);
/// let max = Duration::from_secs(60);
/// assert_eq!(backoff(1, base, max), Duration::from_secs(1));
/// assert_eq!(backoff(3, base, max), Duration::from_secs(4));
/// assert_eq!(backoff(20, base, max), max);
/// ```
pub fn backoff(attempt: u32, base: Duration, max: Duration) -> Duration {
    if attempt == 0 {
        return Duration::ZERO;
    }
    let factor = 2u32.checked_pow(attempt - 1).unwrap_or(u32::MAX);
    base.checked_mul(factor).unwrap_or(max).min(max)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn doubles_until_the_cap() {
        let base = Duration::from_millis(500);
        let max = Duration::from_secs(10);
        let delays: Vec<_> = (0..8).map(|a| backoff(a, base, max).as_millis()).collect();
        assert_eq!(delays, [0, 500, 1000, 2000, 4000, 8000, 10_000, 10_000]);
    }

    #[test]
    fn never_overflows() {
        let max = Duration::from_secs(3600);
        assert_eq!(
            backoff(u32::MAX, Duration::from_secs(u64::MAX / 2), max),
            max
        );
    }

    proptest! {
        #[test]
        fn is_capped_and_non_decreasing(
            attempt in 0u32..200,
            base_ms in 0u64..10_000,
            max_ms in 0u64..10_000_000,
        ) {
            let base = Duration::from_millis(base_ms);
            let max = Duration::from_millis(max_ms);
            let delay = backoff(attempt, base, max);
            prop_assert!(delay <= max);
            prop_assert!(backoff(attempt + 1, base, max) >= delay);
            if attempt >= 1 {
                prop_assert_eq!(backoff(1, base, max), base.min(max));
            }
        }
    }
}
