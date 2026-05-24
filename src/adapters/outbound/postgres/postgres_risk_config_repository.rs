use crate::ports::order_repository::RepositoryError;
use crate::ports::risk_config_source::{RiskConfigSource, RiskLimits};
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::{PgPool, Row};
use std::str::FromStr;

/// `runtime_config` 테이블에서 위험 한도를 로드하는 PostgreSQL 리포지토리.
pub struct PostgresRiskConfigRepository {
    pool: PgPool,
}

impl PostgresRiskConfigRepository {
    /// 새 `PostgresRiskConfigRepository`를 생성한다.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RiskConfigSource for PostgresRiskConfigRepository {
    async fn load(&self) -> Result<RiskLimits, RepositoryError> {
        let rows = sqlx::query(r#"SELECT key, value FROM runtime_config WHERE type = 'risk'"#)
            .fetch_all(&self.pool)
            .await?;

        let mut limits = RiskLimits::default();
        for row in rows {
            let key = get_str(&row, "key")?;
            let value = get_str(&row, "value")?;
            apply(&mut limits, &key, &value);
        }
        Ok(limits)
    }
}

fn get_str(row: &sqlx::postgres::PgRow, col: &str) -> Result<String, RepositoryError> {
    row.try_get(col)
        .map_err(|e| RepositoryError::Storage(e.to_string()))
}

fn apply(limits: &mut RiskLimits, key: &str, value: &str) {
    match key {
        "max_leverage" => parse_into(value, &mut limits.max_leverage),
        "max_open_orders_per_symbol" => parse_into(value, &mut limits.max_open_orders_per_symbol),
        "max_position_notional" => parse_decimal(value, &mut limits.max_position_notional),
        "order_ttl_sec" => parse_into(value, &mut limits.order_ttl_sec),
        "min_available_balance" => parse_decimal(value, &mut limits.min_available_balance),
        "slippage_buffer_bps" => parse_into(value, &mut limits.slippage_buffer_bps),
        _ => {}
    }
}

fn parse_into<T: FromStr>(s: &str, target: &mut T) {
    if let Ok(v) = s.parse::<T>() {
        *target = v;
    }
}

fn parse_decimal(s: &str, target: &mut Decimal) {
    if let Ok(v) = Decimal::from_str(s) {
        *target = v;
    }
}
