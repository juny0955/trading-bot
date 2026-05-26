use crate::application::backtest::backtest_executor::{BacktestOrderExecutor, PortfolioSnapshot};
use crate::domain::backtest::{Bar, DepthSnapshot, Event};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use crate::ports::market_data_source::{
    BarInterval, BarQuery, DepthQuery, DepthSampleInterval, MarketDataSource,
};
use crate::ports::order_executor::OrderExecutor;
use anyhow::{Context as AnyhowContext, Result};
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::info;

/// 백테스트 실행 조건입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::engine::BacktestConfig;
/// use trading_bot::ports::market_data_source::{BarInterval, DepthSampleInterval};
///
/// let cfg = BacktestConfig {
///     symbol: "BTCUSDT".into(),
///     from_ns: 0,
///     to_ns: 1,
///     bar_interval: BarInterval::Min(1),
///     depth_every: DepthSampleInterval::Sec(1),
///     initial_equity: dec!(10_000),
///     fee_bps: dec!(4),
///     depth_freshness_sec: 5,
/// };
/// assert_eq!(cfg.symbol, "BTCUSDT");
/// ```
#[derive(Clone)]
pub struct BacktestConfig {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub bar_interval: BarInterval,
    pub depth_every: DepthSampleInterval,
    pub initial_equity: Decimal,
    pub fee_bps: Decimal,
    pub depth_freshness_sec: i64,
}

/// 백테스트 equity curve의 한 시점입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::engine::EquityPoint;
///
/// let point = EquityPoint {
///     ts_ns: 1,
///     equity: dec!(1000),
///     position_qty: dec!(0),
///     mark_price: dec!(100),
/// };
/// assert_eq!(point.equity, dec!(1000));
/// ```
#[derive(Debug, Clone)]
pub struct EquityPoint {
    pub ts_ns: i64,
    pub equity: Decimal,
    pub position_qty: Decimal,
    pub mark_price: Decimal,
}

/// 백테스트 실행 결과입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::backtest_executor::BacktestOrderExecutor;
/// use trading_bot::application::backtest::engine::BacktestResult;
///
/// let executor = BacktestOrderExecutor::new(dec!(1000), dec!(0)).unwrap();
/// let result = BacktestResult {
///     portfolio: executor.snapshot().unwrap(),
///     equity_curve: Vec::new(),
/// };
/// assert!(result.equity_curve.is_empty());
/// ```
pub struct BacktestResult {
    pub portfolio: PortfolioSnapshot,
    pub equity_curve: Vec<EquityPoint>,
}

/// 저장소에서 봉/호가 이벤트를 조회하고 시간 순서로 병합합니다.
///
/// # Examples
///
/// ```ignore
/// # async fn example(src: &dyn trading_bot::ports::market_data_source::MarketDataSource) -> anyhow::Result<()> {
/// # use rust_decimal_macros::dec;
/// # use trading_bot::application::backtest::engine::{fetch_events, BacktestConfig};
/// # use trading_bot::ports::market_data_source::{BarInterval, DepthSampleInterval};
/// let cfg = BacktestConfig {
///     symbol: "BTCUSDT".into(),
///     from_ns: 0,
///     to_ns: 1,
///     bar_interval: BarInterval::Min(1),
///     depth_every: DepthSampleInterval::Sec(1),
///     initial_equity: dec!(1000),
///     fee_bps: dec!(0),
///     depth_freshness_sec: 5,
/// };
/// let events = fetch_events(src, &cfg).await?;
/// # Ok(()) }
/// ```
pub async fn fetch_events(src: &dyn MarketDataSource, cfg: &BacktestConfig) -> Result<Vec<Event>> {
    let (bars, depths) = tokio::join!(
        src.fetch_bars(BarQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            interval: cfg.bar_interval,
        }),
        src.fetch_depth_snapshots(DepthQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            every: cfg.depth_every,
        }),
    );
    let bars = bars?;
    let depths = depths?;
    info!(bars = bars.len(), depths = depths.len(), "fetched");
    Ok(merge_events(bars, depths))
}

async fn run_inner(
    events: &[Event],
    cfg: &BacktestConfig,
    strategy: &mut dyn Strategy,
) -> Result<BacktestResult> {
    let executor = Arc::new(BacktestOrderExecutor::new(cfg.initial_equity, cfg.fee_bps)?);

    if events.is_empty() {
        return Ok(BacktestResult {
            portfolio: executor.snapshot()?,
            equity_curve: Vec::new(),
        });
    }

    let mut equity_curve = Vec::with_capacity(events.len());
    let mut last_depth: Option<&DepthSnapshot> = None;
    let depth_fresh_ns = cfg.depth_freshness_sec.saturating_mul(1_000_000_000);
    // 현재 활성 SL 주문 ID (새 SL 설정 시 이전 주문 취소)
    let mut current_sl: Option<uuid::Uuid> = None;
    let mut current_tp: Option<uuid::Uuid> = None;

    {
        let ctx = make_ctx(&executor, events[0].ts_ns())?;
        let cmds = strategy.on_start(&ctx)?;
        dispatch(&executor, cmds, &mut current_sl, &mut current_tp).await?;
    }

    for ev in events {
        let ts = ev.ts_ns();
        let buy_price = pick_price(ev, last_depth, ts, depth_fresh_ns, OrderSide::Buy);
        let sell_price = pick_price(ev, last_depth, ts, depth_fresh_ns, OrderSide::Sell);
        executor.drain_pending(buy_price, sell_price, ts)?;

        // Bar 이벤트에서 고저가 기준으로 StopMarket 주문 트리거 확인
        if let Event::Bar(b) = ev {
            executor.drain_stops(b.low, b.high, ts)?;
        }

        let mark = event_mark_price(ev);
        executor.mark(mark)?;
        equity_curve.push(equity_point(&executor, ts, mark)?);

        let ctx = make_ctx(&executor, ts)?;
        let cmds = match ev {
            Event::Bar(b) => strategy.on_bar(b, &ctx)?,
            Event::Depth(d) => {
                last_depth = Some(d);
                strategy.on_depth(d, &ctx)?
            }
        };
        dispatch(&executor, cmds, &mut current_sl, &mut current_tp).await?;
    }

    close_open_position(&executor, cfg, events, &mut equity_curve).await?;

    {
        let last = events
            .last()
            .map(Event::ts_ns)
            .context("events must not be empty after early return")?;
        let ctx = make_ctx(&executor, last)?;
        let cmds = strategy.on_finish(&ctx)?;
        dispatch(&executor, cmds, &mut current_sl, &mut current_tp).await?;
    }

    Ok(BacktestResult {
        portfolio: executor.snapshot()?,
        equity_curve,
    })
}

async fn close_open_position(
    executor: &Arc<BacktestOrderExecutor>,
    cfg: &BacktestConfig,
    events: &[Event],
    equity_curve: &mut [EquityPoint],
) -> Result<()> {
    if let Some(cur_side) = executor.position()?.side {
        let last = &events[events.len() - 1];
        let close_price = event_mark_price(last);
        let close_side = match cur_side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        };
        let qty = executor.position()?.qty;
        // 종료 시 잔여 포지션은 마지막 관측 가격으로 시장가 청산한다.
        executor
            .submit(OrderRequest {
                symbol: cfg.symbol.clone(),
                order_type: OrderType::Market,
                side: close_side,
                qty,
                reduce_only: true,
                ..Default::default()
            })
            .await?;
        executor.drain_pending(close_price, close_price, last.ts_ns())?;
        if let Some(p) = equity_curve.last_mut() {
            p.equity = executor.equity()?;
            p.position_qty = Decimal::ZERO;
        }
    }
    Ok(())
}

/// 시장 데이터 소스에서 이벤트를 가져와 전략을 실행합니다.
///
/// # Examples
///
/// ```ignore
/// # async fn example(
/// #     src: &dyn trading_bot::ports::market_data_source::MarketDataSource,
/// #     cfg: trading_bot::application::backtest::engine::BacktestConfig,
/// #     strategy: &mut dyn trading_bot::domain::strategies::Strategy,
/// # ) -> anyhow::Result<()> {
/// let result = trading_bot::application::backtest::engine::run(src, cfg, strategy).await?;
/// assert!(!result.equity_curve.is_empty() || result.portfolio.closed_trades.is_empty());
/// # Ok(()) }
/// ```
pub async fn run(
    src: &dyn MarketDataSource,
    cfg: BacktestConfig,
    strategy: &mut dyn Strategy,
) -> Result<BacktestResult> {
    let events = fetch_events(src, &cfg).await?;
    run_inner(&events, &cfg, strategy).await
}

/// 이미 로드된 이벤트 목록으로 전략을 실행합니다.
///
/// # Examples
///
/// ```ignore
/// # async fn example(
/// #     cfg: trading_bot::application::backtest::engine::BacktestConfig,
/// #     strategy: Box<dyn trading_bot::domain::strategies::Strategy>,
/// # ) -> anyhow::Result<()> {
/// use std::sync::Arc;
///
/// let events = Arc::new(Vec::new());
/// let result = trading_bot::application::backtest::engine::run_on_events(events, cfg, strategy).await?;
/// assert!(result.equity_curve.is_empty());
/// # Ok(()) }
/// ```
pub async fn run_on_events(
    events: Arc<Vec<Event>>,
    cfg: BacktestConfig,
    mut strategy: Box<dyn Strategy>,
) -> Result<BacktestResult> {
    run_inner(&events, &cfg, strategy.as_mut()).await
}

fn make_ctx(executor: &Arc<BacktestOrderExecutor>, ts_ns: i64) -> Result<Context> {
    let (equity, position) = executor.equity_and_position()?;
    Ok(Context {
        now_ns: ts_ns,
        equity,
        position,
        fear_greed: None,
    })
}

fn equity_point(
    executor: &Arc<BacktestOrderExecutor>,
    ts_ns: i64,
    mark_price: Decimal,
) -> Result<EquityPoint> {
    let (equity, position) = executor.equity_and_position()?;
    Ok(EquityPoint {
        ts_ns,
        equity,
        position_qty: position.qty,
        mark_price,
    })
}

async fn dispatch(
    executor: &Arc<BacktestOrderExecutor>,
    cmds: Vec<StrategyCommand>,
    current_sl: &mut Option<uuid::Uuid>,
    current_tp: &mut Option<uuid::Uuid>,
) -> Result<()> {
    for cmd in cmds {
        match cmd {
            StrategyCommand::SubmitOrder(req) => {
                executor.submit(req).await?;
            }
            StrategyCommand::SetStopLoss {
                side,
                qty,
                stop_price,
            } => {
                // 기존 SL 취소 후 새 SL 설정
                if let Some(old_id) = current_sl.take() {
                    executor.cancel_order_id(old_id)?;
                }
                let order = executor
                    .submit(OrderRequest {
                        order_type: OrderType::StopMarket,
                        side,
                        qty,
                        stop_price: Some(stop_price),
                        reduce_only: true,
                        ..Default::default()
                    })
                    .await?;
                *current_sl = Some(order.id);
            }
            StrategyCommand::SetTakeProfit {
                side,
                qty,
                stop_price,
            } => {
                if let Some(old_id) = current_tp.take() {
                    executor.cancel_order_id(old_id)?;
                }
                let order = executor
                    .submit(OrderRequest {
                        order_type: OrderType::StopMarket,
                        side,
                        qty,
                        stop_price: Some(stop_price),
                        reduce_only: true,
                        ..Default::default()
                    })
                    .await?;
                *current_tp = Some(order.id);
            }
        }
    }
    Ok(())
}

fn pick_price(
    ev: &Event,
    last_depth: Option<&DepthSnapshot>,
    now_ns: i64,
    fresh_ns: i64,
    side: OrderSide,
) -> Decimal {
    let select = |d: &DepthSnapshot| match side {
        OrderSide::Buy => d.ask1_price,
        OrderSide::Sell => d.bid1_price,
    };
    if let Some(d) = last_depth.filter(|d| now_ns - d.ts_ns <= fresh_ns) {
        return select(d);
    }
    match ev {
        Event::Bar(b) => b.open,
        Event::Depth(d) => select(d),
    }
}

fn event_mark_price(ev: &Event) -> Decimal {
    match ev {
        Event::Bar(b) => b.close,
        Event::Depth(d) => (d.bid1_price + d.ask1_price) / Decimal::from(2),
    }
}

fn merge_events(bars: Vec<Bar>, depths: Vec<DepthSnapshot>) -> Vec<Event> {
    let mut out: Vec<Event> = bars
        .into_iter()
        .map(Event::Bar)
        .chain(depths.into_iter().map(Event::Depth))
        .collect();

    // 동일 timestamp에서는 기존 동작처럼 Bar를 Depth보다 먼저 처리한다.
    out.sort_by_key(|event| (event.ts_ns(), event_priority(event)));
    out
}

fn event_priority(event: &Event) -> u8 {
    match event {
        Event::Bar(_) => 0,
        Event::Depth(_) => 1,
    }
}
