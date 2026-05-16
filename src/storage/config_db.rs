use crate::config::{
    AppConfig, BinanceConfig, BinanceNet, RuntimeConfig, StreamConfig, StreamType, SymbolConfig,
};
use anyhow::{Result, bail};
use sqlx::{PgPool, query};

pub async fn init_db() -> Result<PgPool> {
    let url = std::env::var("DATABASE_URL").expect("DB URL 없음");
    let pool = PgPool::connect(&url).await?;
    sqlx::raw_sql(include_str!("../../migrations/init.sql"))
        .execute(&pool)
        .await?;
    Ok(pool)
}

pub async fn load_config(pool: &PgPool) -> Result<AppConfig> {
    let binance_net = match std::env::var("BINANCE_NET").as_deref() {
        Ok("testnet") => BinanceNet::Testnet,
        _ => BinanceNet::Mainnet,
    };

    let symbols = load_symbols(pool).await?;
    let runtime_rows = load_runtime_rows(pool).await?;
    let streams = load_streams(pool).await?;

    Ok(AppConfig {
        binance: BinanceConfig {
            net: binance_net,
            symbols,
            streams,
        },
        runtime: RuntimeConfig::from_rows(&runtime_rows),
    })
}

async fn load_symbols(pool: &PgPool) -> Result<Vec<SymbolConfig>> {
    let rows = query!("SELECT symbol, enabled FROM symbols")
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|r| SymbolConfig {
            symbol: r.symbol,
            enabled: r.enabled,
        })
        .collect())
}

async fn load_runtime_rows(pool: &PgPool) -> Result<Vec<(String, String, String)>> {
    let rows = query!(r#"SELECT "type", key, value FROM runtime_config"#)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.r#type, r.key, r.value))
        .collect())
}

async fn load_streams(pool: &PgPool) -> Result<Vec<StreamConfig>> {
    let rows = query!("SELECT name, stream_type, suffix, enabled FROM streams")
        .fetch_all(pool)
        .await?;

    rows.into_iter()
        .map(|r| {
            let stream_type = match r.stream_type.as_str() {
                "MARKET" => StreamType::Market,
                "PUBLIC" => StreamType::Public,
                "PRIVATE" => StreamType::Private,
                other => bail!("알수없는 stream_type: {other}"),
            };
            Ok(StreamConfig {
                name: r.name,
                stream_type,
                suffix: r.suffix,
                enabled: r.enabled,
            })
        })
        .collect()
}
