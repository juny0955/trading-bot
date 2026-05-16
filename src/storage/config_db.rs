use crate::config::{
    AppConfig, BinanceConfig, BinanceNet, RuntimeConfig, StreamConfig, SymbolConfig,
};
use anyhow::Result;
use rusqlite::Connection;

pub fn init_db() -> Result<Connection> {
    let conn = Connection::open("./data/config.db")?;
    conn.execute_batch(include_str!("../../migrations/config_init.sql"))?;
    Ok(conn)
}

pub fn load_config(conn: &Connection) -> Result<AppConfig> {
    let binance_net = match std::env::var("BINANCE_NET").as_deref() {
        Ok("testnet") => BinanceNet::Testnet,
        _ => BinanceNet::Mainnet,
    };

    let symbols = load_symbols(conn)?;
    let runtime_rows = load_runtime_rows(conn)?;
    let streams = load_streams(conn)?;

    Ok(AppConfig {
        binance: BinanceConfig {
            net: binance_net,
            symbols,
            streams,
        },
        runtime: RuntimeConfig::from_rows(&runtime_rows),
    })
}

fn load_symbols(conn: &Connection) -> Result<Vec<SymbolConfig>> {
    let mut stmt = conn.prepare("SELECT symbol, enabled FROM v_config_symbols_current")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SymbolConfig {
                symbol: row.get(0)?,
                enabled: row.get::<_, i64>(1)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn load_runtime_rows(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare("SELECT type, key, value FROM v_config_runtime_current")?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn load_streams(conn: &Connection) -> Result<Vec<StreamConfig>> {
    let mut stmt =
        conn.prepare("SELECT name, stream_type, suffix, enabled FROM v_config_streams_current")?;
    let rows = stmt
        .query_map([], |row| {
            Ok(StreamConfig {
                name: row.get(0)?,
                stream_type: row.get(1)?,
                suffix: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
