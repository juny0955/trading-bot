use anyhow::Result;
use trading_bot::adapters::inbound::alternative::alternative_fng_provider::AlternativeFngProvider;
use trading_bot::adapters::inbound::binance::binance_market_provider::BinanceMarketProvider;
use trading_bot::adapters::outbound::questdb::writer::QuestDbSink;
use trading_bot::application::market_data_collector::MarketDataCollector;
use trading_bot::bootstrap::AppBootstrap;
use trading_bot::ports::collection_use_case::CollectionUseCase;

#[tokio::main]
async fn main() -> Result<()> {
    let boot = AppBootstrap::init().await?;

    let quest_db_url = std::env::var("QUESTDB_URL").expect("QuestDB URL 없음");

    let feed = Box::new(BinanceMarketProvider::new(
        boot.binance_cfg.clone(),
        boot.config.runtime.binance.clone(),
    ));
    let fng = Box::new(AlternativeFngProvider::new(
        boot.config.runtime.alternative.clone(),
    ));
    let sink = Box::new(QuestDbSink::new(
        quest_db_url,
        boot.config.runtime.questdb.clone(),
    ));

    Box::new(MarketDataCollector).run(feed, fng, sink).await
}
