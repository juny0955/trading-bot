use crate::domain::lifecycle::{
    LifecycleSide, PositionLifecycle, UnresolvedFill, UnresolvedStatus,
};
use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::lifecycle_repository::LifecycleRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresLifecycleRepository {
    pool: PgPool,
}

impl PostgresLifecycleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── DB 모델 ──────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DbLifecycle {
    id: Uuid,
    strategy_instance_id: i64,
    symbol: String,
    side: String,
    opened_at: DateTime<Utc>,
    closed_at: Option<DateTime<Utc>>,
    entry_avg_price: Option<Decimal>,
    exit_avg_price: Option<Decimal>,
    total_qty: Option<Decimal>,
    realized_pnl: Option<Decimal>,
    fee_total: Option<Decimal>,
    net_pnl: Option<Decimal>,
    mfe_pct: Option<Decimal>,
    mae_pct: Option<Decimal>,
    entry_atr: Option<Decimal>,
    high_water_mark: Option<Decimal>,
    low_water_mark: Option<Decimal>,
}

impl TryFrom<DbLifecycle> for PositionLifecycle {
    type Error = RepositoryError;
    fn try_from(db: DbLifecycle) -> Result<Self, Self::Error> {
        let side = match db.side.as_str() {
            "long" => LifecycleSide::Long,
            "short" => LifecycleSide::Short,
            s => return Err(RepositoryError::Storage(format!("unknown lifecycle side: {s}"))),
        };
        Ok(Self {
            id: db.id,
            strategy_instance_id: db.strategy_instance_id,
            symbol: db.symbol,
            side,
            opened_at: db.opened_at,
            closed_at: db.closed_at,
            entry_avg_price: db.entry_avg_price,
            exit_avg_price: db.exit_avg_price,
            total_qty: db.total_qty,
            realized_pnl: db.realized_pnl,
            fee_total: db.fee_total,
            net_pnl: db.net_pnl,
            mfe_pct: db.mfe_pct,
            mae_pct: db.mae_pct,
            entry_atr: db.entry_atr,
            high_water_mark: db.high_water_mark,
            low_water_mark: db.low_water_mark,
        })
    }
}

#[derive(sqlx::FromRow)]
struct DbUnresolvedFill {
    id: Uuid,
    exchange_trade_id: Option<i64>,
    raw_payload: Value,
    status: String,
    reason: Option<String>,
    last_error: Option<String>,
    retry_count: i16,
    inserted_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbUnresolvedFill> for UnresolvedFill {
    fn from(db: DbUnresolvedFill) -> Self {
        let status = match db.status.as_str() {
            "RESOLVED" => UnresolvedStatus::Resolved,
            "DEAD" => UnresolvedStatus::Dead,
            _ => UnresolvedStatus::Pending,
        };
        Self {
            id: db.id,
            exchange_trade_id: db.exchange_trade_id,
            raw_payload: db.raw_payload,
            status,
            reason: db.reason,
            last_error: db.last_error,
            retry_count: db.retry_count,
            inserted_at: db.inserted_at,
            updated_at: db.updated_at,
        }
    }
}

// ── Repository ───────────────────────────────────────────────────────────────

#[async_trait]
impl LifecycleRepository for PostgresLifecycleRepository {
    async fn open(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
        side: LifecycleSide,
        entry_atr: Option<Decimal>,
    ) -> Result<Uuid, RepositoryError> {
        let id = Uuid::now_v7();
        let side_str = match side {
            LifecycleSide::Long => "long",
            LifecycleSide::Short => "short",
        };
        sqlx::query(
            r#"
            INSERT INTO position_lifecycles (id, strategy_instance_id, symbol, side, opened_at, entry_atr)
            VALUES ($1, $2, $3, $4, NOW(), $5)
            "#,
        )
        .bind(id)
        .bind(strategy_instance_id)
        .bind(symbol)
        .bind(side_str)
        .bind(entry_atr)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn close(&self, id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"UPDATE position_lifecycles SET closed_at = NOW() WHERE id = $1"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_open(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
    ) -> Result<Option<PositionLifecycle>, RepositoryError> {
        let row = sqlx::query_as::<_, DbLifecycle>(
            r#"
            SELECT * FROM position_lifecycles
            WHERE strategy_instance_id = $1 AND symbol = $2 AND closed_at IS NULL
            ORDER BY opened_at DESC
            LIMIT 1
            "#,
        )
        .bind(strategy_instance_id)
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await?;
        row.map(PositionLifecycle::try_from).transpose()
    }

    async fn update_water_marks(
        &self,
        id: Uuid,
        high: Option<Decimal>,
        low: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE position_lifecycles
            SET high_water_mark = COALESCE($2, high_water_mark),
                low_water_mark  = COALESCE($3, low_water_mark)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(high)
        .bind(low)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn backfill_close(
        &self,
        id: Uuid,
        exit_avg_price: Decimal,
        realized_pnl: Decimal,
        fee_total: Decimal,
        net_pnl: Decimal,
        mfe_pct: Option<Decimal>,
        mae_pct: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE position_lifecycles
            SET exit_avg_price = $2,
                realized_pnl   = $3,
                fee_total      = $4,
                net_pnl        = $5,
                mfe_pct        = COALESCE($6, mfe_pct),
                mae_pct        = COALESCE($7, mae_pct)
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(exit_avg_price)
        .bind(realized_pnl)
        .bind(fee_total)
        .bind(net_pnl)
        .bind(mfe_pct)
        .bind(mae_pct)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn save_unresolved(
        &self,
        exchange_trade_id: Option<i64>,
        raw_payload: Value,
        reason: &str,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            INSERT INTO unresolved_fills (exchange_trade_id, raw_payload, reason)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(exchange_trade_id)
        .bind(raw_payload)
        .bind(reason)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_unresolved_pending(
        &self,
        limit: i64,
    ) -> Result<Vec<UnresolvedFill>, RepositoryError> {
        Ok(sqlx::query_as::<_, DbUnresolvedFill>(
            r#"
            SELECT id, exchange_trade_id, raw_payload, status, reason, last_error, retry_count, inserted_at, updated_at
            FROM unresolved_fills
            WHERE status = 'PENDING'
            ORDER BY retry_count, inserted_at
            LIMIT $1
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    async fn mark_unresolved_resolved(&self, id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"UPDATE unresolved_fills SET status = 'RESOLVED', updated_at = NOW() WHERE id = $1"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn mark_unresolved_dead(&self, id: Uuid, error: &str) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"UPDATE unresolved_fills SET status = 'DEAD', last_error = $2, updated_at = NOW() WHERE id = $1"#,
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn increment_unresolved_retry(
        &self,
        id: Uuid,
        error: &str,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE unresolved_fills
            SET retry_count = retry_count + 1, last_error = $2, updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
