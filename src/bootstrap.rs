use crate::adapters::outbound::postgres::pool::init_db;
use crate::adapters::outbound::postgres::postgres_config_repository::PostgresConfigRepository;
use crate::config::AppConfig;
use crate::config::binance_config::BinanceConfig;
use crate::init::setup;
use crate::ports::config_repository::ConfigRepository;
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppBootstrap {
    pub pool: PgPool,
    pub config: Arc<AppConfig>,
    pub binance_cfg: BinanceConfig,
}

impl AppBootstrap {
    pub async fn init() -> Result<Self> {
        setup();
        let pool = init_db().await?;
        let config = Arc::new(PostgresConfigRepository::new(pool.clone()).load().await?);
        let binance_cfg = config.binance.clone();
        Ok(Self {
            pool,
            config,
            binance_cfg,
        })
    }
}
