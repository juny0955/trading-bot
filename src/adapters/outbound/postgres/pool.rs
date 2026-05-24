use anyhow::Context;
use sqlx::PgPool;

/// PostgreSQL 연결 풀을 생성하고 초기 마이그레이션 SQL을 실행합니다.
///
/// # Examples
///
/// ```ignore
/// let pool = trading_bot::adapters::outbound::postgres::pool::init_db().await?;
/// # anyhow::Ok(())
/// ```
pub async fn init_db() -> anyhow::Result<PgPool> {
    let url = std::env::var("DATABASE_URL").context("DATABASE_URL environment variable missing")?;
    let pool = PgPool::connect(&url).await?;
    sqlx::raw_sql(include_str!("../../../../migrations/init.sql"))
        .execute(&pool)
        .await?;
    Ok(pool)
}
