-- QuestDB table initialization with partitioning and TTL.
-- Usage: paste each statement into QuestDB Web Console (http://localhost:9000)
-- or run via REST API:
--   curl -G "http://localhost:9000/exec" --data-urlencode "query=<statement>"
--
-- Run BEFORE starting the collector so tables are created with proper partitioning.
-- ILP auto-created tables have no partitioning and will grow unbounded.

-- raw trade (TTL 3일 — feature 재계산 보험)
CREATE TABLE IF NOT EXISTS raw_trade (
    symbol                SYMBOL,
    price                 DOUBLE,
    quantity              DOUBLE,
    buyer_is_market_maker BOOLEAN,
    timestamp             TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 3d WAL;

-- 1분 trade features: buy/sell split + vwap + delta (OHLCV는 kline 참조)
CREATE TABLE IF NOT EXISTS trade_features_1m (
    symbol       SYMBOL,
    buy_volume   DOUBLE,
    sell_volume  DOUBLE,
    delta_volume DOUBLE,
    trade_count  LONG,
    vwap         DOUBLE,
    timestamp    TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 365d WAL;

-- 1분 orderbook features: 3레벨 스냅샷 + spread/imbalance 통계
CREATE TABLE IF NOT EXISTS orderbook_features_1m (
    symbol        SYMBOL,
    bid1_price    DOUBLE, bid1_qty DOUBLE,
    bid2_price    DOUBLE, bid2_qty DOUBLE,
    bid3_price    DOUBLE, bid3_qty DOUBLE,
    ask1_price    DOUBLE, ask1_qty DOUBLE,
    ask2_price    DOUBLE, ask2_qty DOUBLE,
    ask3_price    DOUBLE, ask3_qty DOUBLE,
    spread_avg    DOUBLE,
    spread_max    DOUBLE,
    imbalance_avg DOUBLE,
    imbalance_std DOUBLE,
    timestamp     TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 365d WAL;

-- 1분 mark price (funding_rate/basis 분석용, 180일 보관)
CREATE TABLE IF NOT EXISTS mark_price_1m (
    symbol            SYMBOL,
    mark_price        DOUBLE,
    index_price       DOUBLE,
    funding_rate      DOUBLE,
    next_funding_time LONG,
    timestamp         TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 180d WAL;

-- kline (바이낸스 원본, 장기 보관)
CREATE TABLE IF NOT EXISTS kline (
    symbol       SYMBOL,
    open_time    LONG,
    open         DOUBLE,
    high         DOUBLE,
    low          DOUBLE,
    close        DOUBLE,
    volume       DOUBLE,
    quote_volume DOUBLE,
    num_trades   LONG,
    is_closed    BOOLEAN,
    timestamp    TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 730d WAL;

-- fear_greed_index (장기 보관)
CREATE TABLE IF NOT EXISTS fear_greed_index (
    status    SYMBOL,
    value     VARCHAR,
    timestamp TIMESTAMP
) TIMESTAMP(timestamp) PARTITION BY DAY TTL 730d WAL;

-- ── 기존 테이블 정리 (볼륨 확보) ──────────────────────────────────────
-- 주의: 아래 명령은 데이터를 삭제합니다. 확인 후 실행하세요.
--
-- DROP TABLE IF EXISTS depth;
-- DROP TABLE IF EXISTS book_ticker;
-- DROP TABLE IF EXISTS mark_price;
-- DROP TABLE IF EXISTS trades;
