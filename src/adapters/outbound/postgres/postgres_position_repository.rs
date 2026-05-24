use crate::domain::order::{Fill, OrderSide};
use crate::domain::position::{Position, StoredPosition};
use crate::ports::order_repository::RepositoryError;
use crate::ports::position_repository::PositionRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;

pub struct PostgresPositionRepository {
    pool: PgPool,
}

impl PostgresPositionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct DbPosition {
    strategy_instance_id: i64,
    symbol: String,
    side: OrderSide, // DB는 NOT NULL → 항상 값 있음
    qty: Decimal,
    avg_entry_price: Decimal,
    realized_pnl: Decimal,
    last_fill_id: Option<i64>,
    updated_at: DateTime<Utc>,
}

impl From<DbPosition> for StoredPosition {
    fn from(db: DbPosition) -> Self {
        // qty == 0 → flat 으로 통일 (DB side는 무의미)
        let side = if db.qty.is_zero() {
            None
        } else {
            Some(db.side)
        };
        Self {
            strategy_instance_id: db.strategy_instance_id,
            symbol: db.symbol,
            position: Position {
                side,
                qty: db.qty,
                avg_entry_price: db.avg_entry_price,
                realized_pnl: db.realized_pnl,
            },
            last_fill_id: db.last_fill_id,
            updated_at: db.updated_at,
        }
    }
}

#[async_trait]
impl PositionRepository for PostgresPositionRepository {
    async fn get(
        &self,
        instance_id: i64,
        symbol: &str,
    ) -> Result<Option<StoredPosition>, RepositoryError> {
        Ok(sqlx::query_as::<_, DbPosition>(
            r#"
                    SELECT * FROM positions WHERE strategy_instance_id = $1 AND symbol = $2
                "#,
        )
        .bind(instance_id)
        .bind(symbol)
        .fetch_optional(&self.pool)
        .await?
        .map(Into::into))
    }

    async fn apply_fill(
        &self,
        instance_id: i64,
        fill: &Fill,
    ) -> Result<StoredPosition, RepositoryError> {
        let mut tx = self.pool.begin().await?;

        let cur_db = sqlx::query_as::<_, DbPosition>(
            r#"
                    SELECT * FROM positions
                    WHERE strategy_instance_id = $1 AND symbol = $2
                    FOR UPDATE
                "#,
        )
        .bind(instance_id)
        .bind(&fill.symbol)
        .fetch_optional(&mut *tx)
        .await?;

        let prior_side_for_db: Option<OrderSide> = cur_db.as_ref().map(|d| d.side);
        let cur: StoredPosition = cur_db
            .map(StoredPosition::from)
            .unwrap_or_else(|| StoredPosition::flat(instance_id, fill.symbol.clone()));

        let next_core = cur
            .position
            .apply(fill)
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        let next = StoredPosition {
            strategy_instance_id: cur.strategy_instance_id,
            symbol: cur.symbol,
            position: next_core,
            last_fill_id: None,
            updated_at: Utc::now(),
        };

        let db_side = next
            .position
            .side
            .or(prior_side_for_db)
            .unwrap_or(fill.side);

        sqlx::query(
            r#"
                    INSERT INTO positions (
                        strategy_instance_id, symbol, side, qty, avg_entry_price,
                        realized_pnl, last_fill_id, updated_at
                    )
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                    ON CONFLICT (strategy_instance_id, symbol) DO UPDATE SET
                        side = EXCLUDED.side,
                        qty = EXCLUDED.qty,
                        avg_entry_price = EXCLUDED.avg_entry_price,
                        realized_pnl    = EXCLUDED.realized_pnl,
                        last_fill_id    = EXCLUDED.last_fill_id,
                        updated_at      = EXCLUDED.updated_at
            "#,
        )
        .bind(next.strategy_instance_id)
        .bind(&next.symbol)
        .bind(db_side)
        .bind(next.position.qty)
        .bind(next.position.avg_entry_price)
        .bind(next.position.realized_pnl)
        .bind(next.last_fill_id)
        .bind(next.updated_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(next)
    }
}
