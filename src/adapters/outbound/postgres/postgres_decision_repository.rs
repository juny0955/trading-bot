use crate::domain::trade_decision::{ExchangeDecisionStatus, TradeDecision, TradeDecisionInsert};
use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::decision_repository::DecisionRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresDecisionRepository {
    pool: PgPool,
}

impl PostgresDecisionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ── DB 모델 ──────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DbTradeDecision {
    id: Uuid,
    position_lifecycle_id: Uuid,
    symbol: String,
    decision_type: String,
    exchange_status: String,
    pnl_after_5m: Option<Decimal>,
    pnl_after_10m: Option<Decimal>,
    pnl_after_30m: Option<Decimal>,
    max_drawdown_until_close: Option<Decimal>,
    decided_at: DateTime<Utc>,
}

impl From<DbTradeDecision> for TradeDecision {
    fn from(db: DbTradeDecision) -> Self {
        let exchange_status = match db.exchange_status.as_str() {
            "ACKED" => ExchangeDecisionStatus::Acked,
            "REJECTED" => ExchangeDecisionStatus::Rejected,
            "TIMEOUT" => ExchangeDecisionStatus::Timeout,
            _ => ExchangeDecisionStatus::Pending,
        };
        Self {
            id: db.id,
            position_lifecycle_id: db.position_lifecycle_id,
            symbol: db.symbol,
            decision_type: db.decision_type,
            exchange_status,
            pnl_after_5m: db.pnl_after_5m,
            pnl_after_10m: db.pnl_after_10m,
            pnl_after_30m: db.pnl_after_30m,
            max_drawdown_until_close: db.max_drawdown_until_close,
            decided_at: db.decided_at,
        }
    }
}

// ── Repository ───────────────────────────────────────────────────────────────

#[async_trait]
impl DecisionRepository for PostgresDecisionRepository {
    async fn insert(&self, d: &TradeDecisionInsert) -> Result<Uuid, RepositoryError> {
        let id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO trade_decisions (
                id, position_lifecycle_id, symbol, decision_type,
                atr, atr_pct, adx, spread_pct, fng_value, ema_alignment, ob_imbalance,
                unrealized_pnl_pct, time_in_trade_secs, dist_to_sl_atr, dist_to_tp_atr, tp_stage,
                action_taken
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9, $10, $11,
                $12, $13, $14, $15, $16,
                $17
            )
            "#,
        )
        .bind(id)
        .bind(d.position_lifecycle_id)
        .bind(&d.symbol)
        .bind(&d.decision_type)
        .bind(d.atr)
        .bind(d.atr_pct)
        .bind(d.adx)
        .bind(d.spread_pct)
        .bind(d.fng_value)
        .bind(d.ema_alignment)
        .bind(d.ob_imbalance)
        .bind(d.unrealized_pnl_pct)
        .bind(d.time_in_trade_secs)
        .bind(d.dist_to_sl_atr)
        .bind(d.dist_to_tp_atr)
        .bind(d.tp_stage)
        .bind(&d.action_taken)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    async fn ack(&self, id: Uuid, latency_ms: i32) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE trade_decisions
            SET exchange_status = 'ACKED', ack_latency_ms = $2
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(latency_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn reject(&self, id: Uuid, error: &str) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE trade_decisions
            SET exchange_status = 'REJECTED', exchange_error = $2
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn timeout(&self, id: Uuid) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"UPDATE trade_decisions SET exchange_status = 'TIMEOUT' WHERE id = $1"#,
        )
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn find_pending_backfill(
        &self,
        limit: i64,
    ) -> Result<Vec<TradeDecision>, RepositoryError> {
        Ok(sqlx::query_as::<_, DbTradeDecision>(
            r#"
            SELECT id, position_lifecycle_id, symbol, decision_type,
                   exchange_status, pnl_after_5m, pnl_after_10m, pnl_after_30m,
                   max_drawdown_until_close, decided_at
            FROM trade_decisions
            WHERE pnl_after_5m IS NULL
            ORDER BY decided_at
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

    async fn backfill_outcome(
        &self,
        id: Uuid,
        pnl_after_5m: Option<Decimal>,
        pnl_after_10m: Option<Decimal>,
        pnl_after_30m: Option<Decimal>,
        max_drawdown_until_close: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            UPDATE trade_decisions
            SET pnl_after_5m             = $2,
                pnl_after_10m            = $3,
                pnl_after_30m            = $4,
                max_drawdown_until_close = $5
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(pnl_after_5m)
        .bind(pnl_after_10m)
        .bind(pnl_after_30m)
        .bind(max_drawdown_until_close)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
