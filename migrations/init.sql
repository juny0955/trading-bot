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