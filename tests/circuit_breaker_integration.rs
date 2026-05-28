use arc_swap::ArcSwap;
use std::sync::Arc;
use trading_bot::application::circuit_breaker_controller::CircuitBreakerController;
use trading_bot::domain::circuit_breaker::CircuitScope;
use trading_bot::ports::outbound::risk_config_source::RiskLimits;

#[tokio::test]
async fn gate_blocks_after_consecutive_failures() {
    let limits = RiskLimits {
        cb_consecutive_failure_threshold: 2,
        cb_failure_window_sec: 60,
        cb_open_duration_sec: 30,
        ..Default::default()
    };
    let cb = CircuitBreakerController::new(Arc::new(ArcSwap::from_pointee(limits)));

    cb.record_failure(CircuitScope::Global);
    cb.record_failure(CircuitScope::Global);

    let err = cb.gate("BTCUSDT").unwrap_err();
    assert_eq!(err.scope, "GLOBAL");
}
