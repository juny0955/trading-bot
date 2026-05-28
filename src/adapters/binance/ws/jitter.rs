use std::time::Duration;

/// Decorrelated jitter (AWS Architecture Blog 알고리즘):
/// next = min(max_delay, rand_range(initial_delay, prev_delay * 3))
///
/// `prev_delay`가 `initial_delay`보다 작으면 하한을 `initial_delay`로 클램프해
/// 다음 시도가 너무 일찍 발생하지 않도록 한다.
///
/// `rand_range`는 `[lo, hi)` 범위에서 균등 표본을 반환하는 클로저다.
/// 결정적 테스트를 위해 외부 주입한다.
pub fn decorrelated_jitter<R: FnMut(u64, u64) -> u64>(
    prev_delay: Duration,
    initial_delay: Duration,
    max_delay: Duration,
    mut rand_range: R,
) -> Duration {
    let lo = initial_delay.as_millis() as u64;
    let upper_raw = (prev_delay.as_millis() as u64).saturating_mul(3);
    let upper = upper_raw.max(lo + 1);
    let max = max_delay.as_millis() as u64;
    let picked = rand_range(lo, upper).min(max);
    Duration::from_millis(picked)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_returns_value_within_bounds() {
        let d = decorrelated_jitter(
            Duration::from_millis(1000),
            Duration::from_millis(500),
            Duration::from_millis(30_000),
            |lo, hi| {
                assert_eq!(lo, 500);
                assert_eq!(hi, 3000);
                1500
            },
        );
        assert_eq!(d, Duration::from_millis(1500));
    }

    #[test]
    fn jitter_caps_at_max_delay() {
        let d = decorrelated_jitter(
            Duration::from_secs(60),
            Duration::from_secs(1),
            Duration::from_secs(30),
            |_, _| 60_000,
        );
        assert_eq!(d, Duration::from_secs(30));
    }

    #[test]
    fn jitter_uses_initial_when_prev_zero() {
        let d = decorrelated_jitter(
            Duration::ZERO,
            Duration::from_millis(500),
            Duration::from_secs(60),
            |lo, _| lo,
        );
        assert_eq!(d, Duration::from_millis(500));
    }
}
