use anyhow::Result;
use async_trait::async_trait;
use futures_util::future::join_all;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use trading_bot::adapters::binance::api::BinanceOrderApi;
use trading_bot::adapters::inbound::binance::binance_market_provider::BinanceMarketProvider;
use trading_bot::adapters::inbound::binance::user_stream::BinanceUserStreamHandler;
use trading_bot::adapters::outbound::binance::order::executor::BinanceOrderExecutor;
use trading_bot::adapters::outbound::binance::order::order_stream::OrderWsLoop;
use trading_bot::adapters::outbound::postgres::postgres_order_repository::PostgresOrderRepository;
use trading_bot::application::account_event_handler::AccountEventHandler;
use trading_bot::application::live_order_service::LiveOrderService;
use trading_bot::application::live_trader::LiveTrader;
use trading_bot::bootstrap::AppBootstrap;
use trading_bot::domain::order::Fill;
use trading_bot::domain::strategies::{Strategy, create_strategy};
use trading_bot::ports::fill_sink::FillSink;
use trading_bot::ports::order_service::OrderService;
use trading_bot::ports::trading_use_case::TradingUseCase;

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
    let boot = AppBootstrap::init().await?;

    let api_key = std::env::var("BINANCE_API_KEY").expect("BINANCE_API_KEY 없음");
    let secret = std::env::var("BINANCE_SECRET").expect("BINANCE_SECRET 없음");

    let (api, ws_rx) = BinanceOrderApi::new(api_key, secret, &boot.binance_cfg)?;

    let repo = Arc::new(PostgresOrderRepository::new(boot.pool.clone()));
    let executor = Arc::new(BinanceOrderExecutor::new(api.clone()));
    let order_svc: Arc<dyn OrderService> = Arc::new(LiveOrderService::new(executor, repo.clone()));
    let trader = Box::new(LiveTrader::new(order_svc, boot.position_repo.clone()));

    let instances = boot.strategy_repo.load_enabled().await?;
    if instances.is_empty() {
        anyhow::bail!("strategy_instances에 enabled 인스턴스가 없습니다");
    }
    let mut strategies: HashMap<(i64, String), Box<dyn Strategy>> = HashMap::new();
    for inst in &instances {
        for symbol in &inst.symbols {
            let strategy = create_strategy(&inst.strategy_type, &inst.params)?;
            strategies.insert((inst.id, symbol.clone()), strategy);
            info!(instance_id = inst.id, name = %inst.name, %symbol, "전략 부팅");
        }
    }
    info!(total_entries = strategies.len(), "트레이더 시작");

    let feed = Box::new(BinanceMarketProvider::new(
        boot.binance_cfg.clone(),
        boot.config.runtime.binance.clone(),
    ));

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
                if let Err(e) = trader.run(feed, strategies, token).await {
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
