use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use futures_util::future::join_all;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use trading_bot::adapters::binance::api::BinanceOrderApi;
use trading_bot::adapters::inbound::binance::market_stream::BinanceLiveFeed;
use trading_bot::adapters::outbound::binance::order::executor::BinanceOrderExecutor;
use trading_bot::adapters::outbound::binance::order::order_stream::OrderWsLoop;
use trading_bot::adapters::inbound::binance::user_stream::BinanceUserStreamHandler;
use trading_bot::adapters::outbound::postgres::config_repo::PgConfigRepository;
use trading_bot::adapters::outbound::postgres::order_repo::PgOrderRepository;
use trading_bot::adapters::outbound::postgres::pool::init_db;
use trading_bot::application::account_event_handler::AccountEventHandler;
use trading_bot::application::live_order_service::LiveOrderService;
use trading_bot::application::trader;
use trading_bot::config::binance_config::BinanceNet;
use trading_bot::domain::backtest::Position;
use trading_bot::domain::order::Fill;
use trading_bot::domain::strategies::create_strategy;
use trading_bot::init;
use trading_bot::ports::config_repository::ConfigRepository;
use trading_bot::ports::fill_sink::FillSink;

#[derive(Parser)]
#[command(name = "trader")]
struct Args {
    #[arg(long)]
    strategy: String,
    #[arg(long, default_value = "{}")]
    params: String,
}

struct LogFillSink;

#[async_trait]
impl FillSink for LogFillSink {
    async fn emit(&self, fill: Fill) {
        info!(
            symbol = %fill.symbol,
            side = ?fill.side,
            qty = %fill.qty,
            price = %fill.price,
            fee = %fill.fee,
            "체결"
        );
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init::setup();
    let args = Args::parse();

    let pool = init_db().await?;
    let cfg = Arc::new(PgConfigRepository::new(pool.clone()).load().await?);

    let api_key = std::env::var("BINANCE_API_KEY").expect("BINANCE_API_KEY 없음");
    let secret = std::env::var("BINANCE_SECRET").expect("BINANCE_SECRET 없음");
    let testnet = matches!(cfg.binance.net, BinanceNet::Testnet);

    let (api, ws_rx) = BinanceOrderApi::new(api_key, secret, testnet);

    let repo = Arc::new(PgOrderRepository::new(pool));
    let executor = Arc::new(BinanceOrderExecutor::new(api.clone()));
    let order_svc = Arc::new(LiveOrderService::new(executor, repo.clone()));

    let feed = Box::new(BinanceLiveFeed::new(
        cfg.binance.clone(),
        cfg.runtime.binance.clone(),
    ));

    let params: serde_json::Value = serde_json::from_str(&args.params)?;
    let strategy = create_strategy(&args.strategy, &params)?;

    let position = Arc::new(Mutex::new(Position::default()));
    let (event_tx, event_rx) = mpsc::channel(256);
    let fill_sink = Arc::new(LogFillSink);
    let handler = Arc::new(AccountEventHandler::new(repo, fill_sink));
    let user_stream = BinanceUserStreamHandler::new(api.clone());
    let order_loop = OrderWsLoop::new(api);
    let token = CancellationToken::new();

    let mut handles = vec![
        tokio::spawn({
            let token = token.clone();
            async move { order_loop.run_order_stream(ws_rx, token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            async move { user_stream.run_user_stream(event_tx, token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            let handler = handler.clone();
            async move { handler.run(event_rx, token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            async move {
                if let Err(e) = trader::run(feed, strategy, order_svc, position, token).await {
                    error!("trader 종료: {e}");
                }
            }
        }),
    ];

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C graceful shutdown 시작");
            token.cancel();
        }
        _ = join_all(handles.iter_mut()) => {
            warn!("핸들 조기 종료 - 전체 shutdown");
        }
    }

    token.cancel();
    let _ = join_all(handles).await;

    Ok(())
}
