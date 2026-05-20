use anyhow::Result;
use trading_bot::adapters::inbound::alternative::fng::AlternativeFngFeed;
use trading_bot::adapters::inbound::binance::market_stream::BinanceLiveFeed;
use trading_bot::adapters::outbound::questdb::writer::QuestDbSink;
use trading_bot::application::collector::Collector;
use trading_bot::bootstrap::AppBootstrap;
use trading_bot::ports::collection_use_case::CollectionUseCase;

#[tokio::main]
async fn main() -> Result<()> {
    let boot = AppBootstrap::init().await?;

    let quest_db_url = std::env::var("QUESTDB_URL").expect("QuestDB URL 없음");

    let feed = Box::new(BinanceLiveFeed::new(
        boot.binance_cfg.clone(),
        boot.config.runtime.binance.clone(),
    ));
    let fng = Box::new(AlternativeFngFeed::new(
        boot.config.runtime.alternative.clone(),
    ));
    let sink = Box::new(QuestDbSink::new(
        quest_db_url,
        boot.config.runtime.questdb.clone(),
    ));

    Box::new(Collector).run(feed, fng, sink).await
}
