use crate::domain::order::{
    Fill, Order, OrderError, OrderSide, OrderStatus, OrderType, TimeInForce,
};
use crate::ports::order_repository::OrderRepository;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

// ── DB 모델 ──────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct DbOrder {
    id: Uuid,
    client_order_id: String,
    exchange_order_id: Option<i64>,
    symbol: String,
    order_type: OrderType,
    side: OrderSide,
    status: OrderStatus,
    qty: Decimal,
    price: Option<Decimal>,
    stop_price: Option<Decimal>,
    filled_qty: Decimal,
    avg_fill_price: Option<Decimal>,
    time_in_force: TimeInForce,
    reduce_only: bool,
    post_only: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<DbOrder> for Order {
    fn from(db: DbOrder) -> Self {
        Self {
            id: db.id,
            client_order_id: db.client_order_id,
            exchange_order_id: db.exchange_order_id,
            symbol: db.symbol,
            order_type: db.order_type,
            side: db.side,
            status: db.status,
            qty: db.qty,
            price: db.price,
            stop_price: db.stop_price,
            filled_qty: db.filled_qty,
            avg_fill_price: db.avg_fill_price,
            time_in_force: db.time_in_force,
            reduce_only: db.reduce_only,
            post_only: db.post_only,
            created_at: db.created_at,
            updated_at: db.updated_at,
        }
    }
}

#[derive(sqlx::FromRow)]
struct DbFill {
    order_id: Uuid,
    symbol: String,
    side: OrderSide,
    qty: Decimal,
    price: Decimal,
    fee: Decimal,
    fee_asset: String,
    filled_at: DateTime<Utc>,
}

impl From<DbFill> for Fill {
    fn from(db: DbFill) -> Self {
        Self {
            order_id: db.order_id,
            symbol: db.symbol,
            side: db.side,
            qty: db.qty,
            price: db.price,
            fee: db.fee,
            fee_asset: db.fee_asset,
            filled_at: db.filled_at,
        }
    }
}

// ── Repository ───────────────────────────────────────────────────────────────

pub struct PgOrderRepository {
    pool: PgPool,
}

#[async_trait]
impl OrderRepository for PgOrderRepository {
    async fn upsert_order(&self, order: &Order) -> Result<(), OrderError> {
        Self::upsert_order(self, order).await
    }

    async fn record_fill(&self, fill: &Fill) -> Result<(), OrderError> {
        Self::record_fill(self, fill).await
    }

    async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, OrderError> {
        Self::find_open(self, symbol).await
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, OrderError> {
        Self::find_by_id(self, id).await
    }

    async fn find_by_exchange_id(&self, exchange_id: i64) -> Result<Option<Order>, OrderError> {
        Self::find_by_exchange_id(self, exchange_id).await
    }
}

impl PgOrderRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_order(&self, order: &Order) -> Result<(), OrderError> {
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

    pub async fn record_fill(&self, fill: &Fill) -> Result<(), OrderError> {
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

    pub async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, OrderError> {
        Ok(sqlx::query_as::<_, DbOrder>(
            r#"SELECT * FROM orders WHERE symbol = $1 AND status IN ('new', 'partially_filled')"#,
        )
        .bind(symbol)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
    }

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, OrderError> {
        Ok(
            sqlx::query_as::<_, DbOrder>(r#"SELECT * FROM orders WHERE id = $1"#)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .map(Into::into),
        )
    }

    pub async fn find_by_exchange_id(&self, exchange_id: i64) -> Result<Option<Order>, OrderError> {
        Ok(
            sqlx::query_as::<_, DbOrder>(r#"SELECT * FROM orders WHERE exchange_order_id = $1"#)
                .bind(exchange_id)
                .fetch_optional(&self.pool)
                .await?
                .map(Into::into),
        )
    }
}
