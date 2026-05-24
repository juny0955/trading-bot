use crate::domain::order::{Fill, Order, OrderSide, OrderStatus, OrderType, TimeInForce};
use crate::ports::order_repository::{OrderRepository, RepositoryError};

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
    strategy_instance_id: i64,
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
            strategy_instance_id: db.strategy_instance_id,
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
    strategy_instance_id: i64,
    symbol: String,
    side: OrderSide,
    qty: Decimal,
    price: Decimal,
    fee: Decimal,
    fee_asset: String,
    filled_at: DateTime<Utc>,
    exchange_trade_id: Option<i64>,
}

impl From<DbFill> for Fill {
    fn from(db: DbFill) -> Self {
        Self {
            order_id: db.order_id,
            strategy_instance_id: db.strategy_instance_id,
            symbol: db.symbol,
            side: db.side,
            qty: db.qty,
            price: db.price,
            fee: db.fee,
            fee_asset: db.fee_asset,
            filled_at: db.filled_at,
            exchange_trade_id: db.exchange_trade_id.unwrap_or(-1),
        }
    }
}

// ── Repository ───────────────────────────────────────────────────────────────

/// PostgreSQL 기반 주문/체결 저장소입니다.
///
/// # Examples
///
/// ```ignore
/// # use sqlx::PgPool;
/// # use trading_bot::adapters::outbound::postgres::postgres_order_repository::PostgresOrderRepository;
/// # fn example(pool: PgPool) {
/// let repo = PostgresOrderRepository::new(pool);
/// # }
/// ```
pub struct PostgresOrderRepository {
    pool: PgPool,
}

#[async_trait]
impl OrderRepository for PostgresOrderRepository {
    async fn upsert_order(&self, order: &Order) -> Result<(), RepositoryError> {
        Self::upsert_order(self, order).await
    }

    async fn record_fill(&self, fill: &Fill) -> Result<(), RepositoryError> {
        Self::record_fill(self, fill).await
    }

    async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, RepositoryError> {
        Self::find_open(self, symbol).await
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepositoryError> {
        Self::find_by_id(self, id).await
    }

    async fn find_by_exchange_id(
        &self,
        exchange_id: i64,
    ) -> Result<Option<Order>, RepositoryError> {
        Self::find_by_exchange_id(self, exchange_id).await
    }

    async fn find_by_status(&self, status: OrderStatus) -> Result<Vec<Order>, RepositoryError> {
        Self::find_by_status(self, status).await
    }
}

impl PostgresOrderRepository {
    /// 새 PostgreSQL 주문 저장소를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use sqlx::PgPool;
    /// # use trading_bot::adapters::outbound::postgres::postgres_order_repository::PostgresOrderRepository;
    /// # fn example(pool: PgPool) {
    /// let repo = PostgresOrderRepository::new(pool);
    /// # }
    /// ```
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn upsert_order(&self, order: &Order) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            INSERT INTO orders (
                id, client_order_id, exchange_order_id, strategy_instance_id,
                symbol, order_type, side, status, qty, price, stop_price,
                filled_qty, avg_fill_price, time_in_force,
                reduce_only, post_only, created_at, updated_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)
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
        .bind(order.strategy_instance_id)
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

    pub async fn record_fill(&self, fill: &Fill) -> Result<(), RepositoryError> {
        sqlx::query(
            r#"
            INSERT INTO fills (
                order_id, strategy_instance_id, symbol, side, qty, price, fee, fee_asset, filled_at, exchange_trade_id
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)
            ON CONFLICT (order_id, exchange_trade_id)
            WHERE exchange_trade_id IS NOT NULL DO NOTHING
            "#,
        )
        .bind(fill.order_id)
        .bind(fill.strategy_instance_id)
        .bind(&fill.symbol)
        .bind(fill.side)
        .bind(fill.qty)
        .bind(fill.price)
        .bind(fill.fee)
        .bind(&fill.fee_asset)
        .bind(fill.filled_at)
        .bind(if fill.exchange_trade_id >= 0 { Some(fill.exchange_trade_id) } else { None })
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn find_open(&self, symbol: &str) -> Result<Vec<Order>, RepositoryError> {
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

    pub async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepositoryError> {
        Ok(
            sqlx::query_as::<_, DbOrder>(r#"SELECT * FROM orders WHERE id = $1"#)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .map(Into::into),
        )
    }

    pub async fn find_by_exchange_id(
        &self,
        exchange_id: i64,
    ) -> Result<Option<Order>, RepositoryError> {
        Ok(
            sqlx::query_as::<_, DbOrder>(r#"SELECT * FROM orders WHERE exchange_order_id = $1"#)
                .bind(exchange_id)
                .fetch_optional(&self.pool)
                .await?
                .map(Into::into),
        )
    }

    pub async fn find_by_status(
        &self,
        status: OrderStatus,
    ) -> Result<Vec<Order>, RepositoryError> {
        Ok(
            sqlx::query_as::<_, DbOrder>(r#"SELECT * FROM orders WHERE status = $1"#)
                .bind(status)
                .fetch_all(&self.pool)
                .await?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }
}
