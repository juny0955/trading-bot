use crate::order::types::{Fill, Order};
use sqlx::PgPool;
use uuid::Uuid;

pub struct OrderStorage {
    pool: PgPool,
}

impl OrderStorage {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_order(&self, order: &Order) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO orders (
                id, client_order_id, exchange_order_id, symbol,
                order_type, side, status, qty, price, stop_price,
                filled_qty, avg_fill_price, time_in_force,
                reduce_only, post_only, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)
            ON CONFLICT (id) DO UPDATE SET
                exchange_order_id = EXCLUDED.exchange_order_id,
                status            = EXCLUDED.status,
                filled_qty        = EXCLUDED.filled_qty,
                avg_fill_price    = EXCLUDED.avg_fill_price,
                updated_at        = EXCLUDED.updated_at
            "#,
        )
        .bind(order.id)
        .bind(&order.client_order_id)
        .bind(order.exchange_order_id)
        .bind(&order.symbol)
        .bind(order.order_type)
        .bind(order.side)
        .bind(order.status)
        .bind(order.qty)
        .bind(order.price)
        .bind(order.stop_price)
        .bind(order.filled_qty)
        .bind(order.avg_fill_price)
        .bind(order.time_in_force)
        .bind(order.reduce_only)
        .bind(order.post_only)
        .bind(order.created_at)
        .bind(order.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record_fill(&self, fill: &Fill) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO fills (
                order_id, symbol, side, qty, price, fee, fee_asset, filled_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
            "#,
        )
        .bind(fill.order_id)
        .bind(&fill.symbol)
        .bind(fill.side)
        .bind(fill.qty)
        .bind(fill.price)
        .bind(fill.fee)
        .bind(&fill.fee_asset)
        .bind(fill.filled_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, sqlx::Error> {
        sqlx::query_as::<_, Order>(
            r#"SELECT * FROM orders WHERE symbol = $1 AND status IN ('new', 'partially_filled')"#,
        )
        .bind(symbol)
        .fetch_all(&self.pool)
        .await
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, sqlx::Error> {
        sqlx::query_as::<_, Order>(r#"SELECT * FROM orders WHERE id = $1"#)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    pub async fn find_by_exchange_id(
        &self,
        exchange_id: i64,
    ) -> Result<Option<Order>, sqlx::Error> {
        sqlx::query_as::<_, Order>(r#"SELECT * FROM orders WHERE exchange_order_id = $1"#)
            .bind(exchange_id)
            .fetch_optional(&self.pool)
            .await
    }
}
