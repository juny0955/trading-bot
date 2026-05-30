-- 비용 기반 진입 가드 설정을 runtime_config(type='risk')에 시드
INSERT INTO runtime_config (type, key, value, memo)
VALUES ('risk', 'fee_bps', '4', '왕복 계산에 사용할 단방향 수수료 bps'),
       ('risk', 'estimated_slippage_bps', '2', '진입/청산 각 구간 예상 슬리피지 bps'),
       ('risk', 'max_spread_bps', '10', '신규 진입 허용 최대 스프레드 bps'),
       ('risk', 'min_net_reward_risk', '1.2', '비용 차감 후 최소 손익비'),
       ('risk', 'min_net_reward_bps', '10', '비용 차감 후 최소 기대 수익 bps'),
       ('risk', 'require_bracket_for_entry', 'true', '신규 진입 시 SL/TP 동시 존재 요구')
ON CONFLICT (type, key) DO NOTHING;
