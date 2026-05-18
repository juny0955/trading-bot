use sqlx::PgPool;

pub async fn init_db() -> anyhow::Result<PgPool> {
    let url = std::env::var("DATABASE_URL").expect("DB URL 없음");
    let pool = PgPool::connect(&url).await?;
    sqlx::raw_sql(include_str!("../../../migrations/init.sql"))
        .execute(&pool)
        .await?;
    Ok(pool)
}
