use crate::config::binance_config::{
    BinanceConfig, BinanceNet, StreamConfig, StreamType, SymbolConfig,
};
use crate::config::{AppConfig, RuntimeConfig};
use crate::ports::config_repository::ConfigRepository;
use anyhow::{Result, bail};
use async_trait::async_trait;
use sqlx::{PgPool, query};

pub struct PgConfigRepository {
    pool: PgPool,
}

#[async_trait]
impl ConfigRepository for PgConfigRepository {
    async fn load(&self) -> Result<AppConfig> {
        Self::load_config(&self.pool).await
    }
}

impl PgConfigRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn load_config(pool: &PgPool) -> Result<AppConfig> {
        let binance_net = match std::env::var("BINANCE_NET").as_deref() {
            Ok("testnet") => BinanceNet::Testnet,
            _ => BinanceNet::Mainnet,
        };

        let symbols = Self::load_symbols(pool).await?;
        let runtime_rows = Self::load_runtime_rows(pool).await?;
        let streams = Self::load_streams(pool).await?;

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
}
