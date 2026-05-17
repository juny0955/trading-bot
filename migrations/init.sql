-- 1. symbols
CREATE TABLE IF NOT EXISTS symbols
(
    symbol     VARCHAR(20) PRIMARY KEY,
    enabled    BOOLEAN     NOT NULL DEFAULT TRUE,
    memo       VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 2. streams
CREATE TABLE IF NOT EXISTS streams
(
    name        VARCHAR(50) PRIMARY KEY,
    stream_type VARCHAR(30)  NOT NULL, -- ex) 'ticker', 'depth', 'kline'
    suffix      VARCHAR(100) NOT NULL,
    enabled     BOOLEAN      NOT NULL DEFAULT TRUE,
    memo        VARCHAR(255),
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- 3. strategy
CREATE TABLE IF NOT EXISTS strategy
(
    id    BIGSERIAL PRIMARY KEY,
    key   VARCHAR(100)     NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    memo  VARCHAR(255),
    ts    TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);

-- 4. runtime_config
CREATE TABLE IF NOT EXISTS runtime_config
(
    type       VARCHAR(50)  NOT NULL,
    key        VARCHAR(100) NOT NULL,
    value      TEXT         NOT NULL,
    memo       VARCHAR(255),
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    PRIMARY KEY (type, key)
);

-- 5. v_strategy_current (전략 조회용 최신 스냅샷 뷰)
CREATE OR REPLACE VIEW v_strategy_current AS
SELECT key, value, memo, ts
FROM (SELECT key,
             value,
             memo,
             ts,
             ROW_NUMBER() OVER (PARTITION BY key ORDER BY ts DESC) AS rn
      FROM strategy) AS t
WHERE rn = 1;

-- 6. 인덱스
CREATE INDEX IF NOT EXISTS idx_strategy_fetch ON strategy (key, ts DESC);

-- 7. orders
CREATE TABLE IF NOT EXISTS orders
(
    id                UUID         PRIMARY KEY,
    client_order_id   VARCHAR(36)  NOT NULL UNIQUE,
    exchange_order_id BIGINT,
    symbol            VARCHAR(20)  NOT NULL,
    order_type        VARCHAR(20)  NOT NULL,
    side              VARCHAR(10)  NOT NULL,
    status            VARCHAR(20)  NOT NULL,
    qty               NUMERIC      NOT NULL,
    price             NUMERIC,
    stop_price        NUMERIC,
    filled_qty        NUMERIC      NOT NULL DEFAULT 0,
    avg_fill_price    NUMERIC,
    time_in_force     VARCHAR(10)  NOT NULL DEFAULT 'gtc',
    reduce_only       BOOLEAN      NOT NULL DEFAULT FALSE,
    post_only         BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_orders_symbol_status ON orders (symbol, status);
CREATE INDEX IF NOT EXISTS idx_orders_exchange_id ON orders (exchange_order_id);

-- 8. fills
CREATE TABLE IF NOT EXISTS fills
(
    id         BIGSERIAL    PRIMARY KEY,
    order_id   UUID         NOT NULL REFERENCES orders (id),
    symbol     VARCHAR(20)  NOT NULL,
    side       VARCHAR(10)  NOT NULL,
    qty        NUMERIC      NOT NULL,
    price      NUMERIC      NOT NULL,
    fee        NUMERIC      NOT NULL,
    fee_asset  VARCHAR(10)  NOT NULL,
    filled_at  TIMESTAMPTZ  NOT NULL,
    created_at TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_fills_order_id ON fills (order_id);
CREATE INDEX IF NOT EXISTS idx_fills_symbol_time ON fills (symbol, filled_at DESC);