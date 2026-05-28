-- 회로 차단기 임계치를 runtime_config(type='risk')에 시드
INSERT INTO runtime_config (type, key, value, memo)
VALUES ('risk', 'cb_consecutive_failure_threshold', '5', '연속 실패 trip 임계치(0=비활성)'),
       ('risk', 'cb_failure_window_sec', '60', '실패 카운트 슬라이딩 윈도우(초)'),
       ('risk', 'cb_open_duration_sec', '120', 'Open 상태 자동 해제까지 대기 시간(초)'),
       ('risk', 'cb_drawdown_pct', '5.0', '잔고 드로다운 trip 임계치(%, 0=비활성)'),
       ('risk', 'cb_drawdown_lookback_sec', '3600', '드로다운 추적 lookback(초)')
ON CONFLICT (type, key) DO NOTHING;
