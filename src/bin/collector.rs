use anyhow::Result;
use std::sync::Arc;
use trading_bot::adapters::inbound::alternative::fng::AlternativeFngFeed;
use trading_bot::adapters::inbound::binance::market_stream::BinanceLiveFeed;
use trading_bot::adapters::outbound::postgres::config_repo::PgConfigRepository;
use trading_bot::adapters::outbound::postgres::pool::init_db;
use trading_bot::adapters::outbound::questdb::writer::QuestDbSink;
use trading_bot::application::collector::Collector;
use trading_bot::init::setup;
use trading_bot::ports::collection_use_case::CollectionUseCase;
use trading_bot::ports::config_repository::ConfigRepository;

#[tokio::main]
async fn main() -> Result<()> {
    setup();

    let pool = init_db().await?;
    let cfg = Arc::new(PgConfigRepository::new(pool).load().await?);

    let quest_db_url = std::env::var("QUESTDB_URL").expect("QuestDB URL 없음");

    let feed = Box::new(BinanceLiveFeed::new(
        cfg.binance.clone(),
        cfg.runtime.binance.clone(),
    ));
    let fng = Box::new(AlternativeFngFeed::new(cfg.runtime.alternative.clone()));
    let sink = Box::new(QuestDbSink::new(quest_db_url, cfg.runtime.questdb.clone()));

    Box::new(Collector).run(feed, fng, sink).await
}
