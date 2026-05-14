CREATE TABLE IF NOT EXISTS config_symbols (
    symbol  TEXT NOT NULL,
    enabled INTEGER NOT NULL,
    memo    TEXT,
    ts      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS config_streams (
    name        TEXT NOT NULL,
    suffix      TEXT NOT NULL,
    enabled     INTEGER NOT NULL,
    memo        TEXT,
    ts          TEXT NOT NULL DEFAULT (datetime('now'))
);

 CREATE TABLE IF NOT EXISTS config_strategy (
     key   TEXT NOT NULL,
     value REAL NOT NULL,
     memo  TEXT,
     ts    TEXT NOT NULL DEFAULT (datetime('now'))
 );

CREATE TABLE IF NOT EXISTS config_runtime (
    type        TEXT NOT NULL,
    key         TEXT NOT NULL,
    value       TEXT NOT NULL,
    memo        TEXT,
    ts          TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE VIEW IF NOT EXISTS v_config_symbols_current AS
SELECT symbol, enabled, memo, ts
FROM (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY symbol ORDER BY ts DESC) AS rn
    FROM config_symbols
) WHERE rn = 1;

CREATE VIEW IF NOT EXISTS v_config_strategy_current AS
SELECT key, value, memo, ts
FROM (
    SELECT *, ROW_NUMBER() OVER (PARTITION BY key ORDER BY ts DESC) AS rn
    FROM config_strategy
) WHERE rn = 1;

 CREATE VIEW IF NOT EXISTS v_config_runtime_current AS
 SELECT type, key, value, memo, ts
 FROM (
     SELECT *, ROW_NUMBER() OVER (PARTITION BY type, key ORDER BY ts DESC) AS rn
     FROM config_runtime
 ) WHERE rn = 1;

 CREATE VIEW IF NOT EXISTS v_config_streams_current AS
 SELECT name, suffix, enabled, memo, ts
 FROM (
     SELECT *, ROW_NUMBER() OVER (PARTITION BY name ORDER BY ts DESC) AS rn
     FROM config_streams
 ) WHERE rn = 1;
