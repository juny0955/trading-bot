use crate::domain::circuit_breaker::{CircuitScope, CircuitState, TripReason};
use crate::ports::outbound::risk_config_source::RiskLimits;
use arc_swap::ArcSwap;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rust_decimal::Decimal;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

#[derive(Debug, thiserror::Error)]
#[error("회로 차단: scope={scope} reason={reason} retry_after_sec={retry_after_sec}")]
pub struct CircuitTripped {
    pub scope: String,
    pub reason: String,
    pub retry_after_sec: u64,
}

#[derive(Default)]
struct ScopeStats {
    failures: VecDeque<DateTime<Utc>>,
    state: CircuitState,
}

pub struct CircuitBreakerController {
    limits: Arc<ArcSwap<RiskLimits>>,
    inner: Mutex<Inner>,
}

struct Inner {
    global: ScopeStats,
    per_symbol: HashMap<String, ScopeStats>,
    equity_high: Option<Decimal>,
    equity_high_at: Option<DateTime<Utc>>,
}

impl CircuitBreakerController {
    pub fn new(limits: Arc<ArcSwap<RiskLimits>>) -> Self {
        Self {
            limits,
            inner: Mutex::new(Inner {
                global: ScopeStats::default(),
                per_symbol: HashMap::new(),
                equity_high: None,
                equity_high_at: None,
            }),
        }
    }

    pub fn gate(&self, symbol: &str) -> Result<(), CircuitTripped> {
        let now = Utc::now();
        let mut inner = self.inner.lock().unwrap();
        if let Some(t) = scope_trip(&mut inner.global, now, "GLOBAL") {
            return Err(t);
        }
        let stats = inner.per_symbol.entry(symbol.to_string()).or_default();
        if let Some(t) = scope_trip(stats, now, symbol) {
            return Err(t);
        }
        Ok(())
    }

    pub fn record_failure(&self, scope: CircuitScope) {
        let limits = self.limits.load();
        let threshold = limits.cb_consecutive_failure_threshold;
        if threshold == 0 {
            return;
        }
        let window = ChronoDuration::seconds(limits.cb_failure_window_sec as i64);
        let now = Utc::now();
        let mut inner = self.inner.lock().unwrap();
        let stats = match &scope {
            CircuitScope::Global => &mut inner.global,
            CircuitScope::Symbol(s) => inner.per_symbol.entry(s.clone()).or_default(),
        };

        stats.failures.push_back(now);
        while let Some(front) = stats.failures.front().copied() {
            if now.signed_duration_since(front) > window {
                stats.failures.pop_front();
            } else {
                break;
            }
        }

        if stats.failures.len() as u32 >= threshold {
            let reason = TripReason::ConsecutiveFailures {
                count: stats.failures.len() as u32,
                window_sec: limits.cb_failure_window_sec,
            };
            let until = now + ChronoDuration::seconds(limits.cb_open_duration_sec as i64);
            stats.state = CircuitState::Open {
                until,
                reason: reason.clone(),
            };
            warn!(
                scope = %scope.label(),
                reason = %reason.summary(),
                "CB tripped"
            );
        }
    }

    pub fn record_success(&self, scope: CircuitScope) {
        let mut inner = self.inner.lock().unwrap();
        let stats = match &scope {
            CircuitScope::Global => &mut inner.global,
            CircuitScope::Symbol(s) => inner.per_symbol.entry(s.clone()).or_default(),
        };
        stats.failures.clear();
        if matches!(stats.state, CircuitState::HalfOpen { .. }) {
            info!(scope = %scope.label(), "CB closed (half-open success)");
            stats.state = CircuitState::Closed;
        }
    }

    pub fn record_balance(&self, equity: Decimal) {
        let limits = self.limits.load();
        let pct_threshold = limits.cb_drawdown_pct;
        if pct_threshold.is_zero() {
            return;
        }
        let lookback = ChronoDuration::seconds(limits.cb_drawdown_lookback_sec as i64);
        let now = Utc::now();
        let mut inner = self.inner.lock().unwrap();

        if let Some(at) = inner.equity_high_at
            && now.signed_duration_since(at) > lookback
        {
            inner.equity_high = None;
            inner.equity_high_at = None;
        }

        let high = inner.equity_high.unwrap_or(equity).max(equity);
        if Some(high) != inner.equity_high {
            inner.equity_high = Some(high);
            inner.equity_high_at = Some(now);
        }
        if high.is_zero() {
            return;
        }

        let drop_pct = (high - equity) / high * Decimal::from(100);
        if drop_pct >= pct_threshold {
            let reason = TripReason::Drawdown {
                high,
                current: equity,
                pct: drop_pct,
            };
            let until = now + ChronoDuration::seconds(limits.cb_open_duration_sec as i64);
            inner.global.state = CircuitState::Open {
                until,
                reason: reason.clone(),
            };
            warn!(reason = %reason.summary(), "CB tripped: drawdown");
        }
    }
}

fn scope_trip(stats: &mut ScopeStats, now: DateTime<Utc>, label: &str) -> Option<CircuitTripped> {
    match stats.state.clone() {
        CircuitState::Closed => None,
        CircuitState::HalfOpen { .. } => None,
        CircuitState::Open { until, reason } => {
            if now >= until {
                stats.state = CircuitState::HalfOpen {
                    reason: reason.clone(),
                };
                info!(scope = %label, "CB half-open");
                None
            } else {
                Some(CircuitTripped {
                    scope: label.to_string(),
                    reason: reason.summary(),
                    retry_after_sec: (until - now).num_seconds().max(0) as u64,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn limits_with(
        threshold: u32,
        window: u64,
        open: u64,
        dd: Decimal,
        lookback: u64,
    ) -> Arc<ArcSwap<RiskLimits>> {
        let limits = RiskLimits {
            cb_consecutive_failure_threshold: threshold,
            cb_failure_window_sec: window,
            cb_open_duration_sec: open,
            cb_drawdown_pct: dd,
            cb_drawdown_lookback_sec: lookback,
            ..Default::default()
        };
        Arc::new(ArcSwap::from_pointee(limits))
    }

    #[test]
    fn three_failures_trip_global() {
        let cb = CircuitBreakerController::new(limits_with(3, 60, 30, dec!(0), 3600));
        cb.record_failure(CircuitScope::Global);
        cb.record_failure(CircuitScope::Global);
        assert!(cb.gate("BTCUSDT").is_ok());
        cb.record_failure(CircuitScope::Global);
        let err = cb.gate("BTCUSDT").unwrap_err();
        assert_eq!(err.scope, "GLOBAL");
        assert!(err.retry_after_sec > 0);
    }

    #[test]
    fn symbol_trip_does_not_block_other_symbols() {
        let cb = CircuitBreakerController::new(limits_with(2, 60, 30, dec!(0), 3600));
        cb.record_failure(CircuitScope::Symbol("BTCUSDT".into()));
        cb.record_failure(CircuitScope::Symbol("BTCUSDT".into()));
        assert!(cb.gate("BTCUSDT").is_err());
        assert!(cb.gate("ETHUSDT").is_ok());
    }

    #[test]
    fn success_clears_failure_window() {
        let cb = CircuitBreakerController::new(limits_with(3, 60, 30, dec!(0), 3600));
        cb.record_failure(CircuitScope::Global);
        cb.record_failure(CircuitScope::Global);
        cb.record_success(CircuitScope::Global);
        cb.record_failure(CircuitScope::Global);
        cb.record_failure(CircuitScope::Global);
        assert!(cb.gate("BTCUSDT").is_ok());
    }

    #[test]
    fn drawdown_5pct_trips_global() {
        let cb = CircuitBreakerController::new(limits_with(0, 60, 30, dec!(5), 3600));
        cb.record_balance(dec!(1000));
        cb.record_balance(dec!(950));
        let err = cb.gate("BTCUSDT").unwrap_err();
        assert_eq!(err.scope, "GLOBAL");
        assert!(err.reason.contains("drawdown"));
    }
}
