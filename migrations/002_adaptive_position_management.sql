-- Migration 002: Adaptive Position Management + ML Data Pipeline

-- 1. fills 테이블 확장: lifecycle 연결 + 글로벌 dedup 강화
ALTER TABLE fills ADD COLUMN IF NOT EXISTS position_lifecycle_id UUID;

-- exchange_trade_id 글로벌 유니크 인덱스 (양수 trade ID만, -1(non-TRADE) 제외)
CREATE UNIQUE INDEX IF NOT EXISTS uidx_fills_exchange_trade_id
    ON fills (exchange_trade_id)
    WHERE exchange_trade_id IS NOT NULL AND exchange_trade_id > 0;

CREATE INDEX IF NOT EXISTS idx_fills_lifecycle
    ON fills (position_lifecycle_id)
    WHERE position_lifecycle_id IS NOT NULL;

-- 2. unresolved_fills: lifecycle 매핑 실패 체결 격리 (fills와 FK 없음 — fills가 아직 없는 상태이므로)
CREATE TABLE IF NOT EXISTS unresolved_fills
(
    id                UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    exchange_trade_id BIGINT,
    raw_payload       JSONB       NOT NULL,
    status            VARCHAR(20) NOT NULL DEFAULT 'PENDING', -- PENDING / RESOLVED / DEAD
    reason            VARCHAR(100),
    last_error        TEXT,
    retry_count       SMALLINT    NOT NULL DEFAULT 0,
    inserted_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_unresolved_fills_pending
    ON unresolved_fills (retry_count, inserted_at)
    WHERE status = 'PENDING';

-- 3. position_lifecycles: 포지션 생애주기 (진입~청산 하나의 거래 단위)
CREATE TABLE IF NOT EXISTS position_lifecycles
(
    id                   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    strategy_instance_id BIGINT      NOT NULL REFERENCES strategy_instances (id),
    symbol               VARCHAR(20) NOT NULL REFERENCES symbols (symbol),
    side                 VARCHAR(10) NOT NULL,
    opened_at            TIMESTAMPTZ NOT NULL,
    closed_at            TIMESTAMPTZ,
    entry_avg_price      NUMERIC,
    exit_avg_price       NUMERIC,
    total_qty            NUMERIC,
    realized_pnl         NUMERIC,
    fee_total            NUMERIC,
    net_pnl              NUMERIC,
    mfe_pct              NUMERIC, -- Max Favorable Excursion (진입 후 최고 수익 %)
    mae_pct              NUMERIC, -- Max Adverse Excursion (진입 후 최대 손실 %)
    entry_atr            NUMERIC,
    high_water_mark      NUMERIC, -- 트레일링 스탑용 (주기적 flush)
    low_water_mark       NUMERIC
);
CREATE INDEX IF NOT EXISTS idx_lifecycles_instance_symbol
    ON position_lifecycles (strategy_instance_id, symbol, opened_at DESC);
CREATE INDEX IF NOT EXISTS idx_lifecycles_open
    ON position_lifecycles (strategy_instance_id, symbol)
    WHERE closed_at IS NULL;

-- 4. trade_decisions: 포지션 관리 결정 + ML 학습 데이터
CREATE TABLE IF NOT EXISTS trade_decisions
(
    id                       UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    position_lifecycle_id    UUID        NOT NULL REFERENCES position_lifecycles (id),
    symbol                   VARCHAR(20) NOT NULL,
    decision_type            VARCHAR(30) NOT NULL, -- replace_sl / partial_close / early_exit / emergency_close
    -- 시장 특징 (ML features)
    atr                      NUMERIC,
    atr_pct                  NUMERIC,    -- atr / mid_price * 100
    adx                      NUMERIC,
    spread_pct               NUMERIC,
    fng_value                SMALLINT,
    ema_alignment            SMALLINT,   -- 1(정배열) / -1(역배열) / 0(중립)
    ob_imbalance             NUMERIC,
    -- 포지션 상태 (ML features)
    unrealized_pnl_pct       NUMERIC,
    time_in_trade_secs       INTEGER,
    dist_to_sl_atr           NUMERIC,
    dist_to_tp_atr           NUMERIC,
    tp_stage                 SMALLINT,
    -- 결정 내용 (command-specific metadata)
    action_taken             JSONB       NOT NULL,
    -- exchange 실행 결과 (명시 컬럼)
    exchange_status          VARCHAR(20) NOT NULL DEFAULT 'PENDING', -- PENDING / ACKED / REJECTED / TIMEOUT
    ack_latency_ms           INTEGER,
    exchange_error           TEXT,
    -- 결과 (사후 백필 → ML label)
    outcome_pnl_pct          NUMERIC,
    mfe_from_here_pct        NUMERIC,
    mae_from_here_pct        NUMERIC,
    exit_reason              VARCHAR(50), -- trailing_stop / partial_tp / hold_cut / sl / tp
    -- RL Reward 설계용 (QuestDB mark_price 사후 백필)
    pnl_after_5m             NUMERIC,
    pnl_after_10m            NUMERIC,
    pnl_after_30m            NUMERIC,
    max_drawdown_until_close NUMERIC,
    decided_at               TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_trade_decisions_lifecycle
    ON trade_decisions (position_lifecycle_id);
CREATE INDEX IF NOT EXISTS idx_trade_decisions_backfill
    ON trade_decisions (decided_at)
    WHERE pnl_after_5m IS NULL;
CREATE INDEX IF NOT EXISTS idx_trade_decisions_pending_ack
    ON trade_decisions (decided_at)
    WHERE exchange_status = 'PENDING';
