use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// 회로 차단기 적용 범위. 글로벌은 모든 심볼에 적용, Symbol(s)는 해당 심볼만 차단.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CircuitScope {
    Global,
    Symbol(String),
}

impl CircuitScope {
    pub fn label(&self) -> &str {
        match self {
            CircuitScope::Global => "GLOBAL",
            CircuitScope::Symbol(s) => s.as_str(),
        }
    }
}

/// 회로 상태 머신. trip 후 `until` 시점이 지나면 `HalfOpen`으로 전이되며,
/// 한 번의 성공으로 `Closed`로 복귀한다.
#[derive(Debug, Clone, Default)]
pub enum CircuitState {
    #[default]
    Closed,
    Open {
        until: DateTime<Utc>,
        reason: TripReason,
    },
    HalfOpen {
        reason: TripReason,
    },
}

/// trip의 원인. 로그·알림·에러 페이로드에 포함된다.
#[derive(Debug, Clone)]
pub enum TripReason {
    ConsecutiveFailures {
        count: u32,
        window_sec: u64,
    },
    Drawdown {
        high: Decimal,
        current: Decimal,
        pct: Decimal,
    },
}

impl TripReason {
    pub fn summary(&self) -> String {
        match self {
            TripReason::ConsecutiveFailures { count, window_sec } => {
                format!("{count}건 연속 실패 in {window_sec}s")
            }
            TripReason::Drawdown { high, current, pct } => {
                format!("drawdown {pct}% (high={high} -> current={current})")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_label_global_and_symbol() {
        assert_eq!(CircuitScope::Global.label(), "GLOBAL");
        assert_eq!(CircuitScope::Symbol("BTCUSDT".into()).label(), "BTCUSDT");
    }

    #[test]
    fn trip_reason_summary_includes_counts() {
        let r = TripReason::ConsecutiveFailures {
            count: 5,
            window_sec: 60,
        };
        assert!(r.summary().contains("5"));
        assert!(r.summary().contains("60"));
    }
}
