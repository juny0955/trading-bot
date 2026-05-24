use crate::adapters::outbound::postgres::pool::init_db;
use crate::adapters::outbound::postgres::postgres_config_repository::PostgresConfigRepository;
use crate::adapters::outbound::postgres::postgres_position_repository::PostgresPositionRepository;
use crate::adapters::outbound::postgres::postgres_strategy_repository::PostgresStrategyRepository;
use crate::config::AppConfig;
use crate::config::binance_config::BinanceConfig;
use crate::init::setup;
use crate::ports::config_repository::ConfigRepository;
use crate::ports::position_repository::PositionRepository;
use crate::ports::strategy_repository::StrategyRepository;
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppBootstrap {
    pub pool: PgPool,
    pub config: Arc<AppConfig>,
    pub binance_cfg: BinanceConfig,
    pub position_repo: Arc<dyn PositionRepository>,
    pub strategy_repo: Arc<dyn StrategyRepository>,
}

impl AppBootstrap {
    pub async fn init() -> Result<Self> {
        setup();
        let pool = init_db().await?;
        let config = Arc::new(PostgresConfigRepository::new(pool.clone()).load().await?);
        let binance_cfg = config.binance.clone();
        let position_repo: Arc<dyn PositionRepository> =
            Arc::new(PostgresPositionRepository::new(pool.clone()));
        let strategy_repo: Arc<dyn StrategyRepository> =
            Arc::new(PostgresStrategyRepository::new(pool.clone()));
        Ok(Self {
            pool,
            config,
            binance_cfg,
            position_repo,
            strategy_repo,
        })
    }
}
