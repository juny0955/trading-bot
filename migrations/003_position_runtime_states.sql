CREATE TABLE position_runtime_states (
    strategy_instance_id    BIGINT          NOT NULL REFERENCES strategy_instances(id),
    symbol                  TEXT            NOT NULL,
    tp_stage                SMALLINT        NOT NULL DEFAULT 0,
    sl_order_id             UUID,
    pending_reduce_order_id UUID,
    high_water_mark         NUMERIC(30, 10),
    entry_atr               NUMERIC(30, 10),
    desired_sl_price        NUMERIC(30, 10),
    updated_at              TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    PRIMARY KEY (strategy_instance_id, symbol)
);
