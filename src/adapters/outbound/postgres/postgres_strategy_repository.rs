use crate::domain::strategies::StrategyInstance;
use crate::ports::strategy_repository::StrategyRepository;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::PgPool;

/// PostgreSQL 기반 전략 인스턴스 저장소입니다.
///
/// # Examples
///
/// ```ignore
/// # use sqlx::PgPool;
/// # use trading_bot::adapters::outbound::postgres::postgres_strategy_repository::PostgresStrategyRepository;
/// # fn example(pool: PgPool) {
/// let repo = PostgresStrategyRepository::new(pool);
/// # }
/// ```
pub struct PostgresStrategyRepository {
    pool: PgPool,
}

impl PostgresStrategyRepository {
    /// 새 PostgreSQL 전략 저장소를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use sqlx::PgPool;
    /// # use trading_bot::adapters::outbound::postgres::postgres_strategy_repository::PostgresStrategyRepository;
    /// # fn example(pool: PgPool) {
    /// let repo = PostgresStrategyRepository::new(pool);
    /// # }
    /// ```
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct DbStrategyInstance {
    id: i64,
    strategy_type: String,
    name: String,
    params: Value,
    symbols: Vec<String>,
}

impl From<DbStrategyInstance> for StrategyInstance {
    fn from(db: DbStrategyInstance) -> Self {
        Self {
            id: db.id,
            strategy_type: db.strategy_type,
            name: db.name,
            params: db.params,
            symbols: db.symbols,
        }
    }
}

#[async_trait]
impl StrategyRepository for PostgresStrategyRepository {
    async fn load_enabled(&self) -> anyhow::Result<Vec<StrategyInstance>> {
        let rows = sqlx::query_as::<_, DbStrategyInstance>(
            r#"
                SELECT si.id,
                       si.strategy_type,
                       si.name,
                       si.params,
                       array_agg(sis.symbol ORDER BY sis.symbol) AS symbols
                FROM strategy_instances si
                JOIN strategy_instance_symbols sis ON sis.strategy_instance_id = si.id
                WHERE si.enabled = TRUE AND sis.enabled = TRUE
                GROUP BY si.id
                "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}
