-- symbols
CREATE TABLE IF NOT EXISTS symbols
(
    symbol      VARCHAR(20) PRIMARY KEY,
    enabled     BOOLEAN     NOT NULL DEFAULT TRUE,
    leverage    SMALLINT    NOT NULL DEFAULT 10,
    margin_type VARCHAR(10) NOT NULL DEFAULT 'CROSSED',
    memo        VARCHAR(255),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- streams
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

-- strategy_instance
CREATE TABLE IF NOT EXISTS strategy_instances
(
    id            BIGSERIAL PRIMARY KEY,
    strategy_type VARCHAR(50) NOT NULL,
    name          VARCHAR(50) NOT NULL UNIQUE,
    enabled       BOOLEAN     NOT NULL DEFAULT TRUE,
    params        JSONB       NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- strategy_instance_symbols
CREATE TABLE IF NOT EXISTS strategy_instance_symbols
(
    strategy_instance_id BIGINT      NOT NULL REFERENCES strategy_instances (id),
    symbol               VARCHAR(20) NOT NULL REFERENCES symbols (symbol),
    enabled              BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (strategy_instance_id, symbol)
);

-- runtime_config
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

-- orders
CREATE TABLE IF NOT EXISTS orders
(
    id                   UUID PRIMARY KEY,
    client_order_id      VARCHAR(36) NOT NULL UNIQUE,
    exchange_order_id    BIGINT,
    strategy_instance_id BIGINT      NOT NULL REFERENCES strategy_instances (id),
    symbol               VARCHAR(20) NOT NULL REFERENCES symbols (symbol),
    order_type           VARCHAR(20) NOT NULL,
    side                 VARCHAR(10) NOT NULL,
    status               VARCHAR(20) NOT NULL,
    qty                  NUMERIC     NOT NULL,
    price                NUMERIC,
    stop_price           NUMERIC,
    filled_qty           NUMERIC     NOT NULL DEFAULT 0,
    avg_fill_price       NUMERIC,
    time_in_force        VARCHAR(10) NOT NULL DEFAULT 'gtc',
    reduce_only          BOOLEAN     NOT NULL DEFAULT FALSE,
    post_only            BOOLEAN     NOT NULL DEFAULT FALSE,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_orders_strategy_status ON orders (strategy_instance_id, status);
CREATE INDEX IF NOT EXISTS idx_orders_symbol_status ON orders (symbol, status);
CREATE INDEX IF NOT EXISTS idx_orders_exchange_id ON orders (exchange_order_id);

-- fills
CREATE TABLE IF NOT EXISTS fills
(
    id                   BIGSERIAL PRIMARY KEY,
    order_id             UUID        NOT NULL REFERENCES orders (id),
    strategy_instance_id BIGINT      NOT NULL REFERENCES strategy_instances (id),
    symbol               VARCHAR(20) NOT NULL REFERENCES symbols (symbol),
    side                 VARCHAR(10) NOT NULL,
    qty                  NUMERIC     NOT NULL,
    price                NUMERIC     NOT NULL,
    fee                  NUMERIC     NOT NULL,
    fee_asset            VARCHAR(10) NOT NULL,
    filled_at            TIMESTAMPTZ NOT NULL,
    exchange_trade_id    BIGINT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_fills_strategy_time ON fills (strategy_instance_id, filled_at DESC);
CREATE INDEX IF NOT EXISTS idx_fills_symbol_time ON fills (symbol, filled_at DESC);
CREATE INDEX IF NOT EXISTS idx_fills_order_id ON fills (order_id);
-- 재연결/재처리 시 동일 exchange trade를 중복 INSERT 방지
CREATE UNIQUE INDEX IF NOT EXISTS uidx_fills_dedup ON fills (order_id, exchange_trade_id) WHERE exchange_trade_id IS NOT NULL;

-- positions
CREATE TABLE IF NOT EXISTS positions
(
    strategy_instance_id BIGINT      NOT NULL REFERENCES strategy_instances (id),
    symbol               VARCHAR(20) NOT NULL REFERENCES symbols (symbol),
    side                 VARCHAR(10) NOT NULL,
    qty                  NUMERIC     NOT NULL DEFAULT 0,
    avg_entry_price      NUMERIC     NOT NULL DEFAULT 0,
    realized_pnl         NUMERIC     NOT NULL DEFAULT 0,
    last_fill_id         BIGINT REFERENCES fills (id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (strategy_instance_id, symbol)
);

-- orphan_orders: 거래소에는 존재하지만 로컬 DB에 매핑되지 않은 주문 추적
CREATE TABLE IF NOT EXISTS orphan_orders
(
    exchange_order_id BIGINT PRIMARY KEY,
    client_order_id   VARCHAR(36),
    symbol            VARCHAR(20),
    detected_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    raw               JSONB       NOT NULL
);
