use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::warn;

/// Binance Futures 1분 weight 한도(2400). 안전 마진 80%에서 throttle 발동.
const DEFAULT_WEIGHT_LIMIT: u32 = 2400;
const THROTTLE_RATIO: f64 = 0.80;

#[derive(Debug, Default)]
struct RateLimitState {
    used_weight_1m: u32,
    cooldown_until: Option<Instant>,
}

/// 응답 헤더(`x-mbx-used-weight-1m`, `Retry-After`)를 추적해 사전 throttle/cooldown을 적용한다.
pub struct RateLimitTracker {
    state: Mutex<RateLimitState>,
    weight_limit: u32,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(RateLimitState::default()),
            weight_limit: DEFAULT_WEIGHT_LIMIT,
        }
    }

    pub fn with_limit(weight_limit: u32) -> Self {
        Self {
            state: Mutex::new(RateLimitState::default()),
            weight_limit,
        }
    }

    pub fn record_used_weight(&self, used: Option<u32>) {
        if let Some(w) = used {
            self.state.lock().unwrap().used_weight_1m = w;
        }
    }

    /// 429 응답 처리. `Retry-After`(초)를 cooldown으로 등록.
    pub fn record_429(&self, retry_after: Option<Duration>) {
        let wait = retry_after.unwrap_or(Duration::from_secs(60));
        let mut st = self.state.lock().unwrap();
        st.cooldown_until = Some(Instant::now() + wait);
        warn!(?wait, "RateLimitTracker: 429 cooldown");
    }

    /// 호출 가능 여부 점검. cooldown 활성이면 만료까지 대기하고,
    /// weight가 한도의 80% 이상이면 짧게 sleep해 한도 임박을 완화한다.
    pub async fn await_capacity(&self) {
        let (used, cooldown) = {
            let st = self.state.lock().unwrap();
            (st.used_weight_1m, st.cooldown_until)
        };
        if let Some(until) = cooldown {
            let now = Instant::now();
            if until > now {
                sleep(until - now).await;
                self.state.lock().unwrap().cooldown_until = None;
            }
        }
        if used as f64 >= self.weight_limit as f64 * THROTTLE_RATIO {
            sleep(Duration::from_millis(500)).await;
        }
    }
}

impl Default for RateLimitTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// 응답 헤더에서 used-weight를 읽어내는 헬퍼.
pub fn parse_used_weight(headers: &reqwest::header::HeaderMap) -> Option<u32> {
    headers
        .get("x-mbx-used-weight-1m")
        .or_else(|| headers.get("X-MBX-USED-WEIGHT-1M"))
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
}

/// 응답 헤더에서 `Retry-After`(초)를 읽어 `Duration`으로 변환.
pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[tokio::test]
    async fn await_capacity_no_op_when_under_threshold() {
        let t = RateLimitTracker::with_limit(100);
        t.record_used_weight(Some(10));
        let start = Instant::now();
        t.await_capacity().await;
        assert!(start.elapsed() < Duration::from_millis(100));
    }

    #[tokio::test]
    async fn await_capacity_sleeps_when_over_threshold() {
        let t = RateLimitTracker::with_limit(100);
        t.record_used_weight(Some(90));
        let start = Instant::now();
        t.await_capacity().await;
        assert!(start.elapsed() >= Duration::from_millis(400));
    }

    #[test]
    fn parse_used_weight_reads_header() {
        let mut h = HeaderMap::new();
        h.insert("x-mbx-used-weight-1m", HeaderValue::from_static("1234"));
        assert_eq!(parse_used_weight(&h), Some(1234));
    }

    #[test]
    fn parse_retry_after_reads_header() {
        let mut h = HeaderMap::new();
        h.insert(reqwest::header::RETRY_AFTER, HeaderValue::from_static("30"));
        assert_eq!(parse_retry_after(&h), Some(Duration::from_secs(30)));
    }
}
