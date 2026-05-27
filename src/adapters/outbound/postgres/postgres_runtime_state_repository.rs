use crate::domain::lifecycle::PositionRuntimeState;
use crate::ports::order_repository::RepositoryError;
use crate::ports::outbound::runtime_state_repository::RuntimeStateRepository;
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

pub struct PostgresRuntimeStateRepository {
    pool: PgPool,
}

impl PostgresRuntimeStateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RuntimeStateRepository for PostgresRuntimeStateRepository {
    async fn upsert(
        &self,
        strategy_instance_id: i64,
        symbol: &str,
        state: &PositionRuntimeState,
        entry_atr: Option<Decimal>,
    ) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            INSERT INTO position_runtime_states
                (strategy_instance_id, symbol, tp_stage, sl_order_id,
                 pending_reduce_order_id, high_water_mark, entry_atr,
                 desired_sl_price, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            ON CONFLICT (strategy_instance_id, symbol) DO UPDATE SET
                tp_stage                = EXCLUDED.tp_stage,
                sl_order_id             = EXCLUDED.sl_order_id,
                pending_reduce_order_id = EXCLUDED.pending_reduce_order_id,
                high_water_mark         = EXCLUDED.high_water_mark,
                entry_atr               = COALESCE(EXCLUDED.entry_atr, position_runtime_states.entry_atr),
                desired_sl_price        = EXCLUDED.desired_sl_price,
                updated_at              = NOW()
            "#,
        )
        .bind(strategy_instance_id)
        .bind(symbol)
        .bind(state.tp_stage as i16)
        .bind(state.sl_order_id)
        .bind(state.pending_reduce_order_id)
        .bind(state.high_water_mark)
        .bind(entry_atr)
        .bind(state.desired_sl_price)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, strategy_instance_id: i64, symbol: &str) -> Result<(), RepositoryError> {
        sqlx::query(
            "DELETE FROM position_runtime_states WHERE strategy_instance_id = $1 AND symbol = $2",
        )
        .bind(strategy_instance_id)
        .bind(symbol)
        .execute(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn load_all(
        &self,
    ) -> Result<Vec<(i64, String, PositionRuntimeState, Option<Decimal>)>, RepositoryError> {
        let rows: Vec<(
            i64,
            String,
            i16,
            Option<Uuid>,
            Option<Uuid>,
            Option<Decimal>,
            Option<Decimal>,
            Option<Decimal>,
        )> = sqlx::query_as(
            r#"
                SELECT strategy_instance_id, symbol, tp_stage, sl_order_id,
                       pending_reduce_order_id, high_water_mark, entry_atr, desired_sl_price
                FROM position_runtime_states
                "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    instance_id,
                    symbol,
                    tp_stage,
                    sl_order_id,
                    pending_reduce_order_id,
                    high_water_mark,
                    entry_atr,
                    desired_sl_price,
                )| {
                    let state = PositionRuntimeState {
                        tp_stage: tp_stage as u8,
                        sl_order_id,
                        pending_reduce_order_id,
                        desired_sl_price,
                        high_water_mark,
                        // 재시작 시 transient 필드 리셋
                        sl_replace_in_flight: false,
                        sl_retry_count: 0,
                        unprotected_since: None,
                    };
                    (instance_id, symbol, state, entry_atr)
                },
            )
            .collect())
    }
}
