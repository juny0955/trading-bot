use anyhow::{Context, Result};
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
use trading_bot::adapters::outbound::binance::account_client::BinanceAccountClient;
use trading_bot::adapters::outbound::binance::exchange_info_client::BinanceExchangeInfoClient;
use trading_bot::adapters::outbound::binance::order::executor::BinanceOrderExecutor;
use trading_bot::adapters::outbound::binance::order::order_stream::OrderWsLoop;
use trading_bot::adapters::outbound::postgres::postgres_order_repository::PostgresOrderRepository;
use trading_bot::adapters::outbound::telegram::notifier::TelegramNotifier;
use trading_bot::application::account_bootstrap;
use trading_bot::application::account_event_handler::AccountEventHandler;
use trading_bot::application::account_state_refresher::AccountStateRefresher;
use trading_bot::application::daily_reporter::DailyReporter;
use trading_bot::application::exchange_info_refresher::ExchangeInfoRefresher;
use trading_bot::application::live_trader::LiveTrader;
use trading_bot::application::order_reconciler::OrderReconciler;
use trading_bot::application::outbox_order_service::OutboxOrderService;
use trading_bot::application::risk_controller::RiskController;
use trading_bot::bootstrap::AppBootstrap;
use trading_bot::domain::order::Fill;
use trading_bot::domain::strategies::{Strategy, StrategyInstance, create_strategy};
use trading_bot::ports::fill_sink::FillSink;
use trading_bot::ports::order_executor::OrderExecutor;
use trading_bot::ports::order_repository::OrderRepository;
use trading_bot::ports::order_service::OrderService;
use trading_bot::ports::outbound::notification_sink::{NoopNotificationSink, NotificationSink};
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

    let api_key = std::env::var("BINANCE_API_KEY").context("BINANCE_API_KEY 없음")?;
    let secret = std::env::var("BINANCE_SECRET").context("BINANCE_SECRET 없음")?;
    let (api, ws_rx) = BinanceOrderApi::new(api_key, secret, &boot.binance_cfg)?;

    let exchange_info_client = Arc::new(BinanceExchangeInfoClient::new(api.clone()));
    let account_client = Arc::new(BinanceAccountClient::new(api.clone()));
    let repo: Arc<dyn OrderRepository> = Arc::new(PostgresOrderRepository::new(boot.pool.clone()));
    let executor: Arc<dyn OrderExecutor> = Arc::new(BinanceOrderExecutor::new(api.clone()));

    if !boot.config.runtime.trading.live_trading_enabled {
        anyhow::bail!(
            "실거래가 비활성화되어 있습니다. \
            runtime_config에 (type='trading', key='live_trading_enabled', value='true')를 추가하세요."
        );
    }

    let instances = boot.strategy_repo.load_enabled().await?;
    if instances.is_empty() {
        anyhow::bail!("strategy_instances에 enabled 인스턴스가 없습니다");
    }
    let symbols = collect_symbols(&instances);

    sync_state(
        &boot,
        &exchange_info_client,
        &account_client,
        &*executor,
        &*repo,
        &symbols,
        &instances,
    )
    .await;

    let risk = Arc::new(RiskController::new(
        boot.risk_limits.clone(),
        boot.filters_cache.clone(),
        boot.account_state.clone(),
        repo.clone(),
        boot.position_repo.clone(),
        boot.book_ticker_cache.clone(),
    ));
    let order_svc: Arc<dyn OrderService> = Arc::new(
        OutboxOrderService::new(executor.clone(), repo.clone(), risk)
            .with_dry_run(boot.config.runtime.trading.dry_run),
    );

    let strategies = build_strategies(&instances)?;
    info!(total_entries = strategies.len(), "트레이더 시작");

    let feed = Box::new(BinanceMarketProvider::new(
        boot.binance_cfg.clone(),
        boot.config.runtime.binance.clone(),
    ));

    let (event_tx, event_rx) = mpsc::channel(256);
    let fill_sink = Arc::new(LogFillSink);

    // bot_token이 설정돼 있으면 텔레그램 알리미, 없으면 Noop 사용
    let notifier: Arc<dyn NotificationSink> = {
        let tg = &boot.config.runtime.telegram;
        if !tg.bot_token.is_empty() && !tg.chat_id.is_empty() {
            info!("텔레그램 알림 활성화");
            Arc::new(TelegramNotifier::new(&tg.bot_token, &tg.chat_id))
        } else {
            Arc::new(NoopNotificationSink)
        }
    };

    let handler = Arc::new(AccountEventHandler::new(
        repo.clone(),
        boot.position_repo.clone(),
        fill_sink,
        notifier.clone(),
    ));
    let user_stream = BinanceUserStreamHandler::new(api.clone());
    let order_loop = OrderWsLoop::new(api);
    let token = CancellationToken::new();

    let trader = Box::new(LiveTrader::new(
        order_svc.clone(),
        boot.position_repo.clone(),
        boot.book_ticker_cache.clone(),
        boot.account_state.clone(),
    ));
    let reconciler = OrderReconciler::new(
        order_svc,
        executor,
        repo.clone(),
        boot.risk_limits.clone(),
        symbols,
    );
    let info_refresher =
        ExchangeInfoRefresher::new(exchange_info_client, boot.filters_cache.clone());
    let account_refresher = AccountStateRefresher::new(account_client, boot.account_state.clone());

    let mut handles = vec![
        tokio::spawn({
            let token = token.clone();
            async move { order_loop.run_order_stream(ws_rx, token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            let account_state = boot.account_state.clone();
            async move {
                user_stream
                    .run_user_stream(event_tx, account_state, token)
                    .await
            }
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
        tokio::spawn({
            let token = token.clone();
            async move { reconciler.run(token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            async move { info_refresher.run(token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            async move { account_refresher.run(token).await }
        }),
        tokio::spawn({
            let token = token.clone();
            let reporter = DailyReporter::new(repo.clone(), notifier);
            async move { reporter.run(token).await }
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

fn collect_symbols(instances: &[StrategyInstance]) -> Vec<String> {
    let mut symbols: Vec<String> = instances
        .iter()
        .flat_map(|inst| inst.symbols.iter().cloned())
        .collect();
    symbols.sort();
    symbols.dedup();
    symbols
}

async fn sync_state(
    boot: &AppBootstrap,
    exchange_info_client: &BinanceExchangeInfoClient,
    account_client: &BinanceAccountClient,
    executor: &dyn OrderExecutor,
    repo: &dyn OrderRepository,
    symbols: &[String],
    instances: &[StrategyInstance],
) {
    match boot.filters_cache.refresh(exchange_info_client).await {
        Ok(n) => info!(count = n, "ExchangeInfo 로드 완료"),
        Err(e) => warn!(error = %e, "ExchangeInfo 초기 로드 실패 — 계속 진행"),
    }

    match account_bootstrap::bootstrap(
        account_client,
        executor,
        repo,
        &*boot.position_repo,
        instances,
        symbols,
        &boot.binance_cfg.symbols,
    )
    .await
    {
        Ok(snap) => {
            boot.account_state.try_replace(snap);
            info!("계정 동기화 완료");
        }
        Err(e) => warn!(error = %e, "계정 동기화 실패 — 계속 진행"),
    }
}

fn build_strategies(
    instances: &[StrategyInstance],
) -> Result<HashMap<(i64, String), Box<dyn Strategy>>> {
    let mut strategies = HashMap::new();
    for inst in instances {
        for symbol in &inst.symbols {
            let strategy = create_strategy(&inst.strategy_type, &inst.params)?;
            strategies.insert((inst.id, symbol.clone()), strategy);
            info!(instance_id = inst.id, name = %inst.name, %symbol, "전략 부팅");
        }
    }
    Ok(strategies)
}
