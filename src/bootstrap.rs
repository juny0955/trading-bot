use crate::adapters::outbound::cache::book_ticker_cache::BookTickerCache;
use crate::adapters::outbound::cache::symbol_filters_cache::SymbolFiltersCache;
use crate::adapters::outbound::postgres::pool::init_db;
use crate::adapters::outbound::postgres::postgres_config_repository::PostgresConfigRepository;
use crate::adapters::outbound::postgres::postgres_position_repository::PostgresPositionRepository;
use crate::adapters::outbound::postgres::postgres_risk_config_repository::PostgresRiskConfigRepository;
use crate::adapters::outbound::postgres::postgres_runtime_state_repository::PostgresRuntimeStateRepository;
use crate::adapters::outbound::postgres::postgres_strategy_repository::PostgresStrategyRepository;
use crate::application::account_state::AccountState;
use crate::config::AppConfig;
use crate::config::binance_config::BinanceConfig;
use crate::domain::account::AccountSnapshot;
use crate::init::setup;
use crate::ports::config_repository::ConfigRepository;
use crate::ports::outbound::risk_config_source::{RiskConfigSource, RiskLimits};
use crate::ports::outbound::runtime_state_repository::RuntimeStateRepository;
use crate::ports::position_repository::PositionRepository;
use crate::ports::strategy_repository::StrategyRepository;
use anyhow::Result;
use arc_swap::ArcSwap;
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use std::sync::Arc;

pub struct AppBootstrap {
    pub pool: PgPool,
    pub config: Arc<AppConfig>,
    pub binance_cfg: BinanceConfig,
    pub position_repo: Arc<dyn PositionRepository>,
    pub strategy_repo: Arc<dyn StrategyRepository>,
    pub filters_cache: Arc<SymbolFiltersCache>,
    pub account_state: Arc<AccountState>,
    pub risk_limits: Arc<ArcSwap<RiskLimits>>,
    pub book_ticker_cache: Arc<BookTickerCache>,
    pub runtime_state_repo: Arc<dyn RuntimeStateRepository>,
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

        // Load risk limits from DB; fall back to defaults if unavailable
        let limits = PostgresRiskConfigRepository::new(pool.clone())
            .load()
            .await
            .unwrap_or_default();
        let risk_limits = Arc::new(ArcSwap::from_pointee(limits));

        // Empty caches — filled in trader.rs after BinanceOrderApi is created
        let filters_cache = Arc::new(SymbolFiltersCache::new());
        let book_ticker_cache = Arc::new(BookTickerCache::new());

        // Placeholder account state — replaced by account_bootstrap in trader.rs
        let account_state = Arc::new(AccountState::new(AccountSnapshot {
            available_balance: Decimal::ZERO,
            total_wallet_balance: Decimal::ZERO,
            total_unrealized_pnl: Decimal::ZERO,
            fetched_at: Utc::now(),
        }));

        let runtime_state_repo: Arc<dyn RuntimeStateRepository> =
            Arc::new(PostgresRuntimeStateRepository::new(pool.clone()));

        Ok(Self {
            pool,
            config,
            binance_cfg,
            position_repo,
            strategy_repo,
            filters_cache,
            account_state,
            risk_limits,
            book_ticker_cache,
            runtime_state_repo,
        })
    }
}
